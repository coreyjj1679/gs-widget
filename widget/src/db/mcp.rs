//! Read-only catalog access for MCP. No GUI, preference migration or saved calculations.
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde_json::{json, Value};

use super::{alias::DEFAULT_ALIAS_LINE, rows::Index, CalcCatalog, Catalog, Hit};

pub fn page(rows: Vec<Value>, offset: usize, limit: usize) -> Value {
    let total = rows.len();
    let results: Vec<_> = rows.into_iter().skip(offset).take(limit).collect();
    let next = offset.saturating_add(results.len());
    json!({"results": results, "total": total, "offset": offset,
        "next_offset": if next < total { Some(next) } else { None }})
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

impl Catalog {
    pub fn open_for_mcp() -> Result<Self, String> {
        Self::open_mcp_path(&database_path()?)
    }

    fn open_mcp_path(path: &Path) -> Result<Self, String> {
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
        let index = Index::load(&conn, &client_root, &pictures, &aliases)?;
        Ok(Self {
            conn,
            client_root,
            characters: None,
            character_files: std::cell::RefCell::new(None),
            pictures,
            index,
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
        Ok(
            json!({"catalog_version": version, "app_version": env!("CARGO_PKG_VERSION"),
            "read_only": true, "data_source": "bundled offline catalog",
            "live_market_prices": false,
            "counts": {"items": self.index.items.len(), "monsters": self.index.monsters.len(),
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
        self.index.prepare_aliases(&folded);
        // Bypass navigation categories and the GUI's 40-result cap so pagination is complete.
        let rows = match category {
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
            "set" => self.index.set_hits(query, &folded, lang, ""),
            _ => Vec::new(),
        };
        page(rows.into_iter().map(hit).collect(), offset, limit)
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
            _ => false,
        }
    }

    pub fn mcp_entity(&self, key: &str, lang: &str) -> Result<Value, String> {
        let (kind, id) = key
            .split_once(':')
            .ok_or("Use a key returned by search_catalog, such as item:6213")?;
        if !self.mcp_exists(kind, id) {
            return Err(format!(
                "Catalog record not found: {key}. Search for its name first."
            ));
        }
        let (text, related, _) = self.open_key(key, lang);
        let truncated = related.len() > 50;
        let mut result = json!({"key": key, "kind": kind, "text": text,
            "related": related.into_iter().take(50).map(hit).collect::<Vec<_>>(),
            "related_truncated": truncated,
            "related_scope": "Summary only. Use find_item_drops, get_recipes or get_quest_dependencies for complete paginated results."});
        if kind == "monster" {
            let stats = self.conn.query_row(
                "SELECT hp, mp, atk_min, atk_max, hit_resist, magic_resist, exp, element, element_gts, is_boss FROM monsters WHERE code = ?1",
                [id], |row| Ok(json!({"hp": row.get::<_, i64>(0)?, "mp": row.get::<_, i64>(1)?,
                    "attack_min": row.get::<_, i64>(2)?, "attack_max": row.get::<_, i64>(3)?,
                    "hit_resist": row.get::<_, i64>(4)?, "magic_resist": row.get::<_, i64>(5)?,
                    "experience": row.get::<_, i64>(6)?, "element": row.get::<_, i64>(7)?,
                    "element_value": row.get::<_, i64>(8)?, "is_boss": row.get::<_, i64>(9)? != 0})),
            ).map_err(|error| error.to_string())?;
            result["stats"] = stats;
            result["locations"] = json!(self.place_names(id, lang));
        }
        let sheet = match kind {
            "item" => self
                .item_sheet(id.parse().unwrap_or(0), lang)
                .map(|s| (s.0, s.1, s.3)),
            "hero" => self.hero_sheet(id, lang).map(|s| (s.0, s.1, s.3)),
            "npc" => self
                .npc_sheet(id.parse().unwrap_or(0), lang)
                .map(|s| (s.0, s.1, s.3)),
            "monster" => self.monster_sheet(id, lang).map(|s| (s.0, s.1, s.3)),
            "recipe" => self
                .recipe_sheet(id.parse().unwrap_or(0), lang)
                .map(|s| (s.0, s.1, s.3)),
            _ => None,
        };
        if let Some((name, alternate, rows)) = sheet {
            result["name"] = json!(name);
            result["alternate_name"] = json!(alternate);
            result["facts"] = json!(facts(rows));
        }
        Ok(result)
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
        let mut statement = self.conn.prepare(
            "SELECT monster_code, MIN(slot) FROM monster_drops WHERE item_code = ?1 GROUP BY monster_code ORDER BY monster_code",
        ).map_err(|error| error.to_string())?;
        let sources: Vec<(String, i64)> = statement
            .query_map([code], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| error.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        let rows = sources.into_iter().filter_map(|(monster, slot)| {
            self.monster_sheet(&monster, lang).map(|sheet| json!({
                "key": format!("monster:{monster}"), "name": sheet.0, "alternate_name": sheet.1,
                "solo_only": slot >= 9, "locations": self.place_names(&monster, lang), "drop_rate": null,
            }))
        }).collect();
        let mut result = page(rows, offset, limit);
        result["item_code"] = json!(code);
        result["note"] =
            json!("Monster drops only. Drop probabilities are not recorded in this catalog.");
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
        let mut result = page(rows.into_iter().map(hit).collect(), offset, limit);
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
        let rows = self.index.recipes.iter().filter(|row| row.result_str == code.to_string()).map(|row| {
            let document = self.calculator_document(book, row.id, lang);
            json!({"key": format!("recipe:{}", row.id), "recipe_id": row.id,
                "source": row.source, "name": self.item_name(code, lang),
                "calculable": document.is_ok(), "calculation_unavailable_reason": document.as_ref().err(),
                "recipe": document.ok()})
        }).collect();
        let mut result = page(rows, offset, limit);
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
        Ok(
            json!({"recipe_id": recipe_id, "item_code": document.item_code, "name": document.title,
            "requested_quantity": quantity, "batches": calculation.batches, "output_units": calculation.output_units,
            "crafting_fee": calculation.fee, "known_material_cost": calculation.material_total,
            "known_total": calculation.known_total, "total": calculation.total, "complete": calculation.total.is_some(),
            "materials": materials, "success_rate": document.success, "learn_fee": document.learn_fee,
            "note": "Unit prices are supplied by the user, not live market prices. Missing prices remain unknown, not zero. Totals exclude learning fees and failed attempts; quantities cover direct recipe materials only."}),
        )
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
