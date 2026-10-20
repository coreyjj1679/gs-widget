//! Sectioned, typed entity data without forcing callers to fetch every relation.
use super::*;
use rusqlite::OptionalExtension;

const SECTIONS: &[&str] = &[
    "overview",
    "stats",
    "skills",
    "effects",
    "related",
    "production",
];

impl Catalog {
    pub(super) fn mcp_identity(&self, key: &str, lang: &str) -> Option<Value> {
        let (kind, id) = key.split_once(':')?;
        let (en, zh, ko) = match kind {
            "item" => {
                let row = self.index.find_item(id.parse().ok()?)?;
                (&row.en, &row.zh, &row.ko)
            }
            "monster" => {
                let row = self.index.monsters.iter().find(|row| row.code == id)?;
                (&row.en, &row.zh, &row.ko)
            }
            "hero" => {
                let row = self.index.heroes.iter().find(|row| row.code == id)?;
                (&row.en, &row.zh, &row.ko)
            }
            "npc" => {
                let row = self
                    .index
                    .npcs
                    .iter()
                    .find(|row| Some(row.code) == id.parse().ok())?;
                (&row.en, &row.zh, &row.ko)
            }
            "set" => {
                let row = self
                    .index
                    .sets
                    .iter()
                    .find(|row| Some(row.code) == id.parse().ok())?;
                (&row.en, &row.zh, &row.ko)
            }
            "recipe" => {
                let row = self
                    .index
                    .recipes
                    .iter()
                    .find(|row| Some(row.id) == id.parse().ok())?;
                (&row.en, &row.zh, &row.ko)
            }
            "quest" => {
                let row = self
                    .index
                    .quests
                    .iter()
                    .find(|row| Some(row.id) == id.parse().ok())?;
                (&row.title_en, &row.title_zh, &row.title_ko)
            }
            "town" => {
                let row = self.index.towns.iter().find(|row| row.code == id)?;
                (&row.en, &row.zh, &row.ko)
            }
            "bay" => {
                let row = self.index.yards.get(id)?;
                (&row.en, &row.zh, &row.ko)
            }
            "yard" => {
                let row = self.index.works.iter().find(|row| row.code == id)?;
                (&row.en, &row.zh, &row.ko)
            }
            _ => return None,
        };
        let name = lead3(lang, en, zh, ko);
        Some(
            json!({"key": key, "kind": kind, "name": if name.is_empty() { id } else { &name },
            "alternate_name": trail3(lang, en, zh, ko), "names": {"en": en, "zh": zh, "ko": if ko == "0" { "" } else { ko }}}),
        )
    }

    pub(super) fn mcp_item_ref(&self, code: i64, lang: &str) -> Value {
        let key = format!("item:{code}");
        let mut row = self.mcp_reference(&key, lang).unwrap_or_else(
            || json!({"key": key, "name": code.to_string(), "record_missing": true}),
        );
        row["item_code"] = json!(code);
        row
    }

    pub(super) fn mcp_reference(&self, key: &str, lang: &str) -> Option<Value> {
        let identity = self.mcp_identity(key, lang)?;
        Some(json!({"key": key, "kind": identity["kind"], "name": identity["name"]}))
    }

    pub fn mcp_entity_sections(
        &self,
        key: &str,
        lang: &str,
        sections: &[String],
    ) -> Result<Value, String> {
        for section in sections {
            if !SECTIONS.contains(&section.as_str()) {
                return Err(format!(
                    "Unsupported section {section:?}; supported: {}",
                    SECTIONS.join(", ")
                ));
            }
        }
        let (kind, id) = key
            .split_once(':')
            .ok_or("Use a key returned by search_catalog, such as item:6213")?;
        if !self.mcp_exists(kind, id) {
            return Err(format!(
                "Catalog record not found: {key}. Search for its name first."
            ));
        }
        for section in sections {
            let supported = match section.as_str() {
                "overview" | "related" => true,
                "stats" => matches!(kind, "hero" | "monster"),
                "skills" => kind == "hero",
                "effects" => matches!(kind, "hero" | "set"),
                "production" => kind == "item",
                _ => false,
            };
            if !supported {
                return Err(format!("Section {section:?} is not supported for {kind}"));
            }
        }
        let full = sections.is_empty();
        let want = |name: &str| full || sections.iter().any(|section| section == name);
        let mut result = self
            .mcp_identity(key, lang)
            .ok_or("Catalog record not found")?;
        if !full {
            result["sections"] = json!(sections);
        }

        if want("overview") {
            self.mcp_overview_fields(&mut result, kind, id, lang, full)?;
        }
        if want("stats") {
            match kind {
                "hero" => result["stats"] = self.mcp_hero_stats(id)?,
                "monster" => {
                    result["stats"] = self.conn.query_row(
                        "SELECT hp, mp, atk_min, atk_max, hit_resist, magic_resist, exp, element, element_gts, is_boss FROM monsters WHERE code=?1", [id],
                        |row| Ok(json!({"hp": row.get::<_, Option<i64>>(0)?, "mp": row.get::<_, Option<i64>>(1)?,
                            "attack_min": row.get::<_, Option<i64>>(2)?, "attack_max": row.get::<_, Option<i64>>(3)?,
                            "hit_resist": row.get::<_, Option<i64>>(4)?, "magic_resist": row.get::<_, Option<i64>>(5)?,
                            "experience": row.get::<_, Option<i64>>(6)?, "element": row.get::<_, Option<i64>>(7)?,
                            "element_value": row.get::<_, Option<i64>>(8)?, "is_boss": row.get::<_, i64>(9)? != 0})),
                    ).map_err(|error| error.to_string())?;
                    result["locations"] = json!(self.place_names(id, lang));
                }
                _ if !full => result["stats"] = Value::Null,
                _ => (),
            }
        }
        if want("skills") && (kind == "hero" || !full) {
            result["skills"] = if kind == "hero" {
                self.mcp_hero_skills(id, lang)?
            } else {
                json!([])
            };
        }
        if want("effects") {
            match kind {
                "hero" => result["effects"] = json!(facts(self.hero_effect_lines(id, lang))),
                "set" => {
                    let code = id.parse::<i64>().map_err(|error| error.to_string())?;
                    result["effects"] = json!(self.set_effect_rows(code).into_iter().map(|(pieces, stat, value, magic)| {
                        json!({"required_pieces": pieces, "stat": stat, "value": value, "magic_code": magic,
                            "description": self.effect_text(&stat, value, magic, lang)})
                    }).collect::<Vec<_>>());
                }
                _ if !full => result["effects"] = json!([]),
                _ => (),
            }
        }
        if want("production") {
            if kind == "item" {
                result["production"] = self
                    .mcp_production(id.parse::<i64>().map_err(|error| error.to_string())?, lang)?;
            } else if !full {
                result["production"] = Value::Null;
            }
        }
        if want("related") {
            result.as_object_mut().unwrap().extend(
                self.mcp_related_summary(key, lang)?
                    .as_object()
                    .unwrap()
                    .clone(),
            );
        }
        Ok(result)
    }

    fn mcp_overview_fields(
        &self,
        result: &mut Value,
        kind: &str,
        id: &str,
        lang: &str,
        full: bool,
    ) -> Result<(), String> {
        let sheet = match kind {
            "item" => self
                .item_sheet(id.parse().unwrap_or(0), lang)
                .map(|s| (s.0, s.1, s.2, s.3, Some(s.4))),
            "hero" => self.hero_sheet(id, lang).map(|s| {
                let mut rows = self.hero_effect_lines(id, lang);
                rows.extend(self.hero_stat_lines(id, lang));
                rows.extend(s.3);
                (s.0, s.1, s.2, rows, Some(s.4))
            }),
            "npc" => self
                .npc_sheet(id.parse().unwrap_or(0), lang)
                .map(|s| (s.0, s.1, s.2, s.3, Some(s.4))),
            "monster" => self
                .monster_sheet(id, lang)
                .map(|s| (s.0, s.1, s.2, s.3, None)),
            "recipe" => self
                .recipe_sheet(id.parse().unwrap_or(0), lang)
                .map(|s| (s.0, s.1, s.2, s.3, None)),
            "quest" => self.quest_sheet(id.parse().unwrap_or(0), lang).map(|s| {
                (
                    s.0,
                    s.1,
                    s.2,
                    s.3.into_iter()
                        .map(|(label, value, _)| (label, value))
                        .collect(),
                    Some(s.4),
                )
            }),
            "set" => self.set_sheet(id.parse().unwrap_or(0), lang).map(|s| {
                (
                    s.0,
                    s.1,
                    s.2,
                    s.3.into_iter()
                        .map(|(label, value, _, _)| (label, value))
                        .collect(),
                    None,
                )
            }),
            _ => None,
        };
        if let Some((name, alternate, heading, rows, description)) = sheet {
            result["name"] = json!(name);
            result["alternate_name"] = json!(alternate);
            result["heading"] = json!(heading);
            result["facts"] = json!(facts(rows));
            if let Some(description) = description {
                result["description"] = json!(description);
            }
        }
        match kind {
            "quest" => {
                let quest = id.parse::<i64>().map_err(|error| error.to_string())?;
                let source = self.quest_source(quest);
                result["quest_steps"] = json!(self.quest_steps(quest).into_iter().enumerate().map(|(step_index, step)| {
                    let key_kind = match step.kind.to_ascii_lowercase().as_str() {
                        "monster" => Some("monster"),
                        "item" | "useitem" | "wearitem" | "book" => Some("item"),
                        "npc" => Some("npc"),
                        "pet" => Some("hero"),
                        _ => None,
                    };
                    let references: Vec<_> = if let Some(kind) = key_kind {
                        let mut seen = BTreeSet::new();
                        step.code.split(',').map(str::trim).filter(|id| seen.insert(*id))
                            .filter_map(|id| self.mcp_reference(&format!("{kind}:{id}"), lang)).collect()
                    } else if step.kind.eq_ignore_ascii_case("quest") {
                        self.quest_id_by_code(&source, &step.code)
                            .and_then(|id| self.mcp_reference(&format!("quest:{id}"), lang)).into_iter().collect()
                    } else { Vec::new() };
                    let mut row = json!({"step_index": step_index, "role": step.role, "kind": step.kind,
                        "source_code": step.code, "quantity": step.qty, "label": step.label,
                        "references": references});
                    if references.len() == 1 { row["related_key"] = references[0]["key"].clone(); }
                    row
                }).collect::<Vec<_>>());
            }
            "recipe" => {
                result["materials"] =
                    json!(self.mcp_relation_rows(&format!("recipe:{id}"), "related", lang)?);
            }
            "town" => {
                let town = self.index.towns.iter().find(|row| row.code == id).unwrap();
                result["region"] = json!(town.region);
                result["coordinates"] = json!({"x": town.x, "y": town.y});
                result["goods_count"] = json!(town.goods.len());
            }
            "bay" => {
                let yard = &self.index.yards[id];
                result["building_keys"] = json!(self
                    .index
                    .works
                    .iter()
                    .filter(|building| building.facilities.iter().any(|facility| facility == id))
                    .map(|building| format!("yard:{}", building.code))
                    .collect::<Vec<_>>());
                result["product_count"] = json!(yard.products.len());
                result["upgrades"] = json!(yard
                    .upgrades
                    .iter()
                    .map(|row| lead3(lang, &row.en, &row.zh, &row.ko))
                    .collect::<Vec<_>>());
            }
            "yard" => {
                let building = self.index.works.iter().find(|row| row.code == id).unwrap();
                result["cost"] = json!(building.cost);
                result["required_credit"] = json!(building.credit);
                result["facility_count"] = json!(building.facilities.len());
            }
            _ => (),
        }
        // Retain legacy prose using the fields already loaded. Recipes and
        // quests additionally carry unique material-tree/narrative information.
        if full {
            let mut lines = Vec::new();
            for field in ["name", "alternate_name", "heading"] {
                if let Some(value) = result[field].as_str().filter(|value| !value.is_empty()) {
                    lines.push(value.to_string());
                }
            }
            for fact in result["facts"].as_array().into_iter().flatten() {
                let label = fact["label"].as_str().unwrap_or("");
                let value = fact["value"].as_str().unwrap_or("");
                lines.push(if label.is_empty() {
                    value.to_string()
                } else {
                    format!("{label}  {value}")
                });
            }
            if let Some(description) = result["description"]
                .as_str()
                .filter(|value| !value.is_empty())
            {
                lines.push(description.to_string());
            }
            if kind == "recipe" {
                let recipe = id.parse::<i64>().map_err(|error| error.to_string())?;
                lines.push(super::super::text::phrase("materials", lang));
                lines.push(self.material_tree(recipe, 3, &mut vec![recipe], lang));
            }
            result["text"] = if kind == "quest" {
                json!(self.quest_text(id.parse().unwrap_or(0), lang))
            } else {
                json!(lines.join("\n"))
            };
        }
        Ok(())
    }

    fn mcp_hero_stats(&self, code: &str) -> Result<Value, String> {
        if !self.has_table("character_stats") {
            return Ok(Value::Null);
        }
        let names = [
            "strength",
            "dexterity",
            "vitality",
            "intelligence",
            "hp",
            "mp",
            "hit_resist",
            "magic_resist",
            "attack_min",
            "attack_max",
            "move_speed",
            "attack_speed",
            "armor",
            "unit_type",
            "element",
            "element_value",
        ];
        self.conn.query_row("SELECT strength,dexterity,vitality,intelligence,hp,mp,hit_resist,magic_resist,atk_min,atk_max,move_speed,attack_speed,armor,unit_type,element,element_value FROM character_stats WHERE code=?1", [code], |row| {
            let mut object = serde_json::Map::new();
            for (index, name) in names.iter().enumerate() { object.insert((*name).into(), json!(row.get::<_, Option<i64>>(index)?)); }
            Ok(Value::Object(object))
        }).optional().map(|row| row.unwrap_or(Value::Null)).map_err(|error| error.to_string())
    }

    fn mcp_hero_skills(&self, code: &str, lang: &str) -> Result<Value, String> {
        if !self.has_table("character_skills") {
            return Ok(json!([]));
        }
        let mut statement = self
            .conn
            .prepare(
                "SELECT slot,name_ko,desc_gts FROM character_skills WHERE code=?1 ORDER BY slot",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([code], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let mut skills = Vec::new();
        for (slot, name, desc_gts) in rows {
            let text = |language: &str| {
                self.conn.query_row("SELECT text FROM strings WHERE object='OnlineText.dat' AND lang=?1 AND id=printf('%08d',?2)", rusqlite::params![language, desc_gts], |row| row.get::<_, String>(0)).optional().map(|value| value.unwrap_or_default()).map_err(|error| error.to_string())
            };
            let clean = |value: &str| {
                let value = super::super::hero::tidy(value);
                if value == "0" {
                    String::new()
                } else {
                    value
                }
            };
            let en = clean(&text("en")?);
            let zh = clean(&text("zh")?);
            let name = clean(&name);
            let description = lead3(lang, &en, &zh, "");
            if name.is_empty() && description.is_empty() {
                continue;
            }
            skills.push(
                json!({"slot": slot, "name": name, "name_ko": name, "description": description,
                "descriptions": {"en": en, "zh": zh}, "description_gts": desc_gts}),
            );
        }
        Ok(json!(skills))
    }

    pub(super) fn mcp_production(&self, code: i64, lang: &str) -> Result<Value, String> {
        let facilities = self.mcp_relation_rows(&format!("item:{code}"), "production", lang)?;
        let Some(formula) = self.index.formulas.get(&code) else {
            return Ok(
                json!({"status": if facilities.is_empty() { "not_recorded" } else { "facilities_only" }, "facilities": facilities, "formula": null}),
            );
        };
        let parts: Vec<_> = formula
            .parts
            .iter()
            .map(|part| {
                let mut row = self.mcp_item_ref(part.item, lang);
                row["kind"] = json!(part.kind);
                row["quantity"] = json!(part.qty);
                row
            })
            .collect();
        Ok(
            json!({"status": "recorded", "facilities": facilities, "formula": {
            "country_mask": formula.country, "labor": formula.labor, "yield_min": formula.yield_n,
            "yield_max": formula.yield_max, "plant_key": if formula.plant.is_empty() { None } else { Some(format!("bay:{}", formula.plant)) },
            "requirements": parts}, "scope": "Recorded production requirements; tools are distinguished from consumed materials. This is not a computed cost."}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_return_numeric_character_stats_without_unrequested_documents() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let row = catalog
            .mcp_entity_sections("hero:T49", "en", &["stats".into(), "skills".into()])
            .unwrap();
        let expected: Option<i64> = catalog
            .conn
            .query_row(
                "SELECT hp FROM character_stats WHERE code='T49'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(row["stats"]["hp"], json!(expected));
        assert!(row.get("text").is_none());
        assert!(row.get("related").is_none());
        assert!(row["skills"].is_array());
        assert!(catalog
            .mcp_entity_sections("hero:T49", "en", &["wrong".into()])
            .is_err());
        assert!(catalog
            .mcp_entity_sections("item:20", "en", &["skills".into()])
            .unwrap_err()
            .contains("not supported for item"));
    }

    #[test]
    fn full_entity_reports_true_relation_counts_and_filters_ui_headings() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let row = catalog.mcp_entity("item:5507", "en").unwrap();
        assert!(row["text"].is_string());
        assert!(row["related_total"].as_u64().unwrap() > 1000);
        assert_eq!(row["related_truncated"], true);
        assert_eq!(row["related"].as_array().unwrap().len(), 50);
        assert!(row["related"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| !row["key"].as_str().unwrap().starts_with("section:")));
        let town = catalog
            .mcp_entity_sections("town:KY000001", "zh", &["overview".into()])
            .unwrap();
        assert!(town["coordinates"]["x"].is_number());
        assert!(
            catalog.mcp_entity("bay:WEA", "en").unwrap()["product_count"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(
            catalog.mcp_entity("yard:FAC", "en").unwrap()["facility_count"]
                .as_u64()
                .unwrap()
                > 0
        );
    }

    #[test]
    fn set_effects_and_production_preserve_typed_requirements() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let set = catalog
            .mcp_entity_sections("set:66", "en", &["effects".into()])
            .unwrap();
        assert!(set["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["required_pieces"].is_number() && row["value"].is_number()));
        let item = catalog
            .mcp_entity_sections("item:597", "en", &["production".into()])
            .unwrap();
        assert!(item["production"]["formula"]["labor"].is_number());
        assert!(item["production"]["formula"]["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["key"] == "item:63" && row["quantity"] == 200));
        assert!(item.get("related").is_none());
    }

    #[test]
    fn legacy_text_retains_names_facts_descriptions_and_recipe_tree() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for key in ["item:20", "hero:T49", "monster:M24", "npc:160"] {
            let entity = catalog.mcp_entity(key, "en").unwrap();
            let text = entity["text"].as_str().unwrap();
            assert!(text.contains(entity["name"].as_str().unwrap()));
            for row in entity["facts"].as_array().into_iter().flatten() {
                assert!(text.contains(row["value"].as_str().unwrap()));
            }
            if let Some(description) = entity["description"].as_str() {
                assert!(text.contains(description));
            }
        }
        let recipe = catalog.mcp_entity("recipe:659", "en").unwrap();
        let tree = catalog.material_tree(659, 3, &mut vec![659], "en");
        assert!(recipe["text"].as_str().unwrap().contains(&tree));
    }

    #[test]
    fn compact_overview_preserves_direct_recipe_materials_and_quest_rewards() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let recipe = catalog
            .mcp_entity_sections("recipe:659", "en", &["overview".into()])
            .unwrap();
        let material_count: i64 = catalog
            .conn
            .query_row(
                "SELECT COUNT(*) FROM recipe_materials WHERE recipe_id=659",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(material_count > 0);
        assert_eq!(
            recipe["materials"].as_array().unwrap().len(),
            material_count as usize
        );
        assert!(recipe["materials"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["quantity"].is_number()
                && row["key"].as_str().unwrap().starts_with("item:")));
        assert!(recipe.get("text").is_none());
        let (quest_id, code, quantity): (i64, String, String) = catalog.conn.query_row("SELECT quest_id,code,qty FROM quest_steps WHERE role='reward' AND lower(kind)='item' AND code='4469' LIMIT 1", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        let quest = catalog
            .mcp_entity_sections(&format!("quest:{quest_id}"), "zh", &["overview".into()])
            .unwrap();
        assert!(quest["quest_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["role"] == "reward"
                && row["source_code"] == code
                && row["quantity"] == quantity
                && row["related_key"] == "item:4469"));
        assert!(quest.get("text").is_none());
        let multi = catalog
            .mcp_entity_sections("quest:95", "en", &["overview".into()])
            .unwrap();
        assert!(multi["quest_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["source_code"] == "D97,D98"
                && row["references"].as_array().unwrap().len() == 2));
    }
}
