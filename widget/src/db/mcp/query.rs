//! Task-oriented lookup: resolve names before fetching only the requested facts.
//! This is deterministic catalog/terminology matching, not a language model.
use super::*;
use crate::db::{hero, terminology, text};

fn normalized(value: &str) -> String {
    terminology::normalized(value)
}

pub(super) fn allowed(key: &str, category: Option<&str>) -> bool {
    category.is_none_or(|kind| {
        let kind = match kind {
            "facility" => "bay",
            "building" => "yard",
            _ => kind,
        };
        key.split_once(':').is_some_and(|(found, _)| found == kind)
    })
}

fn inferred_sources(query: &str) -> Vec<String> {
    let q = normalized(query);
    let groups: &[(&[&str], &[&str])] = &[
        (
            &[
                "邊度打",
                "边度打",
                "邊隻怪",
                "哪隻怪",
                "掉落",
                "drop",
                "드랍",
                "드롭",
            ],
            &["monster_drops"],
        ),
        (
            &["邊度買", "哪里买", "哪裡買", "商店", "shop", "상점"],
            &["shops"],
        ),
        (
            &["任務獎勵", "任务奖励", "quest reward", "퀘스트 보상"],
            &["quest_rewards"],
        ),
        (
            &["點合", "点合", "合成", "製作", "制作", "craft", "제작"],
            &["recipes", "production"],
        ),
    ];
    let mut result = Vec::new();
    for (words, sources) in groups {
        if words.iter().any(|word| q.contains(word)) {
            result.extend(sources.iter().map(|source| source.to_string()));
        }
    }
    result.sort();
    result.dedup();
    result
}

impl Catalog {
    fn query_resolution(&self, query: &str) -> terminology::TermResolution {
        let mut resolution = match &self.mcp_terminology {
            Some(book) => terminology::resolve_terms_with(book, query),
            None => terminology::resolve_terms(query),
        };
        let folded = normalized(query);
        let aliases = self.index.aliases.borrow();
        let mut matches = Vec::new();
        for (alias, codes) in aliases.iter() {
            let name = normalized(alias);
            if name.chars().count() < 2 {
                continue;
            }
            for (start, value) in folded.match_indices(&name) {
                let end = start + value.len();
                let word = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
                if name.starts_with(word) && folded[..start].chars().next_back().is_some_and(word) {
                    continue;
                }
                if name.ends_with(word) && folded[end..].chars().next().is_some_and(word) {
                    continue;
                }
                matches.push((alias, codes, start, end));
            }
        }
        let selected: Vec<_> = matches
            .iter()
            .filter(|(_, _, start, end)| {
                !matches
                    .iter()
                    .any(|(_, _, a, b)| a <= start && b >= end && b - a > end - start)
            })
            .collect();
        resolution.entities.retain(|entity| {
            let name = normalized(&entity.matched_alias);
            folded.match_indices(&name).any(|(start, value)| {
                !selected
                    .iter()
                    .any(|(_, _, a, b)| *a <= start && *b >= start + value.len())
            })
        });
        let mut seen = BTreeSet::new();
        for (alias, codes, _, _) in selected {
            if !seen.insert(*alias) {
                continue;
            }
            let mut keys: Vec<_> = codes.iter().map(|code| format!("item:{code}")).collect();
            keys.sort();
            resolution.entities.push(terminology::EntityMatch {
                term_id: "user-item-alias".into(),
                matched_alias: (*alias).clone(),
                keys,
                group: true,
                evidence: vec![terminology::Evidence {
                    source: "user_alias".into(),
                    reference: "aliases.txt".into(),
                    note: "User-defined item alias".into(),
                    spellings: vec![(*alias).clone()],
                }],
            });
        }
        resolution
    }

    fn query_identity(&self, key: &str, lang: &str) -> Option<Value> {
        let (kind, id) = key.split_once(':')?;
        let (en, zh, ko, label) = match kind {
            "hero" => {
                let row = self.index.heroes.iter().find(|row| row.code == id)?;
                (
                    &row.en,
                    &row.zh,
                    &row.ko,
                    hero::hero_rank_pair(row.rank).0.to_string(),
                )
            }
            "item" => {
                let row = self.index.find_item(id.parse().ok()?)?;
                (&row.en, &row.zh, &row.ko, row.category.clone())
            }
            "monster" => {
                let row = self.index.monsters.iter().find(|row| row.code == id)?;
                (&row.en, &row.zh, &row.ko, "monster".into())
            }
            "npc" => {
                let row = self
                    .index
                    .npcs
                    .iter()
                    .find(|row| Some(row.code) == id.parse().ok())?;
                (&row.en, &row.zh, &row.ko, "npc".into())
            }
            "set" => {
                let row = self
                    .index
                    .sets
                    .iter()
                    .find(|row| Some(row.code) == id.parse().ok())?;
                (&row.en, &row.zh, &row.ko, "equipment set".into())
            }
            "recipe" => {
                let row = self
                    .index
                    .recipes
                    .iter()
                    .find(|row| Some(row.id) == id.parse().ok())?;
                (&row.en, &row.zh, &row.ko, row.source.clone())
            }
            "quest" => {
                let row = self
                    .index
                    .quests
                    .iter()
                    .find(|row| Some(row.id) == id.parse().ok())?;
                (
                    &row.title_en,
                    &row.title_zh,
                    &row.title_ko,
                    row.source.clone(),
                )
            }
            "town" => {
                let row = self.index.towns.iter().find(|row| row.code == id)?;
                (&row.en, &row.zh, &row.ko, "town".into())
            }
            "yard" => {
                let row = self.index.works.iter().find(|row| row.code == id)?;
                (&row.en, &row.zh, &row.ko, "building".into())
            }
            "bay" => {
                let row = self.index.yards.get(id)?;
                (&row.en, &row.zh, &row.ko, "facility".into())
            }
            _ => return None,
        };
        let name = text::lead3(lang, en, zh, ko);
        Some(json!({"key": key, "kind": kind,
            "name": if name.is_empty() { id.to_string() } else { name },
            "names": {"zh": zh, "en": en, "ko": if ko == "0" { "" } else { ko }},
            "label": label}))
    }

    fn query_canonical_matches(
        &self,
        query: &str,
        category: Option<&str>,
    ) -> Vec<(String, usize, usize)> {
        let query = normalized(query);
        self.mcp_names
            .get_or_init(|| super::NameIndex::load(&self.index))
            .matches(&query, category)
    }

    #[cfg(test)]
    pub fn mcp_query(
        &self,
        query: &str,
        keys: &[String],
        category: Option<&str>,
        action: &str,
        lang: &str,
        limit: usize,
    ) -> Result<Value, String> {
        self.mcp_query_scoped(query, keys, category, action, lang, limit, &[], &[], 0)
    }

    pub fn mcp_query_scoped(
        &self,
        query: &str,
        keys: &[String],
        category: Option<&str>,
        action: &str,
        lang: &str,
        limit: usize,
        sources: &[String],
        sections: &[String],
        candidate_offset: usize,
    ) -> Result<Value, String> {
        let sources = if sources.is_empty() {
            inferred_sources(query)
        } else {
            sources.to_vec()
        };
        let resolution = self.query_resolution(query);
        let mut resolution_json =
            serde_json::to_value(&resolution).map_err(|error| error.to_string())?;
        // Keep the matched spelling and evidence reference, not every glossary
        // spelling and editorial note, in the model's response context.
        if let Some(entities) = resolution_json["entities"].as_array_mut() {
            for entity in entities {
                if let Some(evidence) = entity["evidence"].as_array_mut() {
                    for entry in evidence {
                        if let Some(entry) = entry.as_object_mut() {
                            entry.remove("spellings");
                            entry.remove("note");
                        }
                    }
                }
            }
        }
        let action = if action == "auto" {
            if resolution_json["intent"] == "acquisition" || !sources.is_empty() {
                "acquisition"
            } else {
                "overview"
            }
        } else {
            action
        };
        let mut selected = Vec::new();
        let mut group = false;
        let mut source = "explicit_keys";
        let mut exact_personal = false;
        if !keys.is_empty() {
            for key in keys {
                if !allowed(key, category) {
                    return Err("An explicit key does not match category".into());
                }
                if self.query_identity(key, lang).is_none() {
                    return Err(format!("Catalog record not found: {key}"));
                }
                if !selected.contains(key) {
                    selected.push(key.clone());
                }
            }
            group = true;
        } else {
            let personal = self.index.aliases.borrow();
            let exact_alias = personal.get(&normalized(query));
            if let Some(codes) = exact_alias {
                selected.extend(
                    codes
                        .iter()
                        .map(|code| format!("item:{code}"))
                        .filter(|key| allowed(key, category)),
                );
                group = true;
                source = "user_alias";
                exact_personal = true;
            } else if resolution.unresolved_modifiers.is_empty() {
                // Only overlapping aliases are superseded by a longer canonical
                // name. Keep independent names in the sentence as ambiguity.
                let canonical = self.query_canonical_matches(query, category);
                selected.extend(canonical.iter().map(|(key, _, _)| key.clone()));
                source = "canonical_name";
                let folded = normalized(query);
                for entity in &resolution.entities {
                    let alias = normalized(&entity.matched_alias);
                    let outside_canonical = folded.match_indices(&alias).any(|(start, value)| {
                        !canonical.iter().any(|(key, a, b)| {
                            *a <= start
                                && *b >= start + value.len()
                                && (*b - *a > value.len() || entity.keys.contains(key))
                        })
                    });
                    if !outside_canonical {
                        continue;
                    }
                    // Personal item aliases take precedence over a bundled spelling.
                    let found: Vec<_> = match personal.get(&normalized(&entity.matched_alias)) {
                        Some(codes) => {
                            source = "user_alias";
                            codes.iter().map(|code| format!("item:{code}")).collect()
                        }
                        None => entity.keys.clone(),
                    };
                    selected.extend(found.into_iter().filter(|key| allowed(key, category)));
                    group |= entity.group && canonical.is_empty();
                    if source != "user_alias" {
                        source = if canonical.is_empty() {
                            "terminology"
                        } else {
                            "canonical_name_and_terminology"
                        };
                    }
                }
            }
            selected.retain(|key| self.query_identity(key, lang).is_some());
        }
        if keys.is_empty() {
            selected.sort();
            selected.dedup();
        }
        // A glossary group is intentional, but two independent entity names need clarification.
        if resolution.entities.len() > 1 && keys.is_empty() && !exact_personal {
            group = false;
        }
        let ambiguous =
            selected.len() > 1 && (!group || (action == "calculate" && keys.is_empty()));
        let total = selected.len();
        let mut response = json!({"status": if total == 0 { "not_found" } else if ambiguous { "ambiguous" } else { "resolved" },
            "action": action, "resolution": resolution_json, "match_source": source,
            "results": [], "candidates": [], "matched_count": total, "truncated": total > limit});
        if ambiguous {
            response["candidate_offset"] = json!(candidate_offset);
            let shown: Vec<_> = selected.iter().skip(candidate_offset).take(limit).collect();
            let next = candidate_offset.saturating_add(shown.len());
            response["truncated"] = json!(next < total);
            response["candidates"] = json!(shown
                .iter()
                .filter_map(|key| self.query_identity(key, lang))
                .collect::<Vec<_>>());
            response["next_step"] = json!(
                "Choose the intended candidate key. Different forms are not interchangeable."
            );
            response["next_calls"] = json!(shown.iter().map(|key| json!({
                "tool": "query_catalog", "requires_choice": true,
                "arguments": {"keys": [key], "action": action, "language": language_code(lang), "limit": limit, "sources": if action == "acquisition" { sources.clone() } else { vec![] }, "sections": sections}
            })).collect::<Vec<_>>());
            response["next_call"] = if next < total {
                let mut arguments = json!({"query": query, "action": action, "language": language_code(lang), "candidate_offset": next, "limit": limit,
                    "sources": if action == "acquisition" { sources.clone() } else { vec![] }, "sections": sections});
                if let Some(category) = category {
                    arguments["category"] = json!(category);
                }
                json!({"tool": "query_catalog", "arguments": arguments})
            } else {
                Value::Null
            };
        } else if total == 0 {
            // Suggestions never become an automatic entity selection.
            if let Some(category) = category {
                response["candidates"] = self.mcp_search_filtered(
                    category,
                    query,
                    lang,
                    0,
                    limit,
                    &SearchFilters::default(),
                )?["results"]
                    .clone();
            }
            response["next_step"] = json!("Use a canonical name, known alias or stable key; optionally narrow category. No missing game facts are inferred.");
        } else {
            let count = if keys.is_empty() { limit } else { keys.len() };
            let results = selected
                .iter()
                .take(count)
                .map(|key| {
                    if action == "acquisition" {
                        self.query_acquisition(key, lang, limit, &sources)
                    } else if action == "calculate" {
                        self.query_identity(key, lang)
                            .ok_or_else(|| "Record not found".into())
                    } else {
                        self.query_overview(key, lang, limit, sections)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            response["results"] = json!(results);
            response["truncated"] = json!(total > count);
            if total > count {
                response["remaining_keys"] = json!(selected.iter().skip(count).collect::<Vec<_>>());
                response["next_step"] = json!("Continue with the remaining keys.");
                response["next_call"] = json!({"tool": "query_catalog", "arguments": {
                    "keys": selected.iter().skip(count).take(8).collect::<Vec<_>>(), "action": action,
                    "language": language_code(lang), "limit": limit, "sources": if action == "acquisition" { sources.clone() } else { vec![] }, "sections": sections
                }});
            }
        }
        if action == "compare" {
            let results = response["results"].as_array().cloned().unwrap_or_default();
            let fields: BTreeSet<_> = results
                .iter()
                .filter_map(|row| row["stats"].as_object())
                .flat_map(|stats| stats.keys().cloned())
                .collect();
            response["comparison"] = json!({
                "keys": results.iter().map(|row| row["key"].clone()).collect::<Vec<_>>(),
                "stats": fields.iter().map(|field| json!({"field": field,
                    "values": results.iter().map(|row| row["stats"][field].clone()).collect::<Vec<_>>()
                })).collect::<Vec<_>>(),
                "note": "Null means unavailable; statistics from different entity kinds may have different meanings."
            });
        }
        Ok(response)
    }

    pub(super) fn mcp_terminology_hits(
        &self,
        category: &str,
        query: &str,
        lang: &str,
    ) -> Option<Vec<Value>> {
        let resolution = self.query_resolution(query);
        if !resolution.unresolved_modifiers.is_empty() {
            return Some(Vec::new());
        }
        let personal = self.index.aliases.borrow();
        let keys: BTreeSet<_> = resolution
            .entities
            .into_iter()
            .flat_map(|entity| {
                personal
                    .get(&normalized(&entity.matched_alias))
                    .map(|codes| {
                        codes
                            .iter()
                            .map(|code| format!("item:{code}"))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or(entity.keys)
            })
            .filter(|key| allowed(key, Some(category)))
            .collect();
        if keys.is_empty() {
            return None;
        }
        Some(keys.into_iter().filter_map(|key| {
            let identity = self.query_identity(&key, lang)?;
            let names = &identity["names"];
            Some(json!({"key": key, "name": identity["name"],
                "description": text::trail3(lang, names["en"].as_str().unwrap_or(""), names["zh"].as_str().unwrap_or(""), names["ko"].as_str().unwrap_or("")),
                "note": identity["label"], "group": "", "solo_drop": false}))
        }).collect())
    }

    fn query_overview(
        &self,
        key: &str,
        lang: &str,
        limit: usize,
        sections: &[String],
    ) -> Result<Value, String> {
        let defaults = match key.split(':').next().unwrap_or("") {
            "hero" | "monster" => vec!["overview".into(), "stats".into()],
            "set" => vec!["overview".into(), "effects".into()],
            _ => vec!["overview".into()],
        };
        let mut result = self.mcp_entity_sections(
            key,
            lang,
            if sections.is_empty() {
                &defaults
            } else {
                sections
            },
        )?;
        // Remove prose only when its description is preserved separately.
        // Recipe prose still carries material-tree details absent from facts.
        if (result.get("description").is_some() && !key.starts_with("quest:"))
            || key.starts_with("monster:")
        {
            result.as_object_mut().unwrap().remove("text");
        }
        if let Some(rows) = result.get_mut("related").and_then(Value::as_array_mut) {
            let truncated = rows.len() > limit;
            rows.truncate(limit);
            if truncated {
                result["related_truncated"] = json!(true);
            }
        }
        if let Some(identity) = self.query_identity(key, lang) {
            for field in ["name", "names", "label"] {
                result[field] = identity[field].clone();
            }
        }
        Ok(result)
    }

    fn query_material(&self, code: i64, qty: i64, lang: &str) -> Value {
        json!({"key": format!("item:{code}"), "item_code": code,
            "name": self.item_name(code, lang), "quantity": qty})
    }

    fn query_hero_ref(&self, code: &str, lang: &str) -> Value {
        self.query_identity(&format!("hero:{code}"), lang)
            .unwrap_or_else(
                || json!({"key": format!("hero:{code}"), "name": code, "record_missing": true}),
            )
    }

    fn query_acquisition(
        &self,
        key: &str,
        lang: &str,
        limit: usize,
        sources: &[String],
    ) -> Result<Value, String> {
        let mut result = self.query_identity(key, lang).ok_or("Record not found")?;
        let (kind, id) = key.split_once(':').ok_or("Invalid key")?;
        match kind {
            "hero" => result["acquisition"] = self.query_hero_acquisition(id, lang),
            "item" => {
                let code = id.parse::<i64>().map_err(|error| error.to_string())?;
                let mut acquisition = json!({"scope": "Recorded acquisition sources only; missing records do not prove impossibility. No live prices."});
                let selected =
                    |source: &str| sources.is_empty() || sources.iter().any(|s| s == source);
                if selected("recipes") {
                    let mut recipes = self.query_item_recipes(code, lang, limit)?;
                    recipes["next_call"] = if recipes["next_offset"].is_number() {
                        json!({"tool": "get_recipes", "arguments": {"item_code": code, "language": language_code(lang), "offset": recipes["next_offset"], "limit": limit}})
                    } else {
                        Value::Null
                    };
                    acquisition["recipes"] = recipes;
                }
                if selected("monster_drops") {
                    let mut drops = self.mcp_drops(code, lang, 0, limit)?;
                    drops["next_call"] = if drops["next_offset"].is_number() {
                        json!({"tool": "find_item_drops", "arguments": {"item_code": code, "language": language_code(lang), "offset": drops["next_offset"], "limit": limit}})
                    } else {
                        Value::Null
                    };
                    acquisition["monster_drops"] = drops;
                }
                for source in ["shops", "quest_rewards"] {
                    if selected(source) {
                        acquisition[source] = self.mcp_relations(key, source, lang, 0, limit)?;
                    }
                }
                if selected("production") {
                    acquisition["production"] =
                        self.mcp_entity_sections(key, lang, &["production".into()])?["production"]
                            .clone();
                }
                acquisition["requested_sources"] = json!(if sources.is_empty() {
                    vec![
                        "recipes",
                        "monster_drops",
                        "shops",
                        "production",
                        "quest_rewards",
                    ]
                } else {
                    sources.iter().map(String::as_str).collect()
                });
                result["acquisition"] = acquisition;
            }
            _ => {
                result = self.query_overview(key, lang, limit, &[])?;
                result["acquisition"] = json!({"status": "unsupported_kind",
                    "note": "Structured acquisition is available for heroes and items. These are record details, not an inferred recipe."});
            }
        }
        Ok(result)
    }

    fn query_hero_acquisition(&self, code: &str, lang: &str) -> Value {
        let mut routes = Vec::new();
        if let Some(recipes) = self.index.combos.get(code) {
            for recipe in recipes.iter().filter(|recipe| recipe.result == code) {
                routes.push(json!({"type": "combination", "source": "catalog_job_steps",
                    "from": recipe.parts.iter().map(|part| self.query_hero_ref(part, lang)).collect::<Vec<_>>(),
                    "materials": recipe.materials.iter().map(|(item, qty)| self.query_material(*item, *qty, lang)).collect::<Vec<_>>(),
                    "conditions": text::lead(lang, &recipe.en, &recipe.zh)}));
            }
        }
        if let Some(steps) = self.index.chains.get(code) {
            for pair in steps.windows(2).filter(|pair| pair[1].code == code) {
                let step = &pair[0];
                routes.push(json!({"type": "upgrade", "source": "catalog_job_steps",
                    "from": [self.query_hero_ref(&step.code, lang)],
                    "materials": step.materials.iter().map(|(item, qty)| self.query_material(*item, *qty, lang)).collect::<Vec<_>>(),
                    "conditions": text::lead(lang, &step.en, &step.zh)}));
            }
        }
        // Preserve existing curated beast requirements, including alternatives (quantity 0 is not free).
        if let Some(parts) = hero::fixed_beast(code) {
            let materials: Vec<_> = parts
                .iter()
                .filter(|part| part.item > 0 && part.qty > 0)
                .map(|part| self.query_material(part.item, part.qty, lang))
                .collect();
            let alternatives: Vec<_> = parts.iter().filter(|part| part.item > 0 && part.qty == 0)
                .map(|part| json!({"key": format!("item:{}", part.item), "name": self.item_name(part.item, lang)})).collect();
            let conditions: Vec<_> = parts.iter().filter(|part| part.item == 0)
                .map(|part| json!({"description": crate::db::ui_text(lang, part.zh, part.en), "quantity": part.qty})).collect();
            routes.push(
                json!({"type": "combination", "source": "curated_beast_requirements", "from": [],
                "materials": materials, "alternatives": alternatives, "conditions": conditions,
                "credit_level": hero::fixed_credit(code)}),
            );
        }
        let hero = self.index.heroes.iter().find(|row| row.code == code);
        let hire = hero
            .map(|row| text::lead(lang, &row.hire_en, &row.hire_zh))
            .unwrap_or_default();
        // These supplemental rows already distinguish incoming spirit requirements
        // from next-form navigation; do not flatten next-form materials into this recipe.
        let supplemental: Vec<_> = self
            .hero_data_links(code, lang)
            .into_iter()
            .take_while(|row| row.key != "companion:upgrades")
            .collect();
        json!({"status": if routes.is_empty() && supplemental.is_empty() && hire.is_empty() { "not_recorded" } else { "recorded" },
            "routes": routes, "recruitment": hire,
            "supplemental": supplemental.into_iter().map(hit).collect::<Vec<_>>(),
            "scope": "Direct requirements to obtain this exact form, not the cost of building all prerequisites from scratch. Empty materials or missing values mean not recorded, not free. Conditions are preserved from the offline catalog; routes are not asserted complete."})
    }

    fn query_item_recipes(&self, code: i64, lang: &str, limit: usize) -> Result<Value, String> {
        let ids: Vec<_> = self
            .index
            .recipes
            .iter()
            .filter(|row| row.result_str == code.to_string())
            .map(|row| row.id)
            .collect();
        let mut rows = Vec::new();
        for id in ids.iter().take(limit) {
            let mut row = self.conn.query_row(
                "SELECT source, result_qty, fee, learn_fee, success_rate FROM recipes WHERE id=?1", [id],
                |row| Ok(json!({"key": format!("recipe:{id}"), "recipe_id": id,
                    "source": row.get::<_, String>(0)?, "output_quantity": row.get::<_, i64>(1)?,
                    "fee": row.get::<_, i64>(2)?, "learn_fee": row.get::<_, Option<i64>>(3)?,
                    "success_rate_percent": row.get::<_, Option<i64>>(4)?})),
            ).map_err(|error| error.to_string())?;
            let mut statement = self.conn.prepare("SELECT item_code, qty, enhance FROM recipe_materials WHERE recipe_id=?1 ORDER BY slot").map_err(|error| error.to_string())?;
            let materials = statement
                .query_map([id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            row["materials"] = json!(materials
                .iter()
                .map(|(item, qty, enhance)| {
                    let mut material = self.query_material(*item, *qty, lang);
                    material["enhance"] = json!(enhance);
                    material
                })
                .collect::<Vec<_>>());
            if row["source"] == "reinforce" {
                row["calculable"] = json!(false);
                row["calculation_unavailable_reason"] = json!("Reinforcement output enhancement levels are not recorded; this is not a complete craftable recipe.");
            } else {
                row["calculation_status"] = json!("Use get_recipes to validate calculation support; this summary preserves raw recorded requirements.");
            }
            rows.push(row);
        }
        Ok(
            json!({"results": rows, "total": ids.len(), "next_offset": if ids.len() > limit { Some(limit) } else { None }}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colloquial_hero_question_returns_only_incoming_requirements() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let result = catalog
            .mcp_query("雀天要點合？", &[], None, "auto", "zh-en", 5)
            .unwrap();
        assert_eq!(result["status"], "resolved");
        assert_eq!(result["action"], "acquisition");
        let hero = &result["results"][0];
        assert_eq!(hero["key"], "hero:T49");
        let routes = hero["acquisition"]["routes"].as_array().unwrap();
        let combine = routes
            .iter()
            .find(|route| route["type"] == "combination")
            .unwrap();
        let from: Vec<_> = combine["from"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["key"].as_str().unwrap())
            .collect();
        assert_eq!(from, ["hero:E85", "hero:E75"]);
        assert!(!combine["materials"].as_array().unwrap().is_empty());
        assert!(!routes
            .iter()
            .any(|route| route.to_string().contains("hero:V06")));
    }

    #[test]
    fn names_and_personal_aliases_do_not_silently_pick_a_fuzzy_result() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let missing = catalog
            .mcp_query("never-a-real-name-xyz", &[], Some("hero"), "auto", "en", 5)
            .unwrap();
        assert_eq!(missing["status"], "not_found");
        assert_eq!(missing["results"], json!([]));
        let exact = catalog
            .mcp_query("持國天王", &[], Some("hero"), "overview", "zh", 5)
            .unwrap();
        assert_eq!(exact["results"][0]["key"], "hero:T49");
        catalog
            .index
            .aliases
            .borrow_mut()
            .insert("ssa".into(), ["140".into()].into_iter().collect());
        let custom = catalog
            .mcp_query("SSA", &[], Some("item"), "overview", "en", 5)
            .unwrap();
        assert_eq!(custom["results"][0]["key"], "item:140");
        assert_eq!(custom["matched_count"], 1);
        assert!(custom["results"][0]["description"]
            .as_str()
            .is_some_and(|text| !text.is_empty()));
        let phrase = catalog
            .mcp_query("SSA要點合", &[], Some("item"), "auto", "zh", 5)
            .unwrap();
        assert_eq!(phrase["results"][0]["key"], "item:140");
        let legacy = catalog.mcp_search("item", "SSA要點合", "zh", 0, 5);
        assert_eq!(legacy["results"][0]["key"], "item:140");
        catalog
            .index
            .aliases
            .borrow_mut()
            .insert("火石".into(), ["6213".into()].into_iter().collect());
        let personal = catalog
            .mcp_query("火石要點合", &[], Some("item"), "auto", "zh", 5)
            .unwrap();
        assert_eq!(personal["results"][0]["key"], "item:6213");
        let legacy = catalog.mcp_search("item", "火石要點合", "zh", 0, 5);
        assert_eq!(legacy["results"][0]["key"], "item:6213");
        catalog.index.aliases.borrow_mut().insert(
            "雀天龜天".into(),
            ["140".into(), "141".into()].into_iter().collect(),
        );
        let custom_group = catalog
            .mcp_query("雀天龜天", &[], None, "overview", "zh", 5)
            .unwrap();
        assert_eq!(custom_group["status"], "resolved");
        assert_eq!(custom_group["matched_count"], 2);
        catalog
            .index
            .aliases
            .borrow_mut()
            .insert("corn_test".into(), ["15".into()].into_iter().collect());
        let npc = catalog.mcp_search("npc", "corn_test", "zh", 0, 5);
        assert_eq!(npc["total"], 0, "item:15 alias must not select npc:15");
    }

    #[test]
    fn separate_names_stay_ambiguous_but_a_full_equipment_name_wins_its_span() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let result = catalog
            .mcp_query("雀天同神獸朱雀要點合？", &[], None, "auto", "zh", 5)
            .unwrap();
        assert_eq!(result["status"], "ambiguous");
        assert_eq!(result["results"], json!([]));
        let candidates = result["candidates"].as_array().unwrap();
        for key in ["hero:T49", "hero:E75"] {
            assert!(candidates.iter().any(|row| row["key"] == key), "{result}");
        }
        let item = catalog.index.find_item(6240).unwrap();
        let full_name = catalog
            .mcp_query(&format!("{}要點合？", item.zh), &[], None, "auto", "zh", 5)
            .unwrap();
        assert_eq!(full_name["results"][0]["key"], "item:6240", "{full_name}");
        let unknown = catalog
            .mcp_query("高階雀天", &[], None, "auto", "zh", 5)
            .unwrap();
        assert_eq!(unknown["status"], "not_found");
    }

    #[test]
    fn batch_order_and_incomplete_acquisition_data_remain_explicit() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let result = catalog
            .mcp_query(
                "",
                &["monster:M25".into(), "monster:M24".into()],
                None,
                "compare",
                "en",
                1,
            )
            .unwrap();
        assert_eq!(result["results"][0]["key"], "monster:M25");
        assert_eq!(result["results"][1]["key"], "monster:M24");
        assert_eq!(result["truncated"], false);
        let spirit = catalog.query_hero_acquisition("F16", "en");
        assert!(!spirit["supplemental"].to_string().contains("hero:F19"));
        let recipes = catalog.query_item_recipes(5543, "en", 50).unwrap();
        let reinforced = recipes["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["recipe_id"] == 1053)
            .unwrap();
        assert_eq!(reinforced["calculable"], false);
        assert!(reinforced["calculation_unavailable_reason"]
            .as_str()
            .unwrap()
            .contains("enhancement"));
    }
}
