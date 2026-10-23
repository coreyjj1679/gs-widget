//! Catalog filters use source fields, independently of translated display labels.
use std::collections::{BTreeMap, BTreeSet, HashSet};

use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{hit, item_hit, page, page_map, Catalog, Hit};
use crate::db::rows::{TownRow, WorkBuilding, WorkFacility};
use crate::db::text::{caption, lead3, realm_pair, trail3};

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SearchFilters {
    /// Source region code for town, monster or NPC. See catalog_info filter options.
    pub region: Option<String>,
    /// Exact characters.nation code for heroes: 0, 1, 10, 100, 1000 or 10000.
    pub nation: Option<i64>,
    /// Monster element: 0 none, 1 fire, 2 water, 3 wind, 4 thunder, 5 earth.
    pub element: Option<i64>,
    pub is_boss: Option<bool>,
    /// Inclusive lower bound on recipes.level_min, not an item's equip level.
    pub min_level: Option<i64>,
    /// Inclusive upper bound on recipes.level_min. Unrecorded levels are excluded.
    pub max_level: Option<i64>,
    /// Exact items.category code, as listed by catalog_info.
    pub item_category: Option<String>,
    /// Exact items.subtype code, as listed by catalog_info.
    pub item_subtype: Option<String>,
}

impl SearchFilters {
    fn is_empty(&self) -> bool {
        self.region.is_none()
            && self.nation.is_none()
            && self.element.is_none()
            && self.is_boss.is_none()
            && self.min_level.is_none()
            && self.max_level.is_none()
            && self.item_category.is_none()
            && self.item_subtype.is_none()
    }
}

const TOWN_REGIONS: &[&str] = &["joseon", "japan", "china", "taiwan", "india", "mongolia"];
const MONSTER_REGIONS: &[(&str, i64)] = &[
    ("joseon", 1),
    ("japan", 2),
    ("china", 3),
    ("taiwan", 4),
    ("india", 5),
    ("special", 6),
];
const NPC_REGIONS: &[(&str, &str, &str)] = &[
    ("joseon", "K", "Joseon"),
    ("japan", "J", "Japan"),
    ("china", "C", "China"),
    ("taiwan", "T", "Taiwan"),
    ("india", "I", "India"),
    ("all", "A", "All"),
];
const HERO_NATIONS: &[i64] = &[0, 1, 10, 100, 1000, 10000];

impl Catalog {
    pub(super) fn mcp_set_hits(&self, query: &str, folded: &str, lang: &str) -> Vec<Hit> {
        use crate::db::item::{set_kind_label, set_kind_rank, set_piece_label, set_title_lang};
        let mut rows: Vec<_> = self
            .index
            .sets
            .iter()
            .filter(|set| {
                query.is_empty()
                    || self
                        .index
                        .name_hit(query, folded, &set.code.to_string(), &set.names)
            })
            .collect();
        rows.sort_by(|left, right| {
            set_kind_rank(left.kind)
                .cmp(&set_kind_rank(right.kind))
                .then(left.zh.cmp(&right.zh))
                .then(left.en.cmp(&right.en))
                .then(left.code.cmp(&right.code))
        });
        rows.into_iter()
            .map(|set| Hit {
                key: format!("set:{}", set.code),
                title: set_title_lang(lang, &set.ko, &set.en, &set.zh),
                subtitle: caption(
                    &trail3(lang, &set.en, &set.zh, &set.ko),
                    set_piece_label(set.pieces, lang),
                ),
                aside: if set.kind == 0 {
                    String::new()
                } else {
                    set_kind_label(set.kind, lang)
                },
                chip: String::new(),
                solo: false,
                icon: None,
                badge: None,
            })
            .collect()
    }

    pub fn mcp_search_filtered(
        &self,
        category: &str,
        query: &str,
        lang: &str,
        offset: usize,
        limit: usize,
        filters: &SearchFilters,
    ) -> Result<Value, String> {
        if !(1..=50).contains(&limit) {
            return Err("Search limit must be between 1 and 50".into());
        }
        self.validate_search_filters(category, filters)?;
        if matches!(category, "town" | "building" | "facility") {
            return Ok(self.mcp_location_search(category, query, lang, offset, limit, filters));
        }
        if filters.is_empty() {
            return Ok(self.mcp_search(category, query, lang, offset, limit));
        }
        let allowed = self.filtered_search_keys(category, filters)?;
        if allowed.is_empty() {
            return Ok(page(Vec::new(), offset, limit));
        }
        // Browsing equipment need not format every item merely to discard most
        // of them. The index order is the same as the original unfiltered tool.
        if category == "item" && query.trim().is_empty() {
            let rows = self
                .index
                .items
                .iter()
                .filter(|row| allowed.contains(&format!("item:{}", row.code_str)))
                .collect();
            return Ok(page_map(rows, offset, limit, |row| item_hit(row, lang)));
        }
        // Keep the original canonical/alias/fuzzy precedence. A filter narrows
        // that ranked result set; it must not replace an exact match with a
        // different, less-specific name just because the exact row was excluded.
        let mut ranked = self.mcp_search(category, query, lang, 0, usize::MAX);
        let Value::Array(mut rows) = ranked["results"].take() else {
            return Err("Catalog search did not return a result list".into());
        };
        rows.retain(|row| row["key"].as_str().is_some_and(|key| allowed.contains(key)));
        Ok(page(rows, offset, limit))
    }

    fn validate_search_filters(
        &self,
        category: &str,
        filters: &SearchFilters,
    ) -> Result<(), String> {
        if !matches!(
            category,
            "item"
                | "monster"
                | "recipe"
                | "quest"
                | "hero"
                | "npc"
                | "set"
                | "town"
                | "building"
                | "facility"
        ) {
            return Err(format!("Unknown search category: {category}"));
        }
        let wrong = |field: &str, supported: &str| {
            format!("Filter {field} is not supported for {category}; use it with {supported}")
        };
        if let Some(region) = &filters.region {
            let valid = match category {
                "town" => TOWN_REGIONS.contains(&region.as_str()),
                "monster" => MONSTER_REGIONS.iter().any(|(name, _)| name == region),
                "npc" => NPC_REGIONS.iter().any(|(name, _, _)| name == region),
                _ => return Err(wrong("region", "town, monster or npc")),
            };
            if !valid {
                return Err(format!(
                    "Unknown {category} region {region:?}; see catalog_info filter_options"
                ));
            }
        }
        if let Some(nation) = filters.nation {
            if category != "hero" {
                return Err(wrong("nation", "hero"));
            }
            if !HERO_NATIONS.contains(&nation) {
                return Err(format!(
                    "Unknown hero nation {nation}; use 0, 1, 10, 100, 1000 or 10000"
                ));
            }
        }
        if let Some(element) = filters.element {
            if category != "monster" {
                return Err(wrong("element", "monster"));
            }
            if !(0..=5).contains(&element) {
                return Err("Unknown monster element; use 0 none, 1 fire, 2 water, 3 wind, 4 thunder or 5 earth".into());
            }
        }
        if filters.is_boss.is_some() && category != "monster" {
            return Err(wrong("is_boss", "monster"));
        }
        if filters.min_level.is_some() || filters.max_level.is_some() {
            if category == "item" {
                return Err("Item level requirements are not recorded in this catalog. min_level/max_level filter recipe crafting levels only; use item_category/item_subtype for equipment.".into());
            }
            if category != "recipe" {
                return Err(wrong("min_level/max_level", "recipe"));
            }
            if filters.min_level.is_some_and(|level| level < 0)
                || filters.max_level.is_some_and(|level| level < 0)
            {
                return Err("Recipe level bounds must be non-negative".into());
            }
            if matches!((filters.min_level, filters.max_level), (Some(min), Some(max)) if min > max)
            {
                return Err("min_level must not exceed max_level".into());
            }
        }
        if let Some(value) = &filters.item_category {
            if category != "item" {
                return Err(wrong("item_category", "item"));
            }
            if !self.index.items.iter().any(|row| &row.category == value) {
                return Err(format!(
                    "Unknown item_category {value:?}; see catalog_info filter_options"
                ));
            }
        }
        if let Some(value) = &filters.item_subtype {
            if category != "item" {
                return Err(wrong("item_subtype", "item"));
            }
            if !self.index.items.iter().any(|row| {
                &row.subtype == value
                    && filters
                        .item_category
                        .as_ref()
                        .is_none_or(|category| &row.category == category)
            }) {
                return Err(format!("Unknown item_subtype {value:?} for the selected category; see catalog_info filter_options"));
            }
        }
        Ok(())
    }

    fn filtered_search_keys(
        &self,
        category: &str,
        filters: &SearchFilters,
    ) -> Result<HashSet<String>, String> {
        match category {
            "item" => Ok(self
                .index
                .items
                .iter()
                .filter(|row| {
                    filters
                        .item_category
                        .as_ref()
                        .is_none_or(|value| &row.category == value)
                        && filters
                            .item_subtype
                            .as_ref()
                            .is_none_or(|value| &row.subtype == value)
                })
                .map(|row| format!("item:{}", row.code_str))
                .collect()),
            "hero" => Ok(self
                .index
                .heroes
                .iter()
                .filter(|row| filters.nation.is_none_or(|nation| row.nation == nation))
                .map(|row| format!("hero:{}", row.code))
                .collect()),
            "recipe" => Ok(self
                .index
                .recipes
                .iter()
                .filter(|row| {
                    row.level_min.is_some_and(|level| {
                        filters.min_level.is_none_or(|min| level >= min)
                            && filters.max_level.is_none_or(|max| level <= max)
                    })
                })
                .map(|row| format!("recipe:{}", row.id))
                .collect()),
            "monster" => {
                let nation = filters.region.as_ref().and_then(|region| {
                    MONSTER_REGIONS
                        .iter()
                        .find(|(name, _)| name == region)
                        .map(|(_, nation)| *nation)
                });
                let mut statement = self
                    .conn
                    .prepare(
                        "SELECT code FROM monsters WHERE (?1 IS NULL OR nation = ?1)
                     AND (?2 IS NULL OR element = ?2) AND (?3 IS NULL OR is_boss = ?3)",
                    )
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map(params![nation, filters.element, filters.is_boss], |row| {
                        row.get::<_, String>(0)
                            .map(|code| format!("monster:{code}"))
                    })
                    .map_err(|error| error.to_string())?;
                rows.collect::<Result<_, _>>()
                    .map_err(|error| error.to_string())
            }
            "npc" => {
                let nation = filters.region.as_ref().and_then(|region| {
                    NPC_REGIONS
                        .iter()
                        .find(|(name, _, _)| name == region)
                        .map(|(_, nation, _)| *nation)
                });
                let mut statement = self
                    .conn
                    .prepare("SELECT code FROM npcs WHERE ?1 IS NULL OR nation = ?1")
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map([nation], |row| {
                        row.get::<_, i64>(0).map(|code| format!("npc:{code}"))
                    })
                    .map_err(|error| error.to_string())?;
                rows.collect::<Result<_, _>>()
                    .map_err(|error| error.to_string())
            }
            _ => Err(format!("No filters are available for {category}")),
        }
    }

    fn mcp_location_search(
        &self,
        category: &str,
        query: &str,
        lang: &str,
        offset: usize,
        limit: usize,
        filters: &SearchFilters,
    ) -> Value {
        let query = query.trim();
        let folded = query.to_lowercase();
        let rows: Vec<LocationRow<'_>> = match category {
            "town" => self
                .index
                .towns
                .iter()
                .filter(|town| {
                    filters
                        .region
                        .as_ref()
                        .is_none_or(|region| &town.region == region)
                })
                .map(LocationRow::Town)
                .collect(),
            "building" => self.index.works.iter().map(LocationRow::Building).collect(),
            "facility" => self
                .index
                .yards
                .values()
                .map(LocationRow::Facility)
                .collect(),
            _ => Vec::new(),
        };
        let mut ranked =
            rows.into_iter()
                .filter_map(|row| {
                    let exact = !query.is_empty()
                        && (row.code().eq_ignore_ascii_case(query)
                            || row.key().eq_ignore_ascii_case(query)
                            || row.names().iter().any(|name| {
                                !name.is_empty() && name.trim().to_lowercase() == folded
                            }));
                    if exact || query.is_empty() || row.haystack().contains(&folded) {
                        Some((!exact, row))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
        if ranked.iter().any(|(partial, _)| !partial) {
            ranked.retain(|(partial, _)| !partial);
        }
        ranked.sort_by(|(_, a), (_, b)| a.code().cmp(b.code()));
        page_map(ranked, offset, limit, |(_, row)| match row {
            LocationRow::Town(town) => {
                let (region_en, region_zh) = realm_pair(&town.region);
                hit(Hit {
                    key: format!("town:{}", town.code),
                    title: lead3(lang, &town.en, &town.zh, &town.ko),
                    subtitle: caption(
                        &trail3(lang, &town.en, &town.zh, &town.ko),
                        format!("{}, {}", town.x, town.y),
                    ),
                    aside: lead3(
                        lang,
                        region_en,
                        region_zh,
                        &crate::db::ui_text("ko", region_zh, region_en),
                    ),
                    chip: String::new(),
                    solo: false,
                    icon: None,
                    badge: None,
                })
            }
            LocationRow::Building(building) => hit(self.index.work_building_hit(building, lang)),
            LocationRow::Facility(facility) => hit(self.index.work_facility_hit(facility, lang)),
        })
    }

    pub fn mcp_filter_options(&self) -> Value {
        let mut categories = BTreeMap::<&str, usize>::new();
        let mut subtypes = BTreeMap::<&str, (usize, BTreeSet<&str>)>::new();
        for item in &self.index.items {
            *categories.entry(&item.category).or_default() += 1;
            let subtype = subtypes.entry(&item.subtype).or_default();
            subtype.0 += 1;
            subtype.1.insert(&item.category);
        }
        let item_categories: Vec<_> = categories
            .into_iter()
            .map(|(value, count)| {
                let (en, zh) = crate::db::item::item_category_pair(value);
                json!({"value": value, "name": {"en": en, "zh": zh, "ko": crate::db::ui_text("ko", zh, en)}, "count": count})
            })
            .collect();
        let item_subtypes: Vec<_> = subtypes.into_iter().map(|(value, (count, categories))| {
            json!({"value": value, "categories": categories, "count": count})
        }).collect();
        let town_regions: Vec<_> = TOWN_REGIONS
            .iter()
            .map(|region| {
                json!({"value": region,
            "count": self.index.towns.iter().filter(|row| row.region == *region).count()})
            })
            .collect();
        let monster_regions: Vec<_> = MONSTER_REGIONS.iter().map(|(region, nation)| json!({"value": region,
            "source_nation": nation, "count": self.index.monsters.iter().filter(|row| row.nation == *nation).count()})).collect();
        let npc_regions: Vec<_> = NPC_REGIONS.iter().map(|(region, nation, name)| json!({"value": region,
            "source_nation": nation, "count": self.index.npcs.iter().filter(|row| row.nation_en == *name).count()})).collect();
        let hero_nations: Vec<_> = HERO_NATIONS.iter().map(|nation| {
            let (en, zh) = crate::db::hero::hero_land_pair(*nation);
            json!({"value": nation, "name": {"en": en, "zh": zh, "ko": crate::db::ui_text("ko", zh, en)},
                "count": self.index.heroes.iter().filter(|row| row.nation == *nation).count()})
        }).collect();
        let elements: Vec<_> = [
            (0, "none"),
            (1, "fire"),
            (2, "water"),
            (3, "wind"),
            (4, "thunder"),
            (5, "earth"),
        ]
        .into_iter()
        .map(|(value, name)| json!({"value": value, "name": name}))
        .collect();
        let levels: Vec<_> = self
            .index
            .recipes
            .iter()
            .filter_map(|row| row.level_min)
            .collect();
        json!({
            "by_category": {"item": ["item_category", "item_subtype"], "monster": ["region", "element", "is_boss"],
                "hero": ["nation"], "npc": ["region"], "recipe": ["min_level", "max_level"], "town": ["region"],
                "quest": [], "set": [], "building": [], "facility": []},
            "regions": {"town": town_regions, "monster": monster_regions, "npc": npc_regions},
            "region_note": "NPC region=all selects the catalog's A (All) records; omit region to search every NPC nation.",
            "hero_nations": hero_nations, "monster_elements": elements,
            "item_categories": item_categories, "item_subtypes": item_subtypes,
            "recipe_level": {"source": "recipes.level_min", "minimum": levels.iter().min(), "maximum": levels.iter().max(),
                "unknown_count": self.index.recipes.len() - levels.len(), "bounds": "inclusive; unrecorded levels are excluded"},
            "item_level": {"supported": false, "reason": "Item level requirements are not recorded in this catalog"}
        })
    }
}

enum LocationRow<'a> {
    Town(&'a TownRow),
    Building(&'a WorkBuilding),
    Facility(&'a WorkFacility),
}

impl LocationRow<'_> {
    fn code(&self) -> &str {
        match self {
            Self::Town(row) => &row.code,
            Self::Building(row) => &row.code,
            Self::Facility(row) => &row.code,
        }
    }
    fn key(&self) -> String {
        let prefix = match self {
            Self::Town(_) => "town",
            Self::Building(_) => "yard",
            Self::Facility(_) => "bay",
        };
        format!("{prefix}:{}", self.code())
    }
    fn names(&self) -> [&str; 3] {
        match self {
            Self::Town(row) => [&row.en, &row.zh, &row.ko],
            Self::Building(row) => [&row.en, &row.zh, &row.ko],
            Self::Facility(row) => [&row.en, &row.zh, &row.ko],
        }
    }
    fn haystack(&self) -> &str {
        match self {
            Self::Town(row) => &row.names,
            Self::Building(row) => &row.names,
            Self::Facility(row) => &row.names,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(result: &Value) -> Vec<&str> {
        result["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["key"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn new_categories_have_stable_complete_pages_and_trilingual_names() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for (category, prefix, count) in [
            ("town", "town:", catalog.index.towns.len()),
            ("building", "yard:", catalog.index.works.len()),
            ("facility", "bay:", catalog.index.yards.len()),
        ] {
            let mut all = Vec::new();
            let mut offset = 0;
            loop {
                let result = catalog
                    .mcp_search_filtered(
                        category,
                        "",
                        "ko-en",
                        offset,
                        17,
                        &SearchFilters::default(),
                    )
                    .unwrap();
                assert_eq!(result["total"], count);
                all.extend(keys(&result).into_iter().map(str::to_owned));
                match result["next_offset"].as_u64() {
                    Some(next) => offset = next as usize,
                    None => break,
                }
            }
            assert_eq!(all.len(), count);
            assert_eq!(all.iter().collect::<HashSet<_>>().len(), count);
            assert!(all.windows(2).all(|pair| pair[0] <= pair[1]));
            assert!(all.iter().all(|key| key.starts_with(prefix)));
        }
        let town = &catalog.index.towns[0];
        for (lang, name) in [("en", &town.en), ("zh", &town.zh), ("ko", &town.ko)] {
            let result = catalog
                .mcp_search_filtered(
                    "town",
                    &format!("town:{}", town.code),
                    lang,
                    0,
                    1,
                    &SearchFilters::default(),
                )
                .unwrap();
            assert_eq!(result["total"], 1);
            assert_eq!(result["results"][0]["name"], name.as_str());
        }
    }

    #[test]
    fn set_search_pages_in_gui_order_and_keeps_exact_priority() {
        let mut catalog = Catalog::open_for_mcp().unwrap();
        catalog.index.sets = (0..45)
            .map(|index| {
                let name = format!("Pagination fixture {index:02}");
                crate::db::rows::SetRow {
                    code: 990_000 + index,
                    kind: index % 3,
                    pieces: 5,
                    ko: name.clone(),
                    en: name.clone(),
                    zh: name.clone(),
                    names: name.to_lowercase(),
                    icon: None,
                }
            })
            .collect();
        let mut gui = catalog
            .index
            .set_hits("Pagination fixture", "pagination fixture", "en", "");
        assert_eq!(gui.len(), 45);
        gui.truncate(40);
        let first = catalog
            .mcp_search_filtered(
                "set",
                "Pagination fixture",
                "en",
                0,
                40,
                &SearchFilters::default(),
            )
            .unwrap();
        assert_eq!(first["total"], 45);
        assert_eq!(first["next_offset"], 40);
        assert_eq!(
            first["results"],
            json!(gui.into_iter().map(hit).collect::<Vec<_>>())
        );
        let last = catalog.mcp_search("set", "Pagination fixture", "en", 40, 40);
        assert_eq!(last["total"], 45);
        assert_eq!(keys(&last).len(), 5);
        assert!(last["next_offset"].is_null());
        assert!(keys(&last).iter().all(|key| !keys(&first).contains(key)));

        let exact = catalog.mcp_search("set", "Pagination fixture 44", "en", 0, 50);
        assert_eq!(exact["total"], 1);
        assert_eq!(keys(&exact), ["set:990044"]);
    }

    #[test]
    fn filters_apply_before_pagination_and_preserve_alias_ranking() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let filter = SearchFilters {
            nation: Some(1),
            ..Default::default()
        };
        let original = catalog.mcp_search("hero", "", "en", 0, usize::MAX);
        let expected: Vec<_> = keys(&original)
            .into_iter()
            .filter(|key| {
                catalog
                    .index
                    .heroes
                    .iter()
                    .any(|row| row.nation == 1 && *key == format!("hero:{}", row.code))
            })
            .collect();
        let result = catalog
            .mcp_search_filtered("hero", "", "en", 2, 4, &filter)
            .unwrap();
        assert_eq!(result["total"], expected.len());
        assert_eq!(keys(&result), expected[2..6]);
        assert_eq!(result["next_offset"], 6);

        catalog
            .index
            .aliases
            .borrow_mut()
            .insert("filter-test".into(), ["6213".into(), "143".into()].into());
        let category = catalog.index.find_item(6213).unwrap().category.clone();
        let filter = SearchFilters {
            item_category: Some(category.clone()),
            ..Default::default()
        };
        let result = catalog
            .mcp_search_filtered("item", "filter-test", "en", 0, 50, &filter)
            .unwrap();
        let expected: Vec<_> = [6213, 143]
            .into_iter()
            .filter(|code| catalog.index.find_item(*code).unwrap().category == category)
            .map(|code| format!("item:{code}"))
            .collect();
        assert_eq!(result["total"], expected.len());
        assert_eq!(
            keys(&result).into_iter().collect::<BTreeSet<_>>(),
            expected.iter().map(String::as_str).collect()
        );
    }

    #[test]
    fn source_regions_monster_fields_and_recipe_levels_filter_exactly() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let filter = SearchFilters {
            region: Some("japan".into()),
            ..Default::default()
        };
        let towns = catalog
            .mcp_search_filtered("town", "", "zh", 1, 2, &filter)
            .unwrap();
        assert_eq!(
            towns["total"],
            catalog
                .index
                .towns
                .iter()
                .filter(|row| row.region == "japan")
                .count()
        );
        assert_eq!(keys(&towns).len(), 2);
        let npcs = catalog
            .mcp_search_filtered("npc", "", "ko", 0, 50, &filter)
            .unwrap();
        let expected: i64 = catalog
            .conn
            .query_row("SELECT count(*) FROM npcs WHERE nation='J'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(npcs["total"], expected);

        let filter = SearchFilters {
            region: Some("china".into()),
            element: Some(1),
            is_boss: Some(false),
            ..Default::default()
        };
        let monsters = catalog
            .mcp_search_filtered("monster", "", "en", 0, 50, &filter)
            .unwrap();
        let expected: i64 = catalog
            .conn
            .query_row(
                "SELECT count(*) FROM monsters WHERE nation=3 AND element=1 AND is_boss=0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(monsters["total"], expected);
        for key in keys(&monsters) {
            let fields: (i64, i64, bool) = catalog
                .conn
                .query_row(
                    "SELECT nation,element,is_boss FROM monsters WHERE code=?1",
                    [&key[8..]],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(fields, (3, 1, false));
        }
        let filter = SearchFilters {
            min_level: Some(1),
            max_level: Some(100),
            ..Default::default()
        };
        let recipes = catalog
            .mcp_search_filtered("recipe", "", "en", 0, 50, &filter)
            .unwrap();
        assert_eq!(
            recipes["total"],
            catalog
                .index
                .recipes
                .iter()
                .filter(|row| row
                    .level_min
                    .is_some_and(|level| (1..=100).contains(&level)))
                .count()
        );
        let equipment = catalog
            .index
            .items
            .iter()
            .find(|row| !row.category.is_empty() && !row.subtype.is_empty())
            .unwrap();
        let filter = SearchFilters {
            item_category: Some(equipment.category.clone()),
            item_subtype: Some(equipment.subtype.clone()),
            ..Default::default()
        };
        let items = catalog
            .mcp_search_filtered("item", "", "en", 0, 2, &filter)
            .unwrap();
        assert_eq!(items["total"], catalog.index.items.iter().filter(|row| row.category == equipment.category && row.subtype == equipment.subtype).count());
    }

    #[test]
    fn invalid_filters_report_errors_instead_of_empty_matches() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for (category, filter) in [
            ("unknown", SearchFilters::default()),
            (
                "hero",
                SearchFilters {
                    region: Some("japan".into()),
                    ..Default::default()
                },
            ),
            (
                "town",
                SearchFilters {
                    region: Some("nowhere".into()),
                    ..Default::default()
                },
            ),
            (
                "monster",
                SearchFilters {
                    nation: Some(1),
                    ..Default::default()
                },
            ),
            (
                "hero",
                SearchFilters {
                    nation: Some(2),
                    ..Default::default()
                },
            ),
            (
                "monster",
                SearchFilters {
                    element: Some(6),
                    ..Default::default()
                },
            ),
            (
                "npc",
                SearchFilters {
                    is_boss: Some(false),
                    ..Default::default()
                },
            ),
            (
                "item",
                SearchFilters {
                    min_level: Some(1),
                    ..Default::default()
                },
            ),
            (
                "recipe",
                SearchFilters {
                    min_level: Some(-1),
                    ..Default::default()
                },
            ),
            (
                "recipe",
                SearchFilters {
                    min_level: Some(100),
                    max_level: Some(1),
                    ..Default::default()
                },
            ),
            (
                "item",
                SearchFilters {
                    item_category: Some("unknown".into()),
                    ..Default::default()
                },
            ),
            (
                "item",
                SearchFilters {
                    item_subtype: Some("unknown".into()),
                    ..Default::default()
                },
            ),
            (
                "set",
                SearchFilters {
                    item_category: Some("A".into()),
                    ..Default::default()
                },
            ),
        ] {
            assert!(
                catalog
                    .mcp_search_filtered(category, "", "en", 0, 10, &filter)
                    .is_err(),
                "accepted {category}: {filter:?}"
            );
        }
        assert!(catalog
            .mcp_search_filtered("item", "", "en", 0, 0, &SearchFilters::default())
            .is_err());
        assert!(serde_json::from_value::<SearchFilters>(json!({"unknown_filter": 1})).is_err());
        let options = catalog.mcp_filter_options();
        assert_eq!(options["item_level"]["supported"], false);
        assert!(options["item_categories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| option["value"] == "A"));
        assert_eq!(
            options["hero_nations"].as_array().unwrap().len(),
            HERO_NATIONS.len()
        );
    }
}
