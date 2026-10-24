//! Read-only catalog access for MCP. No GUI, preference migration or saved calculations.
mod calculation;
mod content;
mod facility;
mod names;
mod query;
mod relations;
mod search;
pub(in crate::db) use names::NameIndex;
pub use search::SearchFilters;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use serde_json::{json, Value};

use super::{
    alias::DEFAULT_ALIAS_LINE,
    rows::{Index, ItemRow},
    terminology,
    text::{caption, lead3, trail3},
    CalcCatalog, Catalog, Hit,
};

fn language_code(lang: &str) -> &str {
    lang.split('-').next().unwrap_or("zh")
}

pub fn page(rows: Vec<Value>, offset: usize, limit: usize) -> Value {
    page_map(rows, offset, limit, std::convert::identity)
}

// Select the page before resolving documents, locations or JSON for each row.
fn page_map<T>(rows: Vec<T>, offset: usize, limit: usize, format: impl FnMut(T) -> Value) -> Value {
    let total = rows.len();
    let results: Vec<_> = rows
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(format)
        .collect();
    let next = offset.saturating_add(results.len());
    json!({"results": results, "total": total, "offset": offset,
        "next_offset": if next < total { Some(next) } else { None }})
}

fn item_hit(row: &ItemRow, lang: &str) -> Value {
    // Match Index::item_hits without copying artwork that MCP does not return.
    json!({
        "key": format!("item:{}", row.code_str),
        "name": lead3(lang, &row.en, &row.zh, &row.ko),
        "description": caption(&trail3(lang, &row.en, &row.zh, &row.ko), format!("#{}", row.code_str)),
        "note": "", "group": "", "solo_drop": false,
    })
}

fn exact_name(folded: &str, names: [&str; 3]) -> bool {
    names
        .into_iter()
        .any(|name| !name.is_empty() && name.trim().to_lowercase() == folded)
}

fn hit(row: Hit) -> Value {
    json!({"key": row.key, "name": row.title, "description": row.subtitle,
        "note": row.aside, "group": row.chip, "solo_drop": row.solo})
}

fn facts(rows: Vec<(String, String)>) -> Vec<Value> {
    rows.into_iter()
        .map(|(label, value)| json!({"label": label, "value": value}))
        .collect()
}

type DropSlots = Vec<(i64, Option<i64>)>;

fn item_drop_sources(
    conn: &rusqlite::Connection,
    code: i64,
) -> Result<BTreeMap<String, DropSlots>, String> {
    let rate_column = super::monster::drop_rate_column(conn);
    let mut statement = conn
        .prepare(&format!(
            "SELECT d.monster_code, d.slot, {rate_column} FROM monster_drops d WHERE d.item_code = ?1 ORDER BY d.monster_code, d.slot"
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([code], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    let mut sources: BTreeMap<String, DropSlots> = BTreeMap::new();
    for row in rows {
        let (monster, slot, rate) = row.map_err(|error| error.to_string())?;
        sources.entry(monster).or_default().push((slot, rate));
    }
    Ok(sources)
}

fn drop_rates(rate: Option<i64>) -> Value {
    let rate = rate.filter(|value| (0..=100_000).contains(value));
    json!({
        "drop_rate": rate.map(|value| value as f64 / 100_000.0),
        "drop_rate_percent": rate.map(|value| value as f64 / 1_000.0),
        "drop_rate_per_100000": rate,
    })
}

fn terminology_key_exists(index: &Index, key: &str) -> bool {
    let Some((kind, id)) = key.split_once(':') else {
        return false;
    };
    match kind {
        "hero" => index.heroes.iter().any(|row| row.code == id),
        "monster" => index.monsters.iter().any(|row| row.code == id),
        "item" => id
            .parse()
            .ok()
            .is_some_and(|id| index.find_item(id).is_some()),
        "npc" => index
            .npcs
            .iter()
            .any(|row| Some(row.code) == id.parse().ok()),
        "set" => index
            .sets
            .iter()
            .any(|row| Some(row.code) == id.parse().ok()),
        "recipe" => index
            .recipes
            .iter()
            .any(|row| Some(row.id) == id.parse().ok()),
        "quest" => index
            .quests
            .iter()
            .any(|row| Some(row.id) == id.parse().ok()),
        "town" => index.towns.iter().any(|row| row.code == id),
        "bay" => index.yards.contains_key(id),
        "yard" => index.works.iter().any(|row| row.code == id),
        _ => false,
    }
}

fn drop_details(slots: &[(i64, Option<i64>)]) -> Value {
    // Multiple slots can have different conditions or independent rolls. Do not
    // turn their rates into an unsupported aggregate probability.
    let mut result = drop_rates(if slots.len() == 1 { slots[0].1 } else { None });
    result["solo_only"] = json!(slots.iter().all(|(slot, _)| *slot >= 9));
    result["drops"] = json!(slots
        .iter()
        .map(|&(slot, rate)| {
            let mut entry = drop_rates(rate);
            entry["slot"] = json!(slot);
            entry["solo_only"] = json!(slot >= 9);
            entry
        })
        .collect::<Vec<_>>());
    result
}

impl Catalog {
    pub fn open_for_mcp() -> Result<Self, String> {
        let external = std::env::var_os("GERSANG_TERMINOLOGY").map(std::path::PathBuf::from);
        Self::open_mcp_path_with_terms(&database_path()?, external.as_deref())
    }

    fn open_mcp_path_with_terms(path: &Path, external: Option<&Path>) -> Result<Self, String> {
        let conn =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|error| error.to_string())?;
        let data = path.parent().unwrap_or(Path::new("."));
        // MCP does not need optional game assets or the user's window preferences.
        let client_root = data.join("client");
        let pictures = HashMap::new();
        #[cfg(windows)]
        let aliases_path = std::env::var_os("LOCALAPPDATA")
            .map(|root| std::path::PathBuf::from(root).join("gs-widget/aliases.txt"))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| data.join("aliases.txt"));
        #[cfg(not(windows))]
        let aliases_path = data.join("aliases.txt");
        let aliases =
            std::fs::read_to_string(aliases_path).unwrap_or_else(|_| DEFAULT_ALIAS_LINE.into());
        // Hero data is still indexed, but only the GUI resolves artwork roots
        // through client hints, which require initialized Windows preferences.
        let index = Index::load(&conn, &client_root, &[], &pictures, &aliases)?;
        let default_terms = data.join("terminology.json");
        let terms_path = match external {
            Some(path) => Some(path),
            None if default_terms
                .try_exists()
                .map_err(|error| format!("Cannot inspect terminology.json: {error}"))? =>
            {
                Some(default_terms.as_path())
            }
            None => None,
        };
        let mcp_terminology = terms_path
            .map(|path| terminology::load_external(path, |key| terminology_key_exists(&index, key)))
            .transpose()?;
        Ok(Self {
            conn,
            client_root,
            characters: None,
            character_files: std::cell::RefCell::new(None),
            pictures,
            index,
            calculator_entries: std::cell::OnceCell::new(),
            quest_graph: std::cell::OnceCell::new(),
            mcp_names: std::cell::OnceCell::new(),
            mcp_terminology,
        })
    }

    pub fn mcp_info(&self) -> Result<Value, String> {
        let version: String = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'catalog_version'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let terms = self
            .mcp_terminology
            .as_ref()
            .unwrap_or_else(|| terminology::glossary());
        Ok(
            json!({"catalog_version": version, "app_version": env!("CARGO_PKG_VERSION"),
            "read_only": true, "data_source": "bundled offline catalog",
            "live_market_prices": false,
            "terminology": {"source": if self.mcp_terminology.is_some() { "external" } else { "embedded" },
                "schema_version": terms.schema_version, "entity_count": terms.entities.len(),
                "alias_count": terms.entities.iter().map(|term| term.aliases.len() + term.exact_aliases.len() + term.subject_aliases.len()).sum::<usize>(),
                "reload": "restart_mcp"},
            "categories": ["item", "monster", "recipe", "quest", "hero", "npc", "set", "town", "facility", "building"],
            "filters": self.mcp_filter_options(),
            "relations": (["item", "monster", "recipe", "quest", "hero", "npc", "set", "town", "bay", "yard"].into_iter().map(|kind| (kind.to_string(), json!(relations::relation_names(kind)))).collect::<serde_json::Map<String,Value>>()),
            "counts": {"towns": self.index.towns.len(), "facilities": self.index.yards.len(), "buildings": self.index.works.len(), "items": self.index.items.len(), "monsters": self.index.monsters.len(),
                "recipes": self.index.recipes.len(), "quests": self.index.quests.len(),
                "heroes": self.index.heroes.len(), "npcs": self.index.npcs.len(), "sets": self.index.sets.len()}}),
        )
    }

    pub fn mcp_search(
        &self,
        category: &str,
        query: &str,
        lang: &str,
        offset: usize,
        limit: usize,
    ) -> Value {
        let query = query.trim();
        let folded = query.to_lowercase();
        // The legacy personal alias format contains item IDs, not typed keys.
        // Do not let an item alias accidentally resolve an NPC with the same ID.
        self.index
            .prepare_aliases(if matches!(category, "item" | "recipe") {
                &folded
            } else {
                ""
            });
        if category == "item" {
            // Exact user aliases are authoritative: SSA must not include every
            // item whose English name merely contains those three letters.
            let alias_codes = self.index.aliases.borrow().get(&folded).cloned();
            let rows: Vec<_> = self
                .index
                .items
                .iter()
                .filter(|row| {
                    if let Some(codes) = &alias_codes {
                        codes.contains(&row.code_str)
                    } else {
                        query.is_empty()
                            || row.code_str == query
                            || exact_name(&folded, [&row.en, &row.zh, &row.ko])
                    }
                })
                .collect();
            if query.is_empty() || alias_codes.is_some() || !rows.is_empty() {
                return page_map(rows, offset, limit, |row| item_hit(row, lang));
            }
        }
        let exact_keys = self.mcp_exact_keys(category, &folded);
        if exact_keys.is_empty() {
            if let Some(rows) = self.mcp_terminology_hits(category, query, lang) {
                return page(rows, offset, limit);
            }
        }
        // Bypass navigation categories and the GUI's 40-result cap so pagination is complete.
        let mut rows = match category {
            "item" => self
                .index
                .item_hits(query, &folded, lang, "", "", usize::MAX),
            "monster" => self.index.monster_hits(query, &folded, lang, &[], None),
            "recipe" => self.index.recipe_hits(query, &folded, lang, "", usize::MAX),
            "quest" => self
                .index
                .quest_hits(query, &folded, lang, "", "", usize::MAX),
            "hero" => self
                .index
                .hero_hits(query, &folded, lang, "", "", usize::MAX),
            "npc" => self.index.npc_hits(query, &folded, lang, usize::MAX),
            "set" => self.mcp_set_hits(query, &folded, lang),
            _ => Vec::new(),
        };
        if !exact_keys.is_empty() {
            rows.retain(|row| exact_keys.contains(&row.key));
        }
        page_map(rows, offset, limit, hit)
    }

    fn mcp_exact_keys(&self, category: &str, folded: &str) -> BTreeSet<String> {
        if folded.is_empty() {
            return BTreeSet::new();
        }
        match category {
            "monster" => self
                .index
                .monsters
                .iter()
                .filter(|row| {
                    row.code.eq_ignore_ascii_case(folded)
                        || exact_name(folded, [&row.en, &row.zh, &row.ko])
                })
                .map(|row| format!("monster:{}", row.code))
                .collect(),
            "recipe" => self
                .index
                .recipes
                .iter()
                .filter(|row| {
                    row.result_str == folded || exact_name(folded, [&row.en, &row.zh, &row.ko])
                })
                .map(|row| format!("recipe:{}", row.id))
                .collect(),
            "quest" => self
                .index
                .quests
                .iter()
                .filter(|row| {
                    row.code.eq_ignore_ascii_case(folded)
                        || exact_name(folded, [&row.title_en, &row.title_zh, &row.title_ko])
                })
                .map(|row| format!("quest:{}", row.id))
                .collect(),
            "hero" => self
                .index
                .heroes
                .iter()
                .filter(|row| {
                    row.code.eq_ignore_ascii_case(folded)
                        || exact_name(folded, [&row.en, &row.zh, &row.ko])
                })
                .map(|row| format!("hero:{}", row.code))
                .collect(),
            "npc" => self
                .index
                .npcs
                .iter()
                .filter(|row| {
                    row.code.to_string() == folded
                        || exact_name(folded, [&row.en, &row.zh, &row.ko])
                })
                .map(|row| format!("npc:{}", row.code))
                .collect(),
            "set" => self
                .index
                .sets
                .iter()
                .filter(|row| {
                    row.code.to_string() == folded
                        || exact_name(folded, [&row.en, &row.zh, &row.ko])
                })
                .map(|row| format!("set:{}", row.code))
                .collect(),
            _ => BTreeSet::new(),
        }
    }

    fn mcp_exists(&self, kind: &str, id: &str) -> bool {
        match kind {
            "item" => self.index.items.iter().any(|row| row.code_str == id),
            "monster" => self.index.monsters.iter().any(|row| row.code == id),
            "recipe" => self
                .index
                .recipes
                .iter()
                .any(|row| Some(row.id) == id.parse().ok()),
            "quest" => self
                .index
                .quests
                .iter()
                .any(|row| Some(row.id) == id.parse().ok()),
            "hero" => self.index.heroes.iter().any(|row| row.code == id),
            "npc" => self
                .index
                .npcs
                .iter()
                .any(|row| Some(row.code) == id.parse().ok()),
            "set" => self
                .index
                .sets
                .iter()
                .any(|row| Some(row.code) == id.parse().ok()),
            "town" => self.index.towns.iter().any(|row| row.code == id),
            "bay" => self.index.yards.contains_key(id),
            "yard" => self.index.works.iter().any(|row| row.code == id),
            _ => false,
        }
    }

    #[cfg(test)]
    pub fn mcp_entity(&self, key: &str, lang: &str) -> Result<Value, String> {
        self.mcp_entity_sections(key, lang, &[])
    }

    pub fn mcp_drops(
        &self,
        code: i64,
        lang: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value, String> {
        if !self.mcp_exists("item", &code.to_string()) {
            return Err("Item not found".into());
        }
        let sources = item_drop_sources(&self.conn, code)?;
        let rows = sources
            .into_iter()
            .filter_map(|(monster, slots)| {
                self.index
                    .monsters
                    .binary_search_by(|row| row.code.cmp(&monster))
                    .ok()
                    .map(|index| (&self.index.monsters[index], slots))
            })
            .collect();
        let mut result = page_map(rows, offset, limit, |(monster, slots)| {
            let mut row = drop_details(&slots);
            row["key"] = json!(format!("monster:{}", monster.code));
            row["name"] = json!(lead3(lang, &monster.en, &monster.zh, &monster.ko));
            row["alternate_name"] = json!(trail3(lang, &monster.en, &monster.zh, &monster.ko));
            row["locations"] = json!(self.place_names(&monster.code, lang));
            row
        });
        result["item_code"] = json!(code);
        result["note"] = json!("Monster drops only. drop_rate is a probability from 0 to 1; drop_rate_percent is a percentage; drop_rate_per_100000 is the source integer. Unknown rates are null. Each drops entry preserves its slot and solo-only condition. Top-level rates are null when multiple slots exist; no combined probability is inferred.");
        Ok(result)
    }

    pub fn mcp_quests(
        &self,
        id: i64,
        direction: &str,
        lang: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value, String> {
        if !self.mcp_exists("quest", &id.to_string()) {
            return Err("Quest not found".into());
        }
        let (rows, note) = if direction == "chain" {
            self.quest_dependency_rows(id, lang)
        } else {
            let (parents, children) = self.quest_immediate_dependencies(id);
            let ids = if direction == "prerequisites" {
                parents
            } else {
                children
            };
            (
                ids.into_iter()
                    .filter_map(|id| self.quest_link_hit(id, lang, ""))
                    .collect(),
                "Immediate dependencies only; use direction=chain for the connected quest chain."
                    .into(),
            )
        };
        let mut result = page_map(rows, offset, limit, hit);
        result["quest_id"] = json!(id);
        result["direction"] = json!(direction);
        result["note"] = json!(note);
        Ok(result)
    }

    pub fn mcp_recipes(
        &self,
        book: &CalcCatalog,
        code: i64,
        lang: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value, String> {
        if !self.mcp_exists("item", &code.to_string()) {
            return Err("Item not found".into());
        }
        let code_str = code.to_string();
        let rows = self
            .index
            .recipes
            .iter()
            .filter(|row| row.result_str == code_str)
            .collect();
        let name = self.item_name(code, lang);
        let mut result = page_map(rows, offset, limit, |row| {
            let document = self.calculator_document(book, row.id, lang);
            let mut result = json!({"key": format!("recipe:{}", row.id), "recipe_id": row.id,
                "source": row.source, "name": name,
                "calculable": document.is_ok(), "calculation_unavailable_reason": document.as_ref().err(),
                "recipe": document.ok()});
            self.mcp_attach_facility(&mut result, row.id, lang);
            result
        });
        result["item_code"] = json!(code);
        Ok(result)
    }

    pub fn mcp_calculate(
        &self,
        book: &CalcCatalog,
        recipe_id: i64,
        quantity: i64,
        prices: &[(i64, i64, i64)],
        lang: &str,
    ) -> Result<Value, String> {
        let mut document = self.calculator_document(book, recipe_id, lang)?;
        document.quantity = quantity;
        let mut seen = BTreeSet::new();
        for &(code, enhance, price) in prices {
            if price < 0 || !seen.insert((code, enhance)) {
                return Err(
                    "Unit prices must be non-negative and each material may appear only once"
                        .into(),
                );
            }
            let material = document
                .materials
                .iter_mut()
                .find(|m| m.code == code && m.enhance == enhance)
                .ok_or_else(|| {
                    format!("Item {code} +{enhance} is not a material of this recipe")
                })?;
            material.unit_price = Some(price);
        }
        let calculation = document.calculate()?;
        let materials: Vec<_> = calculation.materials.iter().map(|row| json!({
            "item_code": row.key.code, "enhance": row.key.enhance, "name": row.title,
            "quantity": row.quantity, "unit_price": row.unit_price, "line_total": row.line_total,
        })).collect();
        let mut result = json!({"recipe_id": recipe_id, "item_code": document.item_code, "name": document.title,
            "requested_quantity": quantity, "batches": calculation.batches, "output_units": calculation.output_units,
            "crafting_fee": calculation.fee, "known_material_cost": calculation.material_total,
            "known_total": calculation.known_total, "total": calculation.total, "complete": calculation.total.is_some(),
            "materials": materials, "success_rate": document.success, "learn_fee": document.learn_fee,
            "note": "Unit prices are supplied by the user, not live market prices. Missing prices remain unknown, not zero. Totals exclude learning fees and failed attempts; quantities cover direct recipe materials only."});
        self.mcp_attach_facility(&mut result, recipe_id, lang);
        Ok(result)
    }
}

pub fn database_path() -> Result<std::path::PathBuf, String> {
    match std::env::var_os("GERSANG_DB") {
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            if !path.is_file() {
                return Err("GERSANG_DB must point to an existing gersang.db file".into());
            }
            path.canonicalize().map_err(|error| error.to_string())
        }
        None => super::paths::find_db()?
            .canonicalize()
            .map_err(|error| error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_formats_only_the_requested_window() {
        let mut formatted = Vec::new();
        let result = page_map((0..10).collect(), 3, 2, |row| {
            formatted.push(row);
            json!(row)
        });
        assert_eq!(formatted, vec![3, 4]);
        assert_eq!(
            result,
            json!({"results": [3, 4], "total": 10, "offset": 3, "next_offset": 5})
        );

        let result = page_map(vec![1, 2], usize::MAX, 1, |_| {
            panic!("off-page row was formatted")
        });
        assert_eq!(result["total"], 2);
        assert_eq!(result["results"], json!([]));
        assert!(result["next_offset"].is_null());
    }

    #[test]
    fn mcp_search_prioritizes_exact_aliases_and_canonical_names() {
        let catalog = Catalog::open_for_mcp().expect("headless catalog");
        catalog.index.aliases.borrow_mut().clear();
        catalog.index.aliases.borrow_mut().insert(
            "ssa".into(),
            ["6213", "6214", "6215", "6216", "6943"]
                .map(String::from)
                .into(),
        );
        // A longer alias shares the prefix but must not widen an exact match.
        catalog
            .index
            .aliases
            .borrow_mut()
            .insert("ssaa".into(), ["143".to_string()].into());
        let alias = catalog.mcp_search("item", " SSA ", "en", 0, 100);
        assert_eq!(alias["total"], 5);
        assert_eq!(
            alias["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["key"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "item:6213",
                "item:6214",
                "item:6215",
                "item:6216",
                "item:6943"
            ]
        );
        let next = catalog.mcp_search("item", "SSA", "en", 2, 2);
        assert_eq!(next["total"], 5);
        assert_eq!(next["next_offset"], 4);
        assert_eq!(
            next["results"],
            json!(alias["results"].as_array().unwrap()[2..4])
        );

        let hero = catalog.mcp_search("hero", "持國天王", "zh", 0, 100);
        assert_eq!(hero["total"], 1);
        assert_eq!(hero["results"][0]["key"], "hero:T49");
        assert_eq!(hero["results"][0]["name"], "持國天王");
        let english = catalog
            .index
            .heroes
            .iter()
            .find(|row| row.code == "T49")
            .unwrap()
            .en
            .to_uppercase();
        assert_eq!(
            catalog.mcp_search("hero", &english, "ko", 0, 100)["results"][0]["key"],
            "hero:T49"
        );
    }

    #[test]
    fn mcp_item_pages_and_drop_names_preserve_existing_fields() {
        let catalog = Catalog::open_for_mcp().expect("headless catalog");
        for lang in ["en", "zh", "ko", "ko-en"] {
            let expected = catalog
                .index
                .item_hits("", "", lang, "", "", 3)
                .into_iter()
                .map(hit)
                .collect::<Vec<_>>();
            let actual = catalog.mcp_search("item", "", lang, 1, 2);
            assert_eq!(actual["total"], catalog.index.items.len());
            assert_eq!(actual["results"], json!(&expected[1..]));

            let drops = catalog.mcp_drops(6261, lang, 2, 2).expect("item drops");
            assert_eq!(drops["results"].as_array().unwrap().len(), 2);
            for row in drops["results"].as_array().unwrap() {
                let code = row["key"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("monster:")
                    .unwrap();
                let sheet = catalog.monster_sheet(code, lang).expect("monster sheet");
                assert_eq!(row["name"], sheet.0);
                assert_eq!(row["alternate_name"], sheet.1);
                assert_eq!(row["locations"], json!(catalog.place_names(code, lang)));
            }
        }
    }

    #[test]
    fn headless_character_queries_preserve_data_without_gui_assets() {
        let catalog = Catalog::open_for_mcp().expect("headless catalog");
        assert!(!catalog.index.heroes.is_empty());
        // The real catalog has bundled artwork. MCP deliberately indexes the
        // same character data without resolving GUI asset roots or client hints.
        assert!(catalog.index.heroes.iter().all(|hero| hero.icon.is_none()));
        for key in ["hero:U68", "hero:mount:3", "hero:H30"] {
            let entity = catalog.mcp_entity(key, "en").expect("character data");
            assert!(
                entity["name"].as_str().is_some_and(|name| !name.is_empty()),
                "{key}"
            );
            assert!(
                entity["facts"]
                    .as_array()
                    .is_some_and(|facts| !facts.is_empty()),
                "{key}"
            );
        }
        let mount = catalog.mcp_entity("hero:mount:3", "en").unwrap();
        assert!(mount["related"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["key"] == "hero:mount:1"));
        let spirit = catalog.mcp_entity("hero:H30", "en").unwrap();
        assert!(spirit["facts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["label"] == "Skill effect"));
        assert!(spirit["facts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["label"] == "HP"));
        assert!(catalog.characters.is_none());
        assert!(catalog.character_files.borrow().is_none());
    }

    #[test]
    fn mcp_drop_slots_preserve_conditions_and_unknown_rates() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE monster_drops (monster_code TEXT, item_code INTEGER, slot INTEGER, drop_rate_per_100000 INTEGER);
             INSERT INTO monster_drops VALUES ('M1', 143, 2, 5500), ('M1', 143, 9, 12000),
                 ('M2', 143, 1, NULL), ('M3', 143, 10, 0), ('M1', 144, 3, 500);",
        )
        .unwrap();
        let sources = item_drop_sources(&conn, 143).unwrap();
        assert_eq!(sources.len(), 3);
        let mixed = drop_details(&sources["M1"]);
        assert!(mixed["drop_rate"].is_null());
        assert!(mixed["drop_rate_percent"].is_null());
        assert_eq!(mixed["solo_only"], false);
        assert_eq!(mixed["drops"].as_array().unwrap().len(), 2);
        assert_eq!(mixed["drops"][0]["slot"], 2);
        assert_eq!(mixed["drops"][0]["solo_only"], false);
        assert_eq!(mixed["drops"][0]["drop_rate"], 0.055);
        assert_eq!(mixed["drops"][1]["slot"], 9);
        assert_eq!(mixed["drops"][1]["solo_only"], true);
        assert_eq!(mixed["drops"][1]["drop_rate_percent"], 12.0);

        let unknown = drop_details(&sources["M2"]);
        assert!(unknown["drop_rate"].is_null());
        assert!(unknown["drops"][0]["drop_rate_per_100000"].is_null());
        let zero = drop_details(&sources["M3"]);
        assert_eq!(zero["drop_rate"], 0.0);
        assert_eq!(zero["drop_rate_percent"], 0.0);
        assert_eq!(zero["drop_rate_per_100000"], 0);
        assert_eq!(zero["solo_only"], true);
    }

    #[test]
    fn mcp_drop_slots_support_legacy_catalog_without_rates() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE monster_drops (monster_code TEXT, item_code INTEGER, slot INTEGER);
             INSERT INTO monster_drops VALUES ('M24', 143, 1);",
        )
        .unwrap();
        let sources = item_drop_sources(&conn, 143).unwrap();
        let deer = drop_details(&sources["M24"]);
        assert!(deer["drop_rate"].is_null());
        assert_eq!(deer["drops"][0]["slot"], 1);
        assert!(deer["drops"][0]["drop_rate_percent"].is_null());
        assert!(deer["drops"][0]["drop_rate_per_100000"].is_null());
    }
}
