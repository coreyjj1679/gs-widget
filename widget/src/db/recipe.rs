use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::Connection;

use super::item::pref_rank;
use super::{Catalog, Hit};
use super::names::haystack;
use super::rows::RecipeRow;
use super::store::online;
use super::text::{caption, chrome_text, commas, label, lead3, phrase, show3, trail3};

pub(in crate::db) const PREF: &[&str] = &[
    "alchemy",
    "blacksmith",
    "jeweler",
    "unified",
    "formation",
    "baekje",
    "npc",
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

impl Catalog {
    pub(in crate::db) fn recipes_using(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut stmt = match self.conn.prepare(
            "SELECT r.id, r.source, r.result_item, m.qty
             FROM recipe_materials m
             JOIN recipes r ON r.id = m.recipe_id
             WHERE m.item_code = ?1
             ORDER BY r.id",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([code], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
        })
        .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|(id, source, result, qty)| Hit {
            title: self.item_name(result, lang),
            subtitle: caption(&label(&source, lang), format!("× {}", commas(qty))),
            key: format!("recipe:{id}"),
            icon: self.item_icon(result),
            solo: false,
            aside: phrase("use-craft", lang),
            badge: None,
            chip: String::new(),
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
        let name = self.item_name(result, lang);
        let mut text = format!(
            "{name}\n{}  {} {}\n{} {}",
            label(&source, lang),
            phrase("qty", lang),
            commas(qty),
            phrase("fee", lang),
            commas(fee)
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
            subtitle: caption(&chrome_text(lang, "成品", "Result"), format!("× {}", commas(qty))),
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
            (phrase("fee", lang), commas(fee)),
        ];
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
            let name = self.item_name(code, lang);
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
        out
    }

    pub(in crate::db) fn recipe_material_hits(&self, recipe_id: i64, lang: &str) -> Vec<Hit> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_code, qty, enhance FROM recipe_materials WHERE recipe_id = ?1 ORDER BY slot")
            .expect("link materials");
        stmt.query_map([recipe_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        })
        .expect("link rows")
        .filter_map(Result::ok)
        .map(|(code, qty, enhance)| {
            let child = self.preferred_recipe(code, Some(recipe_id));
            let mut subtitle = format!("× {}", commas(qty));
            if enhance > 0 {
                subtitle.push_str(&format!("  +{}", commas(enhance)));
            }
            if let Some(child) = child {
                subtitle.push_str(&format!("  {}", label(&self.recipe_source(child), lang)));
            }
            Hit {
                title: self.item_name(code, lang),
                subtitle,
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
        .collect()
    }

    pub(in crate::db) fn recipes_for(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source FROM recipes WHERE result_item = ?1")
            .expect("makers");
        let rows: Vec<(i64, String)> = stmt
            .query_map([code], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("maker rows")
            .filter_map(Result::ok)
            .collect();
        let mut best: HashMap<Vec<(i64, i64)>, (i64, String, usize)> = HashMap::new();
        for (id, source) in rows {
            let mut signature = self.recipe_signature(id);
            if signature.is_empty() {
                signature.push((-id, 0));
            }
            let rank = pref_rank(&source);
            match best.get(&signature) {
                Some((_, _, old)) if *old <= rank => {}
                _ => {
                    best.insert(signature, (id, source, rank));
                }
            }
        }
        let mut kept: Vec<(i64, String, usize)> = best.into_values().collect();
        kept.sort_by_key(|(_, _, rank)| *rank);
        kept.into_iter()
            .take(12)
            .map(|(id, source, _)| Hit {
                title: label(&source, lang),
                subtitle: {
                    let materials = self.recipe_material_summary(id, lang);
                    if materials.is_empty() { phrase("open-recipe", lang) } else { materials }
                },
                key: format!("recipe:{id}"),
                icon: self.item_icon(code),
                    solo: false,
                    aside: String::new(),
                badge: None,
                    chip: String::new(),
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
