use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::{Catalog, Hit};
use super::names::haystack;
use super::quest::quest_source_rank;
use super::rows::{MonsterRow, PlaceRow};
use super::store::online;
use super::text::{caption, commas, commas_text, element_file, element_name, lead, phrase, pick, pick_place, trail};

pub(in crate::db) fn load_locations(conn: &Connection) -> Result<HashMap<String, Vec<i64>>, String> {
    let mut stmt = conn
        .prepare("SELECT monster_code, location_gts FROM monster_locations ORDER BY monster_code, location_gts")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|err| err.to_string())?;
    let mut locations = HashMap::new();
    for row in rows {
        let (code, gts) = row.map_err(|err| err.to_string())?;
        locations.entry(code).or_insert_with(Vec::new).push(gts);
    }
    Ok(locations)
}

pub(in crate::db) fn load_monsters(
    conn: &Connection,
    client_root: &Path,
    pictures: &HashMap<String, (PathBuf, i32)>,
    texts: &HashMap<String, String>,
    locations: &HashMap<String, Vec<i64>>,
) -> Result<Vec<MonsterRow>, String> {
    let mut stmt = conn
        .prepare("SELECT code, nation, name_ko, name_gts, element FROM monsters ORDER BY code")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut monsters = Vec::new();
    for row in rows {
        let (code, nation, ko, name_gts, element) = row.map_err(|err| err.to_string())?;
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        let places = locations.get(&code).cloned().unwrap_or_default();
        let mut place_en = Vec::new();
        let mut place_zh = Vec::new();
        for gts in &places {
            let en_name = online(texts, *gts, "en");
            let zh_name = online(texts, *gts, "zh");
            if !en_name.is_empty() {
                place_en.push(en_name);
            }
            if !zh_name.is_empty() {
                place_zh.push(zh_name);
            }
        }
        let badge = element_file(element)
            .map(|relative| client_root.join(relative))
            .filter(|path| path.is_file());
        monsters.push(MonsterRow {
            names: haystack(&[&ko, &en, &zh]),
            code: code.clone(),
            nation,
            en,
            zh,
            place_en: place_en.join(" | "),
            place_zh: place_zh.join(" | "),
            places,
            icon: pictures.get(&code).cloned(),
            badge,
        });
    }
    Ok(monsters)
}

pub(in crate::db) fn nation_counts(monsters: &[MonsterRow]) -> Vec<(i64, i64)> {
    let mut counts = HashMap::<i64, i64>::new();
    for monster in monsters {
        *counts.entry(monster.nation).or_default() += 1;
    }
    let mut nations: Vec<(i64, i64)> = counts.into_iter().collect();
    nations.sort_by_key(|(nation, _)| *nation);
    nations
}

pub(in crate::db) fn load_places(conn: &Connection, texts: &HashMap<String, String>) -> Result<Vec<PlaceRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT m.nation, ml.location_gts, m.code
             FROM monsters m
             JOIN monster_locations ml ON ml.monster_code = m.code",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)))
        .map_err(|err| err.to_string())?;
    let mut grouped: HashMap<(i64, i64), HashSet<String>> = HashMap::new();
    for row in rows {
        let (nation, gts, code) = row.map_err(|err| err.to_string())?;
        grouped.entry((nation, gts)).or_default().insert(code);
    }
    let mut places = Vec::new();
    for ((nation, gts), monsters) in grouped {
        let en = online(texts, gts, "en").to_string();
        let zh = online(texts, gts, "zh").to_string();
        let sort = if zh.is_empty() { en.clone() } else { zh.clone() };
        places.push(PlaceRow {
            nation,
            gts,
            names: haystack(&[&en, &zh]),
            count: monsters.len() as i64,
            en,
            zh,
            sort,
        });
    }
    Ok(places)
}

impl Catalog {
    pub(in crate::db) fn monster_text(&self, code: &str, lang: &str) -> String {
        let row = self.conn.query_row(
            "SELECT m.code, m.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''),
                    m.hp, m.mp, m.hit_resist, m.magic_resist, m.atk_min, m.atk_max, m.exp,
                    m.element, m.element_gts, m.is_boss, m.unit_type
             FROM monsters m
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND m.name_gts > 0 AND se.id = printf('%08d', m.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND m.name_gts > 0 AND sz.id = printf('%08d', m.name_gts)
             WHERE m.code = ?1",
            [code],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    pick(lang, &row.get::<_, String>(1)?, &row.get::<_, String>(2)?, &row.get::<_, String>(3)?),
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, i64>(14)?,
                ))
            },
        );
        let Ok((id, name, hp, mp, hit, magic, atk_min, atk_max, exp, element, ela, boss, unit)) = row else {
            return phrase("missing-monster", lang);
        };
        let lines = self.monster_lines(code, lang, hp, mp, hit, magic, atk_min, atk_max, exp, element, ela, boss, unit);
        let mut text = format!("{name}\n{id}");
        for (label, value) in lines {
            if label.is_empty() {
                text.push_str(&format!("\n{value}"));
            } else {
                text.push_str(&format!("\n{label}  {value}"));
            }
        }
        text
    }

    pub fn monster_sheet(&self, code: &str, lang: &str) -> Option<(String, String, String, Vec<(String, String)>)> {
        let row = self.conn.query_row(
            "SELECT m.code, m.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''),
                    m.hp, m.mp, m.hit_resist, m.magic_resist, m.atk_min, m.atk_max, m.exp,
                    m.element, m.element_gts, m.is_boss, m.unit_type
             FROM monsters m
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND m.name_gts > 0 AND se.id = printf('%08d', m.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND m.name_gts > 0 AND sz.id = printf('%08d', m.name_gts)
             WHERE m.code = ?1",
            [code],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, i64>(14)?,
                ))
            },
        );
        let Ok((id, en, zh, hp, mp, hit, magic, atk_min, atk_max, exp, element, ela, boss, unit)) = row else {
            return None;
        };
        Some((
            lead(lang, &en, &zh),
            trail(lang, &en, &zh),
            id.clone(),
            self.monster_lines(&id, lang, hp, mp, hit, magic, atk_min, atk_max, exp, element, ela, boss, unit),
        ))
    }

    pub(in crate::db) fn monster_lines(
        &self,
        code: &str,
        lang: &str,
        hp: i64,
        mp: i64,
        hit: i64,
        magic: i64,
        atk_min: i64,
        atk_max: i64,
        exp: i64,
        element: i64,
        ela: i64,
        boss: i64,
        unit: i64,
    ) -> Vec<(String, String)> {
        let kind = if boss == 1 { phrase("boss", lang) } else { phrase("normal", lang) };
        let unit_name = if unit == 1 { phrase("air", lang) } else { phrase("ground", lang) };
        let mut lines = vec![
            (phrase("kind", lang), kind),
            (phrase("unit", lang), unit_name),
            (phrase("element", lang), element_name(element, lang)),
        ];
        if ela > 0 {
            lines.push((phrase("ela", lang), commas(ela)));
        }
        lines.extend([
            (phrase("hp", lang), commas(hp)),
            (phrase("mp", lang), commas(mp)),
            (phrase("hit-resist", lang), commas(hit)),
            (phrase("magic-resist", lang), commas(magic)),
            (phrase("atk", lang), format!("{}-{}", commas(atk_min), commas(atk_max))),
            (phrase("exp", lang), commas(exp)),
        ]);
        let places = self.place_names(code, lang);
        if places.is_empty() {
            lines.push((phrase("spawn-place", lang), "—".into()));
        } else {
            for (index, place) in places.into_iter().enumerate() {
                let label = if index == 0 { phrase("spawn-place", lang) } else { String::new() };
                lines.push((label, place));
            }
        }
        lines
    }

    pub(in crate::db) fn place_names(&self, code: &str, lang: &str) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT IFNULL(lz.text, ''), IFNULL(le.text, '')
                 FROM monster_locations ml
                 LEFT JOIN strings lz ON lz.object = 'OnlineText.dat' AND lz.lang = 'zh'
                    AND lz.id = printf('%08d', ml.location_gts)
                 LEFT JOIN strings le ON le.object = 'OnlineText.dat' AND le.lang = 'en'
                    AND le.id = printf('%08d', ml.location_gts)
                 WHERE ml.monster_code = ?1",
            )
            .expect("monster places");
        stmt.query_map([code], |row| Ok(pick_place(lang, &row.get::<_, String>(1)?, &row.get::<_, String>(0)?)))
            .expect("place rows")
            .filter_map(Result::ok)
            .filter(|name| !name.is_empty())
            .collect()
    }

    pub(in crate::db) fn monster_drops(&self, code: &str, lang: &str) -> Vec<Hit> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT d.slot, d.item_code, i.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''),
                        i.large_image, i.large_frame
                 FROM monster_drops d
                 JOIN items i ON i.code = d.item_code
                 LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                    AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
                 LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                    AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
                 WHERE d.monster_code = ?1
                 ORDER BY d.slot",
            )
            .expect("drops");
        let rows: Vec<(i64, i64, String, String, String, i64)> = stmt
            .query_map([code], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })
            .expect("drop rows")
            .filter_map(Result::ok)
            .collect();
        rows.into_iter()
            .map(|(slot, item, en, zh, image, frame)| {
                let solo = slot >= 9;
                let name = lead(lang, &en, &zh);
                Hit {
                    title: if solo {
                        format!("{name}  {}", phrase("solo-drop", lang))
                    } else {
                        name
                    },
                    subtitle: caption(&trail(lang, &en, &zh), format!("#{item}")),
                    key: format!("item:{item}"),
                    icon: self.icon_path(&image, frame),
                    solo,
                    aside: String::new(),
                badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub fn monster_quests(&self, code: &str, lang: &str) -> Vec<Hit> {
        let code = code.trim();
        if code.is_empty() {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT s.quest_id, lower(s.kind), s.code, s.qty
             FROM quest_steps s
             WHERE s.role IN ('objective', 'requirement')
               AND (
                    (lower(s.kind) = 'monster' AND (',' || replace(s.code, ' ', '') || ',') LIKE '%,' || ?1 || ',%')
                    OR (lower(s.kind) = 'item' AND s.code IN (
                        SELECT CAST(item_code AS TEXT) FROM monster_drops WHERE monster_code = ?1
                    ))
               )
             ORDER BY s.quest_id, s.slot",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt
            .query_map([code], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut order = Vec::new();
        let mut reasons: HashMap<i64, (String, i64, String)> = HashMap::new();
        for (id, kind, step_code, qty) in rows {
            let entry = reasons.entry(id).or_insert_with(|| {
                order.push(id);
                (String::new(), 0, String::new())
            });
            if kind == "monster" && entry.0.is_empty() {
                entry.0 = qty;
            } else if kind == "item" && entry.1 == 0 {
                if let Ok(item) = step_code.parse::<i64>() {
                    entry.1 = item;
                    entry.2 = qty;
                }
            }
        }
        order.sort_by_key(|id| {
            let quest = self.index.quests.iter().find(|quest| quest.id == *id);
            (
                quest.map(|quest| quest_source_rank(&quest.source)).unwrap_or(9),
                quest.map(|quest| quest.code.clone()).unwrap_or_default(),
            )
        });
        order
            .into_iter()
            .filter_map(|id| {
                let (kill_qty, item, item_qty) = reasons.get(&id)?;
                let aside = if !kill_qty.is_empty() && kill_qty != "0" {
                    format!("× {}", commas_text(kill_qty))
                } else if *item > 0 {
                    let name = self.named_item(*item, lang, "");
                    if item_qty.is_empty() || item_qty == "0" || name.is_empty() {
                        name
                    } else {
                        format!("{name} × {}", commas_text(item_qty))
                    }
                } else {
                    String::new()
                };
                let mut hit = self.quest_link_hit(id, lang, &aside)?;
                hit.icon = self
                    .index
                    .quests
                    .iter()
                    .find(|quest| quest.id == id)
                    .and_then(|quest| self.index.npc_face(quest.npc));
                Some(hit)
            })
            .collect()
    }

    pub(in crate::db) fn monster_pair(&self, code: &str) -> Option<(String, String)> {
        self.conn
            .query_row(
                "SELECT IFNULL(se.text, ''), IFNULL(sz.text, '')
                 FROM monsters m
                 LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                    AND m.name_gts > 0 AND se.id = printf('%08d', m.name_gts)
                 LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                    AND m.name_gts > 0 AND sz.id = printf('%08d', m.name_gts)
                 WHERE m.code = ?1",
                [code],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok()
    }
}
