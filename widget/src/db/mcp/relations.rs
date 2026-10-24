//! Complete catalog relations. GUI section limits must never become MCP limits.
use super::*;

pub(super) fn relation_names(kind: &str) -> &'static [&'static str] {
    match kind {
        "item" => &[
            "shops",
            "production",
            "quest_rewards",
            "uses",
            "recipes",
            "drops",
            "sets",
            "characters",
        ],
        "monster" => &["drops", "quests"],
        "set" => &["members"],
        "town" => &["goods"],
        "bay" => &["products"],
        "yard" => &["facilities"],
        "npc" => &["quests"],
        "hero" | "quest" | "recipe" => &["related"],
        _ => &[],
    }
}

fn wire_language(lang: &str) -> &str {
    super::super::language::primary_lang(lang)
}

pub(super) fn next_call(
    key: &str,
    relation: &str,
    lang: &str,
    offset: usize,
    limit: usize,
) -> Value {
    json!({"tool": "get_relations", "arguments": {
        "key": key, "relation": relation, "language": wire_language(lang), "offset": offset, "limit": limit
    }})
}

impl Catalog {
    pub fn mcp_relations(
        &self,
        key: &str,
        relation: &str,
        lang: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value, String> {
        if !(1..=50).contains(&limit) || offset > 1_000_000 {
            return Err("limit must be 1–50 and offset must be 0–1000000".into());
        }
        let (kind, id) = key.split_once(':').ok_or("Use a typed catalog key")?;
        if !self.mcp_exists(kind, id) {
            return Err(format!("Catalog record not found: {key}"));
        }
        if !relation_names(kind).contains(&relation) {
            return Err(format!(
                "Unsupported relation {relation:?} for {kind}; supported: {}",
                relation_names(kind).join(", ")
            ));
        }
        let mut response = if kind == "item" && relation == "drops" {
            self.mcp_drops(
                id.parse::<i64>().map_err(|error| error.to_string())?,
                lang,
                offset,
                limit,
            )?
        } else if kind == "item" && relation == "uses" {
            self.mcp_item_uses_page(
                id.parse::<i64>().map_err(|error| error.to_string())?,
                lang,
                offset,
                limit,
            )?
        } else {
            page(self.mcp_relation_rows(key, relation, lang)?, offset, limit)
        };
        response["key"] = json!(key);
        response["relation"] = json!(relation);
        response["next_call"] = response["next_offset"]
            .as_u64()
            .map(|offset| next_call(key, relation, lang, offset as usize, limit))
            .unwrap_or(Value::Null);
        Ok(response)
    }

    pub(super) fn mcp_relation_rows(
        &self,
        key: &str,
        relation: &str,
        lang: &str,
    ) -> Result<Vec<Value>, String> {
        let (kind, id) = key.split_once(':').ok_or("Use a typed catalog key")?;
        let number = || id.parse::<i64>().map_err(|error| error.to_string());
        let mut rows = match (kind, relation) {
            ("item", "shops") => {
                let code = number()?;
                self.index
                    .towns
                    .iter()
                    .filter_map(|town| {
                        let good = town.goods.iter().find(|good| good.item == code)?;
                        let mut row = self.mcp_reference(&format!("town:{}", town.code), lang)?;
                        row["specialty"] = json!(good.specialty);
                        row["region"] = json!(town.region);
                        row["coordinates"] = json!({"x": town.x, "y": town.y});
                        Some(row)
                    })
                    .collect()
            }
            ("item", "production") => {
                let code = number()?;
                let mut places = self.factory_places(code, lang);
                if places.is_empty() {
                    if let Some(place) = self
                        .index
                        .formulas
                        .get(&code)
                        .and_then(|formula| self.inferred_place(formula, lang))
                    {
                        places.push(place);
                    }
                }
                let mut seen = BTreeSet::new();
                places
                    .into_iter()
                    .filter_map(|place| {
                        if !seen.insert(place.key.clone()) {
                            return None;
                        }
                        let mut row = self.mcp_reference(&place.key, lang)?;
                        row["building"] = json!(place.building);
                        row["region"] = json!(place.region);
                        Some(row)
                    })
                    .collect()
            }
            ("item", "quest_rewards") => self
                .item_quest_gains(number()?, lang)
                .into_iter()
                .map(hit)
                .collect(),
            ("item", "uses") => self.mcp_item_uses(number()?, lang)?,
            ("item", "recipes") => self
                .index
                .recipes
                .iter()
                .filter(|row| row.result_str == id)
                .map(|row| {
                    let mut result = json!({"key": format!("recipe:{}", row.id), "recipe_id": row.id,
                    "name": lead3(lang, &row.en, &row.zh, &row.ko), "source": row.source,
                    "item_code": number().unwrap_or_default(), "level_min": row.level_min});
                    self.mcp_attach_facility(&mut result, row.id, lang);
                    result
                })
                .collect(),
            ("item", "drops") => self.mcp_drops(number()?, lang, 0, usize::MAX)?["results"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
            ("item", "sets") => {
                let code = number()?;
                self.set_rows(code)
                    .into_iter()
                    .filter_map(|(set, set_kind, pieces, enhance)| {
                        let mut row = self.mcp_reference(&format!("set:{set}"), lang)?;
                        row["set_kind"] = json!(set_kind);
                        row["pieces"] = json!(pieces);
                        row["enhance"] = json!(enhance);
                        Some(row)
                    })
                    .collect()
            }
            ("item", "characters") => self.mcp_character_uses(number()?, lang)?,
            ("monster", "drops") => self.mcp_monster_drop_rows(id, lang)?,
            ("monster", "quests") => self.monster_quests(id, lang).into_iter().map(hit).collect(),
            ("set", "members") => {
                let mut statement = self.conn.prepare("SELECT item_code, enhance FROM item_set_members WHERE set_code=?1 ORDER BY item_code, enhance").map_err(|error| error.to_string())?;
                let members = statement
                    .query_map([number()?], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                    })
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                members
                    .into_iter()
                    .map(|(item, enhance)| {
                        let mut row = self.mcp_item_ref(item, lang);
                        row["enhance"] = json!(enhance);
                        row
                    })
                    .collect()
            }
            ("town", "goods") => self
                .index
                .towns
                .iter()
                .find(|row| row.code == id)
                .map(|town| {
                    town.goods
                        .iter()
                        .map(|good| {
                            let mut row = self.mcp_item_ref(good.item, lang);
                            row["specialty"] = json!(good.specialty);
                            row
                        })
                        .collect()
                })
                .unwrap_or_default(),
            ("bay", "products") => self
                .index
                .yards
                .get(id)
                .map(|yard| {
                    yard.products
                        .iter()
                        .map(|product| {
                            let mut row = self.mcp_item_ref(product.item, lang);
                            row["nations"] = json!(product.nations);
                            row
                        })
                        .collect()
                })
                .unwrap_or_default(),
            ("yard", "facilities") => self
                .index
                .works
                .iter()
                .find(|row| row.code == id)
                .map(|building| {
                    building
                        .facilities
                        .iter()
                        .filter_map(|id| self.mcp_reference(&format!("bay:{id}"), lang))
                        .collect()
                })
                .unwrap_or_default(),
            ("npc", "quests") => self
                .quests_for_npc(number()?, lang)
                .into_iter()
                .map(hit)
                .collect(),
            ("hero", "related") => {
                let mut links = self.index.hero_chain(id, lang);
                links.extend(self.hero_data_links(id, lang));
                let mut section = String::new();
                links
                    .into_iter()
                    .filter_map(|row| {
                        if row.chip == "section" {
                            section = row.title;
                            return None;
                        }
                        if !row
                            .key
                            .split_once(':')
                            .is_some_and(|(kind, id)| self.mcp_exists(kind, id))
                        {
                            return None;
                        }
                        let mut row = hit(row);
                        if !section.is_empty() {
                            row["section"] = json!(section);
                        }
                        Some(row)
                    })
                    .collect()
            }
            ("quest", "related") => {
                let quest = number()?;
                let source = self.quest_source(quest);
                let mut rows = Vec::new();
                for (step_index, step) in self.quest_steps(quest).into_iter().enumerate() {
                    let key_kind = match step.kind.to_ascii_lowercase().as_str() {
                        "monster" => Some("monster"),
                        "item" | "useitem" | "wearitem" => Some("item"),
                        "npc" => Some("npc"),
                        "pet" => Some("hero"),
                        _ => None,
                    };
                    let references = if let Some(kind) = key_kind {
                        let mut seen = BTreeSet::new();
                        step.code
                            .split(',')
                            .map(str::trim)
                            .filter(|id| seen.insert(*id))
                            .filter_map(|id| self.mcp_reference(&format!("{kind}:{id}"), lang))
                            .collect::<Vec<_>>()
                    } else {
                        self.step_link(&source, &step, lang, &std::collections::HashSet::new())
                            .filter(|row| {
                                row.key
                                    .split_once(':')
                                    .is_some_and(|(kind, id)| self.mcp_exists(kind, id))
                            })
                            .map(hit)
                            .into_iter()
                            .collect()
                    };
                    for mut row in references {
                        row["role"] = json!(step.role);
                        row["requirement_kind"] = json!(step.kind);
                        row["quantity"] = json!(step.qty);
                        row["step_index"] = json!(step_index);
                        row["source_codes"] = json!(step.code);
                        rows.push(row);
                    }
                }
                rows
            }
            ("recipe", "related") => {
                let mut statement = self.conn.prepare("SELECT item_code, qty, enhance FROM recipe_materials WHERE recipe_id=?1 ORDER BY slot").map_err(|error| error.to_string())?;
                let materials = statement
                    .query_map([number()?], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    })
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                materials
                    .into_iter()
                    .map(|(item, quantity, enhance)| {
                        let mut row = self.mcp_item_ref(item, lang);
                        row["quantity"] = json!(quantity);
                        row["enhance"] = json!(enhance);
                        row
                    })
                    .collect()
            }
            _ => return Err(format!("Unsupported relation {relation:?} for {kind}")),
        };
        // Stable across languages and calls. Preserve separate slots/uses rather
        // than silently combining requirements with different meanings.
        rows.sort_by(|left, right| {
            left["key"]
                .as_str()
                .cmp(&right["key"].as_str())
                .then(
                    left["usage_kind"]
                        .as_str()
                        .cmp(&right["usage_kind"].as_str()),
                )
                .then(left["enhance"].as_i64().cmp(&right["enhance"].as_i64()))
        });
        Ok(rows)
    }

    fn mcp_item_uses(&self, code: i64, lang: &str) -> Result<Vec<Value>, String> {
        Ok(
            self.mcp_item_uses_page(code, lang, 0, usize::MAX)?["results"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        )
    }

    fn mcp_item_uses_page(
        &self,
        code: i64,
        lang: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value, String> {
        // Recipe uses can number in the thousands. Count them and page their
        // IDs before looking up names or building response JSON.
        let recipe_total: usize = self.conn.query_row("SELECT COUNT(DISTINCT r.id) FROM recipe_materials m JOIN recipes r ON r.id=m.recipe_id WHERE m.item_code=?1", [code], |row| row.get(0)).map_err(|error| error.to_string())?;
        let mut other: Vec<_> = self
            .item_quest_uses(code, lang)
            .into_iter()
            .map(|row| {
                let mut value = hit(row);
                value["usage_kind"] = json!("quest_requirement");
                value
            })
            .collect();
        for (item, formula) in &self.index.formulas {
            let parts: Vec<_> = formula
                .parts
                .iter()
                .filter(|part| part.item == code)
                .collect();
            if parts.is_empty() {
                continue;
            }
            let mut row = self.mcp_item_ref(*item, lang);
            row["usage_kind"] = json!("production_requirement");
            row["requirements"] = json!(parts
                .iter()
                .map(|part| json!({"kind": part.kind, "quantity": part.qty}))
                .collect::<Vec<_>>());
            other.push(row);
        }
        other.extend(
            self.mcp_character_uses(code, lang)?
                .into_iter()
                .map(|mut row| {
                    row["usage_kind"] = json!("character_requirement");
                    row
                }),
        );
        other.extend(self.mcp_enchant_material_uses(code, lang)?);
        other.sort_by(|left, right| {
            left["key"].as_str().cmp(&right["key"].as_str()).then(
                left["usage_kind"]
                    .as_str()
                    .cmp(&right["usage_kind"].as_str()),
            )
        });
        let total = recipe_total + other.len();
        let mut result = Vec::new();
        if offset < recipe_total {
            let count = limit.min(recipe_total - offset);
            let mut statement = self.conn.prepare("SELECT r.id, r.result_item, r.source, m.qty, m.enhance FROM recipe_materials m JOIN recipes r ON r.id=m.recipe_id WHERE m.item_code=?1 AND r.id IN (SELECT DISTINCT rr.id FROM recipe_materials mm JOIN recipes rr ON rr.id=mm.recipe_id WHERE mm.item_code=?1 ORDER BY rr.id LIMIT ?2 OFFSET ?3) ORDER BY r.id, m.slot").map_err(|error| error.to_string())?;
            let recipes = statement
                .query_map(
                    rusqlite::params![code, count as i64, offset as i64],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    },
                )
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            let mut grouped = BTreeMap::<i64, Value>::new();
            for (recipe, output, source, quantity, enhance) in recipes {
                let row = grouped.entry(recipe).or_insert_with(|| json!({"key": format!("recipe:{recipe}"),
                    "recipe_id": recipe, "name": self.mcp_item_ref(output, lang)["name"], "output_key": format!("item:{output}"),
                    "source": source, "usage_kind": "recipe_material", "requirements": []}));
                row["requirements"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"item_code": code, "quantity": quantity, "enhance": enhance}));
            }
            result.extend(grouped.into_iter().map(|(recipe, mut row)| {
                self.mcp_attach_facility(&mut row, recipe, lang);
                row
            }));
        }
        let remaining = limit.saturating_sub(result.len());
        result.extend(
            other
                .into_iter()
                .skip(offset.saturating_sub(recipe_total))
                .take(remaining),
        );
        let next = offset.saturating_add(result.len());
        Ok(
            json!({"results": result, "total": total, "offset": offset, "next_offset": if next < total { Some(next) } else { None }}),
        )
    }

    fn mcp_enchant_material_uses(&self, code: i64, lang: &str) -> Result<Vec<Value>, String> {
        if !self.has_table("enchant_recipes") || !self.has_table("enchant_materials") {
            return Ok(Vec::new());
        }
        let mut statement = self.conn.prepare(
            "SELECT r.item_code, r.recipe_key, m.slot, m.qty
             FROM enchant_materials m JOIN enchant_recipes r ON r.recipe_key=m.recipe_key
             WHERE m.item_code=?1 ORDER BY r.item_code, m.slot",
        ).map_err(|error| error.to_string())?;
        let rows = statement.query_map([code], |row| Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))).map_err(|error| error.to_string())?;
        let mut grouped = BTreeMap::<i64, Value>::new();
        for row in rows {
            let (scroll, recipe_key, slot, quantity) = row.map_err(|error| error.to_string())?;
            let usage = grouped.entry(scroll).or_insert_with(|| {
                let mut usage = self.mcp_item_ref(scroll, lang);
                usage["usage_kind"] = json!("enchant_material");
                usage["source"] = json!("enchant_materials");
                usage["recipe_key"] = json!(recipe_key);
                usage["requirements"] = json!([]);
                usage
            });
            // One usage per scroll, preserving every matching source slot.
            // Several scrolls may share a material table without sharing a use.
            usage["requirements"].as_array_mut().unwrap().push(
                json!({"item_code": code, "slot": slot, "quantity": quantity}),
            );
        }
        Ok(grouped.into_values().collect())
    }

    fn mcp_character_uses(&self, code: i64, lang: &str) -> Result<Vec<Value>, String> {
        let mut result: Vec<_> = self
            .index
            .roster_hits(code, lang)
            .into_iter()
            .map(hit)
            .collect();
        if self.has_table("character_materials") {
            let mut statement = self.conn.prepare("SELECT code,kind,qty FROM character_materials WHERE item_code=?1 ORDER BY code,kind,slot").map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([code], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            for (owner, kind, quantity) in rows {
                if let Some(mut row) = self.mcp_reference(&format!("hero:{owner}"), lang) {
                    row["material_kind"] = json!(kind);
                    row["quantity"] = json!(quantity);
                    row["source"] = json!("character_materials");
                    // The source code owns this requirement; evolution rows
                    // must not be represented as incoming acquisition costs.
                    row["scope"] = json!("Requirement recorded on this character; evolution materials describe progression from this form.");
                    result.push(row);
                }
            }
        }
        Ok(result)
    }

    fn mcp_monster_drop_rows(&self, code: &str, lang: &str) -> Result<Vec<Value>, String> {
        let rate_column = super::super::monster::drop_rate_column(&self.conn);
        let mut statement = self.conn.prepare(&format!("SELECT d.item_code, d.slot, {rate_column} FROM monster_drops d WHERE d.monster_code=?1 ORDER BY d.slot")).map_err(|error| error.to_string())?;
        let drops = statement
            .query_map([code], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(drops
            .into_iter()
            .map(|(item, slot, rate)| {
                let mut row = self.mcp_item_ref(item, lang);
                row["slot"] = json!(slot);
                row["solo_only"] = json!(slot >= 9);
                row.as_object_mut()
                    .unwrap()
                    .extend(drop_rates(rate).as_object().unwrap().clone());
                row
            })
            .collect())
    }

    pub(super) fn mcp_related_summary(&self, key: &str, lang: &str) -> Result<Value, String> {
        let kind = key
            .split_once(':')
            .map(|pair| pair.0)
            .ok_or("Use a typed catalog key")?;
        let mut all = Vec::new();
        let mut available = Vec::new();
        let mut total = 0;
        for &relation in relation_names(kind) {
            let remaining = 50usize.saturating_sub(all.len());
            let page = self.mcp_relations(key, relation, lang, 0, remaining.max(1))?;
            let count = page["total"].as_u64().unwrap_or(0) as usize;
            total += count;
            if count > 0 {
                available.push(json!({"relation": relation, "total": count, "next_call": next_call(key, relation, lang, 0, 20)}));
            }
            all.extend(
                page["results"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(remaining)
                    .cloned()
                    .map(|mut row| {
                        row["relation"] = json!(relation);
                        row
                    }),
            );
        }
        Ok(
            json!({"related": all.into_iter().take(50).collect::<Vec<_>>(), "related_total": total,
            "related_truncated": total > 50, "relations": available,
            "related_scope": "Summary only; each relation provides an executable get_relations call for its complete paginated list."}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enchant_material_uses_are_complete_and_unique_across_pages() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for (code, expected_count, example_scrolls, quantity) in [
            (2204, 39, [1127, 1137], 5),
            (2287, 2, [1129, 1130], 10),
        ] {
            let key = format!("item:{code}");
            let expected: BTreeSet<i64> = catalog.conn.prepare(
                "SELECT DISTINCT r.item_code FROM enchant_materials m
                 JOIN enchant_recipes r ON r.recipe_key=m.recipe_key WHERE m.item_code=?1",
            ).unwrap().query_map([code], |row| row.get(0)).unwrap()
                .collect::<Result<_, _>>().unwrap();
            assert_eq!(expected.len(), expected_count);
            let mut actual = BTreeSet::new();
            let mut all_uses = BTreeSet::new();
            let mut offset = 0;
            loop {
                let page = catalog.mcp_relations(&key, "uses", "zh", offset, 3).unwrap();
                for row in page["results"].as_array().unwrap() {
                    assert!(all_uses.insert((
                        row["key"].as_str().unwrap().to_string(),
                        row["usage_kind"].as_str().unwrap().to_string(),
                    )), "repeated usage across pages: {row}");
                    if row["usage_kind"] != "enchant_material" {
                        continue;
                    }
                    let scroll = row["item_code"].as_i64().unwrap();
                    assert!(actual.insert(scroll));
                    assert!(catalog.mcp_identity(row["key"].as_str().unwrap(), "zh").is_some());
                    assert_eq!(row["source"], "enchant_materials");
                    assert!(row.get("recipe_id").is_none());
                    for requirement in row["requirements"].as_array().unwrap() {
                        assert_eq!(requirement["item_code"], code);
                        if example_scrolls.contains(&scroll) {
                            assert_eq!(requirement["quantity"], quantity);
                        }
                    }
                }
                let Some(next) = page["next_offset"].as_u64() else {
                    assert_eq!(page["total"], all_uses.len());
                    assert!(page["next_call"].is_null());
                    break;
                };
                assert_eq!(page["next_call"]["arguments"]["offset"], next);
                assert!(next as usize > offset);
                offset = next as usize;
            }
            assert_eq!(actual, expected, "{key}");
        }
    }

    #[test]
    fn enchant_material_uses_keep_repeated_slots_and_shared_tables() {
        let mut catalog = Catalog::open_for_mcp().unwrap();
        catalog.conn = rusqlite::Connection::open_in_memory().unwrap();
        assert!(catalog.mcp_enchant_material_uses(2204, "en").unwrap().is_empty());
        catalog.conn.execute_batch(
            "CREATE TABLE enchant_recipes (item_code INTEGER PRIMARY KEY, recipe_key TEXT);
             CREATE TABLE enchant_materials (recipe_key TEXT, slot INTEGER, item_code INTEGER, qty INTEGER);
             INSERT INTO enchant_recipes VALUES (1127, 'shared'), (1137, 'shared'), (1151, 'other');
             INSERT INTO enchant_materials VALUES
                 ('shared', 4, 2204, 2), ('shared', 1, 2206, 99),
                 ('shared', 2, 2204, 5), ('other', 1, 2204, 9);",
        ).unwrap();
        let rows = catalog.mcp_enchant_material_uses(2204, "en").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.iter().map(|row| row["item_code"].as_i64().unwrap()).collect::<Vec<_>>(),
            [1127, 1137, 1151]);
        for row in &rows[..2] {
            assert_eq!(row["recipe_key"], "shared");
            assert_eq!(row["requirements"], json!([
                {"item_code": 2204, "slot": 2, "quantity": 5},
                {"item_code": 2204, "slot": 4, "quantity": 2},
            ]));
        }
        assert_eq!(rows[2]["requirements"], json!([
            {"item_code": 2204, "slot": 1, "quantity": 9},
        ]));
    }

    #[test]
    fn quest_use_pages_include_wearing_and_using_items() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for (key, quests) in [
            ("item:44", [829, 843, 857, 871]),
            ("item:143", [834, 848, 862, 876]),
        ] {
            let expected: BTreeSet<_> = quests.iter().map(|id| format!("quest:{id}")).collect();
            let mut actual = BTreeSet::new();
            let mut offset = 0;
            loop {
                let page = catalog.mcp_relations(key, "uses", "zh", offset, 3).unwrap();
                for row in page["results"].as_array().unwrap() {
                    if row["usage_kind"] == "quest_requirement" {
                        assert!(actual.insert(row["key"].as_str().unwrap().to_string()));
                        assert!(row.get("quantity").is_none());
                    }
                }
                let Some(next) = page["next_offset"].as_u64() else {
                    break;
                };
                offset = next as usize;
            }
            assert_eq!(actual, expected, "{key}");
        }
    }

    #[test]
    fn all_shops_and_recipe_uses_are_reachable_without_gui_caps() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for (key, relation, sql) in [
            ("item:20", "shops", "SELECT COUNT(DISTINCT town) FROM town_goods WHERE item_code=20"),
            ("item:4469", "quest_rewards", "SELECT COUNT(DISTINCT quest_id) FROM quest_steps WHERE lower(kind)='item' AND lower(role)='reward' AND code='4469'"),
        ] {
            let expected: usize = catalog.conn.query_row(sql, [], |row| row.get(0)).unwrap();
            let mut offset = 0;
            let mut keys = BTreeSet::new();
            loop {
                let page = catalog.mcp_relations(key, relation, "zh-en", offset, 7).unwrap();
                assert_eq!(page["total"], expected);
                for row in page["results"].as_array().unwrap() {
                    assert!(keys.insert(row["key"].as_str().unwrap().to_string()));
                }
                let Some(next) = page["next_offset"].as_u64() else {
                    assert!(page["next_call"].is_null());
                    break;
                };
                assert_eq!(page["next_call"]["arguments"]["offset"], next);
                assert_eq!(page["next_call"]["arguments"]["language"], "zh");
                offset = next as usize;
            }
            assert_eq!(keys.len(), expected);
        }
        let uses = catalog
            .mcp_relation_rows("item:5507", "uses", "en")
            .unwrap();
        let actual = uses
            .iter()
            .filter(|row| row["usage_kind"] == "recipe_material")
            .count();
        let expected: usize = catalog
            .conn
            .query_row(
                "SELECT COUNT(DISTINCT recipe_id) FROM recipe_materials WHERE item_code=5507",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(actual, expected);
        assert!(actual > 24);
    }

    #[test]
    fn relation_keys_are_followable_and_paging_rejects_invalid_requests() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for (key, relation) in [
            ("item:20", "shops"),
            ("item:597", "production"),
            ("town:KY000001", "goods"),
            ("bay:WEA", "products"),
            ("yard:FAC", "facilities"),
            ("set:66", "members"),
        ] {
            let page = catalog.mcp_relations(key, relation, "en", 0, 50).unwrap();
            assert!(page["total"].as_u64().unwrap() > 0);
            for row in page["results"].as_array().unwrap() {
                let key = row["key"].as_str().unwrap();
                assert!(!key.starts_with("section:"));
                assert!(catalog.mcp_identity(key, "en").is_some(), "{key}");
            }
        }
        assert!(catalog
            .mcp_relations("item:20", "members", "en", 0, 10)
            .is_err());
        assert!(catalog
            .mcp_relations("item:20", "shops", "en", 0, 0)
            .is_err());
        assert!(catalog
            .mcp_relations("item:0", "shops", "en", 0, 10)
            .is_err());
        let page = catalog
            .mcp_relations("item:20", "shops", "en", 999, 10)
            .unwrap();
        assert_eq!(page["results"], json!([]));
        assert!(page["next_call"].is_null());
    }
}
