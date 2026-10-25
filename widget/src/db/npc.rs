use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::{Catalog, Hit};
use super::names::{fuzzy_rows, haystack, plain_name};
use super::rows::{Index, NpcRow};
use super::store::{online, table_exists};
use super::language::primary_lang;
use super::text::{caption, chrome_text, commas, lead3, phrase, show3, stack3, trail3};

pub(in crate::db) fn npc_sheet_parts(npc: &NpcRow, lang: &str) -> (String, String, String, Vec<(String, String)>, String) {
    let (title, alt) = npc_names(lang, npc);
    let map = show3(lang, &npc.map_en, &npc.map_zh, &npc.map_ko);
    let mut facts = Vec::new();
    let nation = show3(lang, &npc.nation_en, &npc.nation_zh, &chrome_text("ko", &npc.nation_zh, &npc.nation_en));
    if !nation.is_empty() {
        facts.push((phrase("nation", lang), nation));
    }
    if !map.is_empty() {
        facts.push((phrase("map", lang), map.clone()));
    }
    facts.push((phrase("coord", lang), format!("{}, {}", commas(npc.x), commas(npc.y))));
    (title, alt, map, facts, stack3(lang, &npc.place_en, &npc.place_zh, ""))
}

pub(in crate::db) fn npc_names(lang: &str, npc: &NpcRow) -> (String, String) {
    let title = lead3(lang, &npc.en, &npc.zh, &npc.ko);
    let alt = trail3(lang, &npc.en, &npc.zh, &npc.ko);
    if title.is_empty() {
        let ko = plain_name(&npc.ko);
        if ko.is_empty() { (npc.code.to_string(), String::new()) } else { (ko, String::new()) }
    } else {
        (title, alt)
    }
}

pub(in crate::db) fn load_npcs(
    conn: &Connection,
    client_root: &Path,
    texts: &HashMap<String, String>,
) -> Result<Vec<NpcRow>, String> {
    if !table_exists(conn, "npcs") {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT n.code, n.nation, n.name_ko, n.name_gts, n.map_index, n.map_x, n.map_y, n.portrait, n.place_gts,
                    IFNULL(m.name_ko, ''), IFNULL(m.name_gts, 0)
             FROM npcs n
             LEFT JOIN maps m ON m.code = n.map_index
             ORDER BY n.code",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, i64>(10)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut npcs = Vec::new();
    for row in rows {
        let (code, nation, ko, name_gts, _map, x, y, portrait, place_gts, map_ko, map_gts) =
            row.map_err(|err| err.to_string())?;
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        let map_en = online(texts, map_gts, "en").to_string();
        let map_zh = online(texts, map_gts, "zh").to_string();
        let place_en = online(texts, place_gts, "en").to_string();
        let place_zh = online(texts, place_gts, "zh").to_string();
        let (nation_en, nation_zh) = npc_nation(&nation);
        let names = haystack(&[
            &ko,
            &plain_name(&ko),
            &en,
            &zh,
            &map_ko,
            &map_en,
            &map_zh,
            &place_en,
            &place_zh,
            nation_en,
            nation_zh,
        ]);
        npcs.push(NpcRow {
            code,
            ko,
            en,
            zh,
            nation_en: nation_en.to_string(),
            nation_zh: nation_zh.to_string(),
            map_ko,
            map_en,
            map_zh,
            map_index: _map,
            x,
            y,
            place_en,
            place_zh,
            names,
            icon: npc_portrait(client_root, portrait),
        });
    }
    npcs.sort_by(|a, b| a.zh.cmp(&b.zh).then(a.en.cmp(&b.en)).then(a.code.cmp(&b.code)));
    Ok(npcs)
}

pub(in crate::db) fn npc_nation(code: &str) -> (&'static str, &'static str) {
    match code {
        "K" => ("Joseon", "朝鮮"),
        "J" => ("Japan", "日本"),
        "C" => ("China", "中國"),
        "T" => ("Taiwan", "台灣"),
        "I" => ("India", "印度"),
        "A" => ("All", "全區"),
        _ => ("", ""),
    }
}

pub(in crate::db) fn npc_portrait(client_root: &Path, index: i64) -> Option<(PathBuf, i32)> {
    if index <= 0 {
        return None;
    }
    let relative = match index / 300 {
        0 => "Assets/Textures/UI/Portraits/portrait_npc.AGF",
        1 => "Assets/Textures/UI/Portraits/portrait_npc2.AGF",
        _ => return None,
    };
    let path = client_root.join(relative);
    path.is_file().then_some((path, (index % 300) as i32))
}

impl Catalog {
    pub fn npc_sheet(&self, code: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String)>, String)> {
        let npc = self.index.npcs.iter().find(|npc| npc.code == code)?;
        let (title, alt, map, mut facts, note) = npc_sheet_parts(npc, lang);
        facts.extend(self.enchant_npc_lines(code, lang));
        Some((title, alt, map, facts, note))
    }

    fn enchant_npc_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        if !self.has_table("enchant_npcs") {
            return Vec::new();
        }
        let Ok((fortune, success, fees, share)) = self.conn.query_row(
            "SELECT n.fortune_fee, n.success_rate, n.fee_1, n.fee_2, n.fee_3, n.fee_4, n.fee_5, n.fee_6, n.fee_7,
                    IFNULL(b.share_rate, -1)
             FROM enchant_npcs n
             LEFT JOIN enchant_npc_bonus b ON b.param_code = n.param_code
             WHERE n.npc_code = ?1",
            [code],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    [
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                    ],
                    row.get::<_, i64>(9)?,
                ))
            },
        ) else {
            return Vec::new();
        };
        let mut lines = vec![(phrase("fortune-fee", lang), commas(fortune))];
        for (grade, fee) in fees.into_iter().enumerate() {
            let rank = match primary_lang(lang) {
                "en" => format!("Grade {}", grade + 1),
                "ko" => format!("{}등급", grade + 1),
                _ => format!("{}級", grade + 1),
            };
            lines.push((phrase("enchant-fee", lang), format!("{rank}  {}", commas(fee))));
        }
        lines.push((phrase("enchant-weight", lang), commas(success)));
        if share >= 0 {
            lines.push((phrase("fortune-share", lang), commas(share)));
        }
        lines
    }

    pub(in crate::db) fn open_npc(&self, code: i64, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let Some(npc) = self.index.npcs.iter().find(|npc| npc.code == code) else {
            return (phrase("missing-npc", lang), Vec::new(), None);
        };
        let (name, alt, map, facts, note) = self.npc_sheet(code, lang).unwrap_or_else(|| npc_sheet_parts(npc, lang));
        let mut text = name.clone();
        if !alt.is_empty() {
            text.push_str(&format!("\n{alt}"));
        }
        if !map.is_empty() {
            text.push_str(&format!("\n{map}"));
        }
        for (label, value) in &facts {
            text.push_str(&format!("\n{label}  {value}"));
        }
        if !note.is_empty() {
            text.push_str(&format!("\n{note}"));
        }
        (text, self.quests_for_npc(code, lang), npc.icon.clone())
    }
}

impl Index {
    pub(in crate::db) fn npc(&self, code: i64) -> Option<&NpcRow> {
        if code <= 0 {
            return None;
        }
        self.npcs.iter().find(|npc| npc.code == code)
    }

    pub(in crate::db) fn npc_face(&self, code: i64) -> Option<(PathBuf, i32)> {
        self.npc(code).and_then(|npc| npc.icon.clone())
    }

    pub(in crate::db) fn npc_hits(&self, query: &str, folded: &str, lang: &str, limit: usize) -> Vec<Hit> {
        let mut picked = Vec::new();
        for npc in &self.npcs {
            if !self.name_hit(query, folded, &npc.code.to_string(), &npc.names) {
                continue;
            }
            picked.push(npc);
            if picked.len() == limit {
                break;
            }
        }
        if picked.len() < limit {
            picked.extend(fuzzy_rows(&self.npcs, folded, limit - picked.len(), |npc| {
                !self.name_hit(query, folded, &npc.code.to_string(), &npc.names)
            }, |npc| &npc.names));
        }
        picked
            .into_iter()
            .map(|npc| {
                let (title, alt) = npc_names(lang, npc);
                Hit {
                    title,
                    subtitle: caption(&alt, show3(lang, &npc.map_en, &npc.map_zh, &npc.map_ko)),
                    key: format!("npc:{}", npc.code),
                    icon: npc.icon.clone(),
                    solo: false,
                    aside: lead3(lang, &npc.nation_en, &npc.nation_zh, &chrome_text("ko", &npc.nation_zh, &npc.nation_en)),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }
}
