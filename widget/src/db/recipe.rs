use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::Connection;

use super::item::pref_rank;
use super::language::{primary_lang, secondary_lang};
use super::{Catalog, Hit};
use super::names::haystack;
use super::rows::{Index, RecipeMaterialChoice, RecipeRow};
use super::store::{online, table_exists};
use super::text::{caption, chrome_text, commas, label, lead3, phrase, show3, trail3};

pub(in crate::db) const PREF: &[&str] = &[
    "alchemy",
    "blacksmith",
    "jeweler",
    "unified",
    "formation",
    "baekje",
    "npc",
    "divine_weapon",
    "facility",
    "reinforce",
];

pub(in crate::db) fn formula_lands(code: i64, lang: &str) -> String {
    if code <= 0 {
        return String::new();
    }
    let flags = [(1, "land-k"), (10, "land-j"), (100, "land-c"), (1000, "land-tw")];
    let present = flags
        .into_iter()
        .filter(|(unit, _)| (code / unit) % 10 == 1)
        .map(|(_, key)| phrase(key, lang))
        .collect::<Vec<_>>();
    if present.len() >= 3 {
        return phrase("all-regions", lang);
    }
    present.join(" · ")
}

pub(in crate::db) fn load_recipes(
    conn: &Connection,
    texts: &HashMap<String, String>,
    icons: &HashMap<i64, (PathBuf, i32)>,
) -> Result<Vec<RecipeRow>, String> {
    let mut choices = HashMap::new();
    if table_exists(conn, "recipe_material_choices") {
        let mut stmt = conn.prepare(
            "SELECT recipe_id, choice_count, item_code, qty FROM recipe_material_choices ORDER BY recipe_id, item_code",
        ).map_err(|err| err.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        }).map_err(|err| err.to_string())?;
        for row in rows {
            let (id, choose_count, code, qty) = row.map_err(|err| err.to_string())?;
            choices.entry(id).or_insert_with(|| RecipeMaterialChoice { choose_count, options: Vec::new() })
                .options.push((code, qty));
        }
    }
    let mut stmt = conn
        .prepare(
            "SELECT r.id, r.source, r.result_item, i.name_ko, i.name_gts, r.level_min
             FROM recipes r
             JOIN items i ON i.code = r.result_item
             ORDER BY r.id",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut recipes = Vec::new();
    for row in rows {
        let (id, source, result, ko, name_gts, level_min) = row.map_err(|err| err.to_string())?;
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        recipes.push(RecipeRow {
            names: haystack(&[&ko, &en, &zh]),
            id,
            source,
            material_choice: choices.remove(&id),
            result_str: result.to_string(),
            ko,
            en,
            zh,
            level_min,
            icon: icons.get(&result).cloned(),
        });
    }
    Ok(recipes)
}

impl Index {
    pub(in crate::db) fn material_choice(&self, id: i64) -> Option<&RecipeMaterialChoice> {
        let index = self.recipes.binary_search_by_key(&id, |recipe| recipe.id).ok()?;
        self.recipes[index].material_choice.as_ref()
    }

    fn choice_names(&self, choice: &RecipeMaterialChoice, lang: &str) -> String {
        let separator = if primary_lang(lang) == "zh" { "、" } else { ", " };
        choice.options.iter().filter_map(|(code, _)| self.find_item(*code))
            .map(|item| lead3(lang, &item.en, &item.zh, &item.ko))
            .collect::<Vec<_>>().join(separator)
    }
}

impl RecipeMaterialChoice {
    pub(in crate::db) fn rule(&self, lang: &str) -> String {
        let rule = chrome_text(lang, "{total} 款幻獸武器任選 {count} 款", "Choose any {count} of the {total} fey weapons")
            .replace("{total}", &self.options.len().to_string()).replace("{count}", &self.choose_count.to_string());
        if let Some((_, qty)) = self.options.first().filter(|(_, qty)| self.options.iter().all(|(_, n)| n == qty)) {
            let each = chrome_text(lang, "，各 {qty} 件", ", {qty} of each").replace("{qty}", &commas(*qty));
            format!("{rule}{each}")
        } else {
            rule
        }
    }
}

fn recipe_fee_text(fee: i64, lang: &str) -> String {
    if fee < 0 { chrome_text(lang, "未記錄", "Not recorded") } else { commas(fee) }
}

impl Catalog {
    /// Display names for related item rows, keeping metadata separate from the
    /// selected secondary name and preserving the existing unnamed-item fallback.
    pub(in crate::db) fn related_item_names(&self, code: i64, lang: &str) -> (String, String) {
        let (en, zh, ko) = self.item_names3(code);
        let title = lead3(lang, &en, &zh, &ko);
        // Linked items can synthesize a translated prefix + base name even when
        // their own GTS names are empty. Keep that fallback unless Korean was
        // explicitly selected, in which case the recorded Korean name is useful.
        let linked_fallback = en.trim().is_empty() && zh.trim().is_empty()
            && primary_lang(lang) != "ko" && secondary_lang(lang) != Some("ko");
        let title = if title.is_empty() || linked_fallback { self.item_name(code, lang) } else { title };
        (title, trail3(lang, &en, &zh, &ko))
    }

    pub(in crate::db) fn recipes_using(&self, code: i64, lang: &str) -> Vec<Hit> {
        let candidates = if table_exists(&self.conn, "recipe_material_choices") {
            "UNION ALL SELECT recipe_id, qty, 1 FROM recipe_material_choices WHERE item_code = ?1"
        } else {
            ""
        };
        let sql = format!(
            "SELECT r.id, r.source, r.result_item, m.qty, m.is_choice
             FROM (SELECT recipe_id, qty, 0 AS is_choice FROM recipe_materials WHERE item_code = ?1 {candidates}) m
             JOIN recipes r ON r.id = m.recipe_id
             ORDER BY r.id",
        );
        let mut stmt = match self.conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([code], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, bool>(4)?))
        })
        .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|(id, source, result, qty, is_choice)| {
            let (title, alternate) = self.related_item_names(result, lang);
            let detail = if is_choice {
                self.index.material_choice(id).map(|group| group.rule(lang)).unwrap_or_default()
            } else { format!("× {}", commas(qty)) };
            Hit {
                title,
                subtitle: caption(&alternate, caption(&label(&source, lang), detail)),
                key: format!("recipe:{id}"),
                icon: self.item_icon(result),
                solo: false,
                aside: phrase("use-craft", lang),
                badge: None,
                chip: String::new(),
            }
        })
        .collect()
    }

    pub(in crate::db) fn open_recipe(&self, id: i64, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let row = self.conn.query_row(
            "SELECT source, result_item, result_qty, fee, learn_fee, npc FROM recipes WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        );
        let Ok((source, result, qty, fee, learn, npc)) = row else {
            return (phrase("missing-recipe", lang), Vec::new(), None);
        };
        let (name, alternate) = self.related_item_names(result, lang);
        let heading = if alternate.is_empty() { name.clone() } else { format!("{name}\n{alternate}") };
        let mut text = format!(
            "{heading}\n{}  {} {}\n{} {}",
            label(&source, lang),
            phrase("qty", lang),
            commas(qty),
            phrase("fee", lang),
            recipe_fee_text(fee, lang)
        );
        if let Some(learn) = learn.filter(|value| *value > 0) {
            text.push_str(&format!("\n{} {}", phrase("learn", lang), commas(learn)));
        }
        for (label, value) in self.recipe_npc_facts(&npc, lang) {
            text.push_str(&format!("\n{label} {value}"));
        }
        for (label, value) in self.facility_recipe_facts(id, lang) {
            text.push_str(&format!("\n{label} {value}"));
        }
        text.push_str(&format!("\n\n{}\n", phrase("materials", lang)));
        text.push_str(&self.material_tree(id, 3, &mut vec![id], lang));
        let mut links = vec![Hit {
            title: name,
            subtitle: caption(&alternate, caption(&chrome_text(lang, "成品", "Result"), format!("× {}", commas(qty)))),
            key: format!("item:{result}"),
            icon: self.item_icon(result),
            solo: false,
            aside: chrome_text(lang, "查看物品", "View item"),
            badge: None,
            chip: String::new(),
        }];
        links.extend(self.recipe_material_hits(id, lang));
        let icon = self.item_icon(result);
        (text, links, icon)
    }

    pub fn recipe_sheet(&self, id: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String)>)> {
        let row = self.conn.query_row(
            "SELECT source, result_item, result_qty, fee, learn_fee, level_min, level_max, success_rate, npc
             FROM recipes WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        );
        let Ok((source, result, qty, fee, learn, level_min, _level_max, success, npc)) = row else {
            return None;
        };
        let mut lines = vec![
            (phrase("kind", lang), label(&source, lang)),
            (phrase("code", lang), result.to_string()),
            (phrase("qty", lang), commas(qty)),
            (phrase("fee", lang), recipe_fee_text(fee, lang)),
        ];
        if let Some(group) = self.index.material_choice(id) {
            lines.push((phrase("materials", lang), group.rule(lang)));
        }
        if let Some(learn) = learn.filter(|value| *value > 0) {
            lines.push((phrase("learn", lang), commas(learn)));
        }
        if let Some(level) = level_min.filter(|value| *value > 0) {
            lines.push((phrase("min-level", lang), commas(level)));
        }
        if let Some(rate) = success.filter(|value| *value > 0) {
            lines.push((phrase("success", lang), format!("{}%", commas(rate))));
        }
        lines.extend(self.recipe_npc_facts(&npc, lang));
        lines.extend(self.facility_recipe_facts(id, lang));
        let (en, zh, ko) = self.item_names3(result);
        Some((lead3(lang, &en, &zh, &ko), trail3(lang, &en, &zh, &ko), format!("#{result}"), lines))
    }

    fn recipe_npc_facts(&self, recorded: &str, lang: &str) -> Vec<(String, String)> {
        let mut facts = Vec::new();
        for npc in recorded.split(',')
            .filter_map(|value| value.trim().parse::<i64>().ok())
            .filter(|code| *code > 0)
            .filter_map(|code| self.index.npc(code))
        {
            let (name, alternate) = super::npc::npc_names(lang, npc);
            let name = if alternate.is_empty() { name } else { format!("{name} / {alternate}") };
            facts.push((phrase("npc", lang), name));
            let place = show3(lang, &npc.place_en, &npc.place_zh, "");
            let location = if place.is_empty() {
                show3(lang, &npc.map_en, &npc.map_zh, &npc.map_ko)
            } else {
                place
            };
            if !location.is_empty() {
                facts.push((chrome_text(lang, "所在地", "Location"), location));
            }
        }
        facts
    }

    fn recipe_material_summary(&self, id: i64, lang: &str) -> String {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT item_code, qty, enhance FROM recipe_materials WHERE recipe_id = ?1 ORDER BY slot",
        ) else { return String::new() };
        let materials = stmt.query_map([id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        }).map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>()).unwrap_or_default();
        // Keep the complete list for the existing hover/keyboard information panel;
        // the compact row elides this text to fit its available width.
        materials.into_iter().map(|(code, qty, enhance)| {
            let (en, zh, ko) = self.item_names3(code);
            let mut name = show3(lang, &en, &zh, &ko);
            if name.is_empty() { name = self.item_name(code, lang); }
            if enhance > 0 { name.push_str(&format!(" +{}", commas(enhance))); }
            format!("{name} × {}", commas(qty))
        }).collect::<Vec<_>>().join(" · ")
    }

    pub(in crate::db) fn material_tree(&self, recipe_id: i64, depth: usize, seen: &mut Vec<i64>, lang: &str) -> String {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT item_code, qty FROM recipe_materials WHERE recipe_id = ?1 ORDER BY slot",
            )
            .expect("materials");
        let rows: Vec<(i64, i64)> = stmt
            .query_map([recipe_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("material rows")
            .filter_map(Result::ok)
            .collect();
        let mut out = String::new();
        let indent = "  ".repeat(3 - depth.min(3));
        for (code, qty) in rows {
            let (name, alternate) = self.related_item_names(code, lang);
            let name = if alternate.is_empty() { name } else { format!("{name} · {alternate}") };
            out.push_str(&format!("{indent}{name} × {}\n", commas(qty)));
            if depth > 0 {
                if let Some(child) = self.preferred_recipe(code, Some(recipe_id)) {
                    if !seen.contains(&child) {
                        seen.push(child);
                        out.push_str(&self.material_tree(child, depth - 1, seen, lang));
                    }
                }
            }
        }
        if let Some(group) = self.index.material_choice(recipe_id) {
            out.push_str(&format!("{indent}{}\n{indent}  {}\n", group.rule(lang), self.index.choice_names(group, lang)));
        }
        out
    }

    pub(in crate::db) fn recipe_material_hits(&self, recipe_id: i64, lang: &str) -> Vec<Hit> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_code, qty, enhance FROM recipe_materials WHERE recipe_id = ?1 ORDER BY slot")
            .expect("link materials");
        let mut hits: Vec<_> = stmt.query_map([recipe_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        })
        .expect("link rows")
        .filter_map(Result::ok)
        .map(|(code, qty, enhance)| {
            let (title, alternate) = self.related_item_names(code, lang);
            let child = self.preferred_recipe(code, Some(recipe_id));
            let mut subtitle = format!("× {}", commas(qty));
            if enhance > 0 {
                subtitle.push_str(&format!("  +{}", commas(enhance)));
            }
            if let Some(child) = child {
                subtitle.push_str(&format!("  {}", label(&self.recipe_source(child), lang)));
            }
            Hit {
                title,
                subtitle: caption(&alternate, subtitle),
                key: match child {
                    Some(id) => format!("recipe:{id}"),
                    None => format!("item:{code}"),
                },
                icon: self.item_icon(code),
                solo: false,
                aside: String::new(),
                badge: None,
                    chip: String::new(),
            }
        })
        .collect();
        if let Some(group) = self.index.material_choice(recipe_id) {
            let option = chrome_text(lang, "可選武器", "Weapon option");
            for &(code, qty) in &group.options {
                let (title, alternate) = self.related_item_names(code, lang);
                hits.push(Hit {
                    title, subtitle: caption(&alternate, format!("{option} · × {}", commas(qty))),
                    key: format!("item:{code}"), icon: self.item_icon(code), solo: false,
                    aside: String::new(), badge: None, chip: String::new(),
                });
            }
        }
        hits
    }

    pub(in crate::db) fn recipes_for(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source, npc FROM recipes WHERE result_item = ?1")
            .expect("makers");
        let rows: Vec<(i64, String, String)> = stmt
            .query_map([code], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("maker rows")
            .filter_map(Result::ok)
            .collect();
        let mut best = HashMap::new();
        for (id, source, npc) in rows {
            let mut signature = self.recipe_signature(id);
            if signature.is_empty() {
                signature.push((-id, 0));
            }
            let signature = (signature, self.index.material_choice(id));
            let rank = pref_rank(&source);
            match best.get(&signature) {
                Some((_, _, _, old)) if *old <= rank => {}
                _ => {
                    best.insert(signature, (id, source, npc, rank));
                }
            }
        }
        let mut kept: Vec<_> = best.into_values().collect();
        kept.sort_by_key(|(id, _, _, rank)| (*rank, *id));
        kept.into_iter()
            .take(12)
            .map(|(id, source, npc, _)| {
                let materials = self.recipe_material_summary(id, lang);
                let (title, subtitle) = if let Some(group) = self.index.material_choice(id) {
                    let npc = npc.parse::<i64>().ok().and_then(|code| self.index.npc(code))
                        .map(|npc| super::npc::npc_names(lang, npc).0).unwrap_or_default();
                    (group.rule(lang), caption(&npc, caption(&materials, self.index.choice_names(group, lang))))
                } else {
                    (label(&source, lang), if materials.is_empty() { phrase("open-recipe", lang) } else { materials })
                };
                Hit {
                    title, subtitle,
                    key: format!("recipe:{id}"),
                    icon: self.item_icon(code),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn recipe_signature(&self, id: i64) -> Vec<(i64, i64)> {
        let mut stmt = match self.conn.prepare(
            "SELECT item_code, qty FROM recipe_materials WHERE recipe_id = ?1 ORDER BY item_code, qty",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    pub(in crate::db) fn preferred_recipe(&self, code: i64, skip: Option<i64>) -> Option<i64> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source FROM recipes WHERE result_item = ?1")
            .ok()?;
        let mut rows: Vec<(i64, String)> = stmt
            .query_map([code], |row| Ok((row.get(0)?, row.get(1)?)))
            .ok()?
            .filter_map(Result::ok)
            .filter(|(id, _)| Some(*id) != skip)
            .collect();
        rows.sort_by_key(|(_, source)| pref_rank(source));
        rows.into_iter().next().map(|(id, _)| id)
    }

    pub(in crate::db) fn recipe_source(&self, id: i64) -> String {
        self.conn
            .query_row("SELECT source FROM recipes WHERE id = ?1", [id], |row| row.get(0))
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "tests/related_bilingual.rs"]
mod related_bilingual_tests;
