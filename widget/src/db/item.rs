use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::{Catalog, Hit};
use super::language::primary_lang;
use super::names::haystack;
use super::prefs::{load_set_extra, set_extra_name};
use super::recipe::PREF;
use super::rows::{ItemRow, SetMagic, SetRow};
use super::store::online;
use super::text::{caption, chrome_text, commas, element_file, element_letter, lead, lead3, phrase, stack3, trail3};

pub(in crate::db) struct EnhanceRow {
    pub(in crate::db) plus: i64,
    pub(in crate::db) dmin: i64,
    pub(in crate::db) dmax: i64,
    pub(in crate::db) move_speed: i64,
    pub(in crate::db) atk_speed: i64,
    pub(in crate::db) magic_resist: i64,
    pub(in crate::db) hit_resist: i64,
    pub(in crate::db) element: String,
    pub(in crate::db) element_damage: i64,
    pub(in crate::db) attack_range: i64,
    pub(in crate::db) skill_range: i64,
    pub(in crate::db) crit: i64,
    pub(in crate::db) mana: i64,
    pub(in crate::db) sight: i64,
    pub(in crate::db) air: i64,
    pub(in crate::db) ground: i64,
    pub(in crate::db) cost: i64,
    pub(in crate::db) merc_def: i64,
    pub(in crate::db) merc_atk: i64,
    pub(in crate::db) spell: i64,
    pub(in crate::db) reflect: i64,
    pub(in crate::db) invuln: i64,
}

impl EnhanceRow {
    pub(in crate::db) fn is_blank(&self) -> bool {
        self.dmin == 0
            && self.dmax == 0
            && self.move_speed == 0
            && self.atk_speed == 0
            && self.magic_resist == 0
            && self.hit_resist == 0
            && self.element.is_empty()
            && self.element_damage == 0
            && self.attack_range == 0
            && self.skill_range == 0
            && self.crit == 0
            && self.mana == 0
            && self.sight == 0
            && self.air == 0
            && self.ground == 0
            && self.cost == 0
            && self.merc_def == 0
            && self.merc_atk == 0
            && self.spell == 0
            && self.reflect == 0
            && self.invuln == 0
    }

    pub(in crate::db) fn summary(&self, lang: &str) -> String {
        let mut bits = vec![format!("+{}", self.plus)];
        if self.dmin != 0 || self.dmax != 0 {
            let damage = if self.dmin == self.dmax {
                commas(self.dmin)
            } else {
                format!("{}-{}", commas(self.dmin), commas(self.dmax))
            };
            bits.push(format!("{} {damage}", phrase("damage", lang)));
        }
        Self::push(&mut bits, lang, "move-speed", self.move_speed);
        Self::push(&mut bits, lang, "atk-speed", self.atk_speed);
        Self::push(&mut bits, lang, "magic-resist", self.magic_resist);
        Self::push(&mut bits, lang, "hit-resist", self.hit_resist);
        if !self.element.is_empty() || self.element_damage != 0 {
            let name = element_letter(&self.element, lang);
            if self.element_damage == 0 {
                bits.push(name);
            } else if name.is_empty() {
                bits.push(commas(self.element_damage));
            } else {
                bits.push(format!("{name} {}", commas(self.element_damage)));
            }
        }
        Self::push(&mut bits, lang, "atk-range", self.attack_range);
        Self::push(&mut bits, lang, "skill-range", self.skill_range);
        Self::push(&mut bits, lang, "crit", self.crit);
        Self::push(&mut bits, lang, "mana-cut", self.mana);
        Self::push(&mut bits, lang, "sight", self.sight);
        Self::push(&mut bits, lang, "air-dmg", self.air);
        Self::push(&mut bits, lang, "ground-dmg", self.ground);
        Self::push(&mut bits, lang, "stigma", self.cost);
        Self::push(&mut bits, lang, "merc-def", self.merc_def);
        Self::push(&mut bits, lang, "merc-atk", self.merc_atk);
        Self::push(&mut bits, lang, "spell-rate", self.spell);
        Self::push(&mut bits, lang, "reflect", self.reflect);
        Self::push(&mut bits, lang, "invuln", self.invuln);
        if bits.len() == 1 {
            bits.push("—".into());
        }
        bits.join("  ")
    }

    pub(in crate::db) fn push(bits: &mut Vec<String>, lang: &str, key: &str, value: i64) {
        if value != 0 {
            bits.push(format!("{} {}", phrase(key, lang), commas(value)));
        }
    }
}

pub(in crate::db) fn pref_rank(source: &str) -> usize {
    PREF.iter().position(|key| *key == source).unwrap_or(99)
}

pub(in crate::db) fn item_category_pair(key: &str) -> (&'static str, &'static str) {
    match key {
        "set" => ("Sets", "套裝"),
        "W" => ("Weapons", "武器"),
        "E" => ("Materials", "材料"),
        "H" => ("Helmets", "頭盔"),
        "A" => ("Armor", "盔甲"),
        "Y" => ("Orbs", "寶珠"),
        "I" => ("Consumables", "消耗"),
        "IL" => ("Scrolls", "傳送"),
        "II" => ("Boxes", "寶箱"),
        "S" => ("Shoes", "鞋子"),
        "G" => ("Gloves", "手套"),
        "B" => ("Belts", "腰帶"),
        "C" => ("Clothes", "衣服"),
        "R" => ("Rings", "戒指"),
        "J" => ("Bracelets", "手環"),
        "V" => ("Leggings", "綁腿"),
        "F" => ("Food", "食物"),
        "N" => ("Necklaces", "項鍊"),
        "P" => ("Earrings", "耳環"),
        "L" => ("Medicine", "藥品"),
        "T" => ("Tools", "工具"),
        "U" => ("Charms", "護符"),
        "Q" => ("Guardians", "武神"),
        "M" => ("Beasts", "神獸"),
        "O" => ("Animals", "動物"),
        "K" => ("Statues", "石像"),
        _ => ("Other", "其他"),
    }
}

pub(in crate::db) fn item_category_order(key: &str) -> usize {
    [
        "W", "E", "H", "A", "Y", "I", "IL", "II", "S", "G", "B", "C", "R", "J", "V", "F", "N", "P", "L", "T", "U", "Q", "M", "O", "K", "",
    ]
    .iter()
    .position(|item| *item == key)
    .unwrap_or(80)
}

pub(in crate::db) fn item_category_name(key: &str, lang: &str) -> String {
    let (en, zh) = item_category_pair(key);
    chrome_text(lang, zh, en)
}

pub(in crate::db) fn set_kind_rank(kind: i64) -> i64 {
    match kind {
        0 => 0,
        2 => 1,
        _ => 2,
    }
}

pub(in crate::db) fn set_kind_label(kind: i64, lang: &str) -> String {
    match kind {
        1 => phrase("set-charm", lang),
        2 => phrase("set-unique", lang),
        _ => phrase("set", lang),
    }
}

pub(in crate::db) const PIECE_WORDS: &[(&str, &str)] = &[
    ("팔보호구", "G"),
    ("머리띠", "H"),
    ("전투화", "S"),
    ("목걸이", "N"),
    ("귀걸이", "P"),
    ("귀고리", "P"),
    ("가락지", "R"),
    ("팔찌", "J"),
    ("장갑", "G"),
    ("갑옷", "A"),
    ("예복", "A"),
    ("투구", "H"),
    ("요대", "B"),
    ("신발", "S"),
    ("반지", "R"),
    ("지환", "R"),
    ("각반", "V"),
    ("의복", "C"),
    ("복장", "C"),
    ("도복", "C"),
];

pub(in crate::db) fn piece_prefix(full: &str, base: &str, lang: &str) -> String {
    let full = full.trim();
    let base = base.trim();
    let rest = if !base.is_empty() && full.ends_with(base) {
        full[..full.len() - base.len()].trim()
    } else {
        ""
    };
    match rest {
        "" => String::new(),
        "북두칠성" => chrome_text(lang, "北斗七星", "Big Dipper"),
        "천추" => chrome_text(lang, "天樞", "Dubhe"),
        "천선" => chrome_text(lang, "天璇", "Merak"),
        "천기" => chrome_text(lang, "天璣", "Phecda"),
        "천권" => chrome_text(lang, "天權", "Megrez"),
        "옥형" => chrome_text(lang, "玉衡", "Alioth"),
        "개양" => chrome_text(lang, "開陽", "Mizar"),
        "요광" => chrome_text(lang, "搖光", "Alkaid"),
        "홈이있는" => chrome_text(lang, "有孔", "Socketed"),
        other => other.to_string(),
    }
}

pub(in crate::db) fn piece_slot(category: &str, name: &str) -> String {
    if !category.is_empty() {
        return category.to_string();
    }
    PIECE_WORDS
        .iter()
        .find(|(word, _)| name.contains(word))
        .map(|(_, slot)| (*slot).to_string())
        .unwrap_or_else(|| "other".into())
}

pub(in crate::db) fn set_slot_order(slot: &str) -> usize {
    ["W", "H", "A", "G", "B", "S", "R", "N", "P", "J", "V", "C", "other"]
        .iter()
        .position(|item| *item == slot)
        .unwrap_or(90)
}

pub(in crate::db) fn set_slot_pair(slot: &str) -> Option<(&'static str, &'static str)> {
    Some(match slot {
        "W" => ("Weapon", "武器"),
        "H" => ("Head", "頭"),
        "A" => ("Armor", "甲"),
        "G" => ("Hands", "手"),
        "B" => ("Waist", "腰"),
        "S" => ("Shoes", "鞋"),
        "R" => ("Ring ×2", "戒×2"),
        "N" => ("Necklace", "項鍊"),
        "P" => ("Earring", "耳環"),
        "J" => ("Bracelet", "手環"),
        "V" => ("Leggings", "綁腿"),
        "C" => ("Clothes", "衣服"),
        "other" => ("Other", "其他"),
        _ => return None,
    })
}

pub(in crate::db) fn set_slot_label(slot: &str, lang: &str) -> String {
    set_slot_pair(slot).map(|(en, zh)| chrome_text(lang, zh, en)).unwrap_or_else(|| slot.to_string())
}

pub(in crate::db) fn set_slot_heading(slot: &str, lang: &str) -> Option<String> {
    set_slot_pair(slot).map(|(en, zh)| chrome_text(lang, zh, en))
}

pub(in crate::db) fn set_piece_label(pieces: i64, lang: &str) -> String {
    if pieces <= 0 {
        return String::new();
    }
    match primary_lang(lang) {
        "en" => format!("{pieces} pc"),
        "ko" => format!("{pieces}개"),
        _ => format!("{pieces}件"),
    }
}

pub(in crate::db) fn set_title_lang(lang: &str, ko: &str, en: &str, zh: &str) -> String {
    lead3(lang, en, zh, ko)
}

pub(in crate::db) fn set_stat_name(stat: &str, lang: &str) -> Option<String> {
    let (zh, en) = match stat {
        "S" => ("力量", "STR"),
        "D" => ("敏捷", "AGI"),
        "V" => ("體質", "VIT"),
        "I" => ("智力", "INT"),
        "P" => ("打擊抗性", "Hit resist"),
        "M" => ("魔法抗性", "Magic resist"),
        "W" => ("水屬性", "Water"),
        "A" => ("風屬性", "Wind"),
        "F" => ("火屬性", "Fire"),
        "T" => ("雷屬性", "Thunder"),
        "E" => ("土屬性", "Earth"),
        "R" => ("移動速度", "Move"),
        "C" => ("暴擊", "Crit"),
        _ => return None,
    };
    Some(chrome_text(lang, zh, en))
}

pub(in crate::db) fn set_stat_line(stat: &str, value: i64, lang: &str) -> Option<String> {
    if value == 0 {
        return None;
    }
    Some(format!("{} +{value}", set_stat_name(stat, lang)?))
}

#[derive(Default)]
pub(in crate::db) struct StatBag {
    pub(in crate::db) order: Vec<String>,
    pub(in crate::db) values: HashMap<String, i64>,
    pub(in crate::db) dmin: i64,
    pub(in crate::db) dmax: i64,
    pub(in crate::db) notes: Vec<String>,
    pub(in crate::db) damage_pct: i64,
    pub(in crate::db) element: i64,
    pub(in crate::db) earth: i64,
}

impl StatBag {
    pub(in crate::db) fn add(&mut self, label: &str, value: i64) {
        if value == 0 || label.is_empty() {
            return;
        }
        if !self.values.contains_key(label) {
            self.order.push(label.to_string());
        }
        *self.values.entry(label.to_string()).or_default() += value;
    }

    pub(in crate::db) fn add_gear(&mut self, values: &[i64; 10], lang: &str) {
        self.add(&chrome_text(lang, "力量", "STR"), values[0]);
        self.add(&chrome_text(lang, "敏捷", "AGI"), values[1]);
        self.add(&chrome_text(lang, "體質", "VIT"), values[2]);
        self.add(&chrome_text(lang, "智力", "INT"), values[3]);
        self.add(&chrome_text(lang, "生命上限", "Max HP"), values[4]);
        self.add(&chrome_text(lang, "魔力上限", "Max MP"), values[5]);
        self.add(&chrome_text(lang, "攻擊", "Attack"), values[6]);
        self.add(&chrome_text(lang, "防禦", "Defense"), values[7]);
        self.add(&chrome_text(lang, "打擊抗性", "Hit resist"), values[8]);
        self.add(&chrome_text(lang, "魔法抗性", "Magic resist"), values[9]);
    }

    pub(in crate::db) fn add_affix(&mut self, values: &[i64; 10], lang: &str) {
        self.add(&chrome_text(lang, "力量", "STR"), values[0]);
        self.add(&chrome_text(lang, "敏捷", "AGI"), values[1]);
        self.add(&chrome_text(lang, "體質", "VIT"), values[2]);
        self.add(&chrome_text(lang, "智力", "INT"), values[3]);
        self.add_element(values[4], values[5]);
        self.add_damage_pct(values[6]);
        self.add(&phrase("dmg-cut", lang), values[7]);
        self.add(&phrase("crit", lang), values[8]);
        self.add(&phrase("crit-dmg", lang), values[9]);
    }

    pub(in crate::db) fn absorb(&mut self, other: &StatBag, copies: i64) {
        if other.dmin != 0 || other.dmax != 0 {
            self.dmin += other.dmin * copies;
            self.dmax += other.dmax * copies;
        }
        self.add_damage_pct(other.damage_pct * copies);
        self.element += other.element * copies;
        self.earth += other.earth * copies;
        for label in &other.order {
            self.add(label, other.values[label] * copies);
        }
        for note in &other.notes {
            if !self.notes.iter().any(|have| have == note) {
                self.notes.push(note.clone());
            }
        }
    }

    pub(in crate::db) fn add_damage_pct(&mut self, value: i64) {
        self.damage_pct += value;
    }

    pub(in crate::db) fn add_element(&mut self, element: i64, land: i64) {
        if element == 0 && land == 0 {
            return;
        }
        self.element += element;
        self.earth += if land == 0 { element } else { element + land };
    }

    pub(in crate::db) fn note(&mut self, text: String) {
        if !text.is_empty() && !self.notes.iter().any(|have| have == &text) {
            self.notes.push(text);
        }
    }

    pub(in crate::db) fn is_empty(&self) -> bool {
        self.dmin == 0
            && self.dmax == 0
            && self.damage_pct == 0
            && self.element == 0
            && self.earth == 0
            && self.values.values().all(|value| *value == 0)
            && self.notes.is_empty()
    }

    pub(in crate::db) fn rows(&self, lang: &str) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        if self.dmin != 0 || self.dmax != 0 {
            let damage = if self.dmin == self.dmax {
                commas(self.dmin)
            } else {
                format!("{}-{}", commas(self.dmin), commas(self.dmax))
            };
            rows.push((phrase("damage", lang), damage));
        }
        if self.damage_pct != 0 {
            rows.push((phrase("damage", lang), format!("+{}%", self.damage_pct)));
        }
        if self.element != 0 {
            rows.push((phrase("ela", lang), format!("+{}", self.element)));
        }
        if self.earth != 0 && self.earth != self.element {
            rows.push((chrome_text(lang, "土", "Earth"), format!("+{}", self.earth)));
        }
        for label in &self.order {
            let value = self.values[label];
            if value == 0 {
                continue;
            }
            let shown = if value > 0 { format!("+{value}") } else { value.to_string() };
            rows.push((label.clone(), shown));
        }
        for note in &self.notes {
            rows.push((note.clone(), String::new()));
        }
        rows
    }

    pub(in crate::db) fn text(&self, lang: &str) -> String {
        let mut parts = Vec::new();
        if self.dmin != 0 || self.dmax != 0 {
            let damage = if self.dmin == self.dmax {
                commas(self.dmin)
            } else {
                format!("{}-{}", commas(self.dmin), commas(self.dmax))
            };
            parts.push(format!("{} {damage}", phrase("damage", lang)));
        }
        if self.damage_pct != 0 {
            parts.push(format!("{} +{}%", phrase("damage", lang), self.damage_pct));
        }
        if self.element != 0 || self.earth != 0 {
            let earth = chrome_text(lang, "土", "Earth");
            if self.earth != 0 && self.earth != self.element {
                parts.push(format!("{} +{}（{earth} +{}）", phrase("ela", lang), self.element, self.earth));
            } else {
                parts.push(format!("{} +{}", phrase("ela", lang), self.element));
            }
        }
        for label in &self.order {
            let value = self.values[label];
            if value == 0 {
                continue;
            }
            if value > 0 {
                parts.push(format!("{label} +{value}"));
            } else {
                parts.push(format!("{label} {value}"));
            }
        }
        parts.extend(self.notes.iter().cloned());
        parts.join("、")
    }
}

pub(in crate::db) fn damage_percent_magic(code: i64) -> bool {
    matches!(code, 10 | 14 | 15 | 23 | 28)
}

pub(in crate::db) fn set_magic_line(code: i64, value1: i64, value2: i64, percent: bool, lang: &str) -> String {
    let mark = if percent { "%" } else { "" };
    let (zh, en) = match code {
        1 => (format!("攻擊力 +{value1}{mark}"), format!("Attack +{value1}{mark}")),
        2 => (format!("傷害反射 {value1}"), format!("Reflect {value1}")),
        3 => (format!("體力吸收 {value1}"), format!("Absorb HP {value1}")),
        4 => (format!("魔力消耗 -{value1}%"), format!("Mana cost -{value1}%")),
        5 => (format!("魔力吸收 {value1}%"), format!("Absorb mana {value1}%")),
        6 => ("免疫".into(), "Immunity".into()),
        7 | 16 | 21 | 30 => (
            format!("屬性 +{value1}（土 +{value2}）"),
            format!("Element +{value1} (earth +{value2})"),
        ),
        8 => (format!("對空傷害 +{value1}%"), format!("Air damage +{value1}%")),
        9 => (format!("對地傷害 +{value1}%"), format!("Ground damage +{value1}%")),
        10 | 14 | 15 | 23 | 28 => (format!("傷害 +{value1}%"), format!("Damage +{value1}%")),
        11 => (format!("每 2 秒回復體力 {value1}"), format!("HP regen {value1} / 2s")),
        12 => (format!("每 2 秒回復魔力 {value1}"), format!("Mana regen {value1} / 2s")),
        13 => (format!("每 3 秒回復 {value1}"), format!("Regen {value1} / 3s")),
        17 => ("雷電彈洗禮".into(), "Lightning barrage".into()),
        18 => ("冰花爆".into(), "Ice burst".into()),
        19 => ("黃牛突進".into(), "Bull rush".into()),
        20 => ("赤狼劍舞".into(), "Sword dance".into()),
        22 => ("宰相的加護".into(), "Bajirao's blessing".into()),
        24 => ("天子銃改良".into(), "Improved cannon".into()),
        25 => ("雷鳥突擊".into(), "Thunderbird charge".into()),
        26 => ("激流陣".into(), "Torrent".into()),
        27 => ("一閃".into(), "Quick draw".into()),
        29 => ("大帝的加護".into(), "Akbar's blessing".into()),
        31 => (format!("疾風 {value1}"), format!("Gale {value1}")),
        _ => return String::new(),
    };
    chrome_text(lang, &zh, &en)
}

pub(in crate::db) fn weapon_family_pair(key: &str) -> (&'static str, &'static str) {
    match key {
        "axe" => ("Axe", "斧"),
        "spear" => ("Spear", "槍"),
        "bow" => ("Bow", "弓"),
        "sling" => ("Sling", "彈弓"),
        "cannon" => ("Cannon", "銃砲"),
        "gun" => ("Gun", "鐵炮"),
        "dart" => ("Dart", "鏢"),
        "fan" => ("Fan", "扇"),
        "needle" => ("Needle", "針"),
        "claw" => ("Claw", "爪"),
        "bead" => ("Beads", "念珠"),
        "mirror" => ("Mirror", "鏡"),
        "charm" => ("Charm", "符"),
        "blade" => ("Blade", "刀"),
        "sword" => ("Sword", "劍"),
        "doll" => ("Doll", "娃娃"),
        "bell" => ("Bell", "鈴"),
        "lute" => ("Lute", "月琴"),
        "tablet" => ("Tablet", "笏"),
        _ => ("Other", "其他"),
    }
}

pub(in crate::db) fn weapon_family_name(key: &str, lang: &str) -> String {
    let (en, zh) = weapon_family_pair(key);
    chrome_text(lang, zh, en)
}

pub(in crate::db) fn load_items(conn: &Connection, texts: &HashMap<String, String>) -> Result<Vec<ItemRow>, String> {
    let has_column = |name: &str| {
        conn.query_row(
            "SELECT 1 FROM pragma_table_info('items') WHERE name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .is_ok()
    };
    let sql = if has_column("subtype") {
        "SELECT code, name_ko, name_gts, IFNULL(category, ''), IFNULL(subtype, '') FROM items ORDER BY code"
    } else if has_column("category") {
        "SELECT code, name_ko, name_gts, IFNULL(category, ''), '' FROM items ORDER BY code"
    } else {
        "SELECT code, name_ko, name_gts, '', '' FROM items ORDER BY code"
    };
    let mut stmt = conn.prepare(sql).map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut items = Vec::new();
    for row in rows {
        let (code, ko, name_gts, category, subtype) = row.map_err(|err| err.to_string())?;
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        items.push(ItemRow {
            code,
            names: haystack(&[&ko, &en, &zh]),
            code_str: code.to_string(),
            ko,
            en,
            zh,
            category,
            subtype,
        });
    }
    Ok(items)
}

pub(in crate::db) fn load_sets(
    conn: &Connection,
    texts: &HashMap<String, String>,
    icons: &HashMap<i64, (PathBuf, i32)>,
) -> Result<(Vec<SetRow>, HashMap<i64, SetMagic>), String> {
    let ready = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'item_sets'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .is_ok();
    if !ready {
        return Ok((Vec::new(), HashMap::new()));
    }
    let mut icons_for: HashMap<i64, (PathBuf, i32)> = HashMap::new();
    let mut member_stmt = conn
        .prepare("SELECT set_code, item_code FROM item_set_members ORDER BY enhance, item_code")
        .map_err(|err| err.to_string())?;
    let member_rows = member_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|err| err.to_string())?;
    let mut counted: HashMap<i64, i64> = HashMap::new();
    for row in member_rows {
        let (set_code, item_code) = row.map_err(|err| err.to_string())?;
        *counted.entry(set_code).or_default() += 1;
        if icons_for.contains_key(&set_code) {
            continue;
        }
        if let Some(icon) = icons.get(&item_code) {
            icons_for.insert(set_code, icon.clone());
        }
    }
    let mut stmt = conn
        .prepare("SELECT code, kind, name_ko, name_gts, pieces FROM item_sets")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut sets = Vec::new();
    for row in rows {
        let (code, kind, ko, name_gts, pieces) = row.map_err(|err| err.to_string())?;
        if counted.get(&code).copied().unwrap_or(0) == 0 {
            continue;
        }
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        sets.push(SetRow {
            code,
            kind,
            pieces,
            names: haystack(&[&ko, &en, &zh, &code.to_string()]),
            ko,
            en,
            zh,
            icon: icons_for.remove(&code),
        });
    }
    let mut magic_stmt = conn
        .prepare("SELECT code, value1, value2, percent, note FROM item_set_magic")
        .map_err(|err| err.to_string())?;
    let magic_rows = magic_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut magic = HashMap::new();
    for row in magic_rows {
        let (code, value1, value2, percent, note) = row.map_err(|err| err.to_string())?;
        magic.insert(
            code,
            SetMagic {
                value1,
                value2,
                percent: percent != 0,
                note,
            },
        );
    }
    Ok((sets, magic))
}

pub(in crate::db) fn load_item_icons(conn: &Connection, client_root: &Path) -> Result<HashMap<i64, (PathBuf, i32)>, String> {
    let folder = client_root.join("Assets").join("Textures").join("Items");
    let mut files = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            files.insert(entry.file_name().to_string_lossy().to_lowercase(), entry.path());
        }
    }
    let mut stmt = conn
        .prepare("SELECT code, large_image, large_frame FROM items WHERE large_image != '' AND large_frame >= 0")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))
        .map_err(|err| err.to_string())?;
    let mut icons = HashMap::new();
    for row in rows {
        let (code, image, frame) = row.map_err(|err| err.to_string())?;
        let Some(file_name) = Path::new(&image).file_name() else { continue };
        let Some(path) = files.get(&file_name.to_string_lossy().to_lowercase()) else { continue };
        icons.insert(code, (path.clone(), frame as i32));
    }
    borrow_base_icons(conn, &mut icons);
    Ok(icons)
}

pub(in crate::db) fn borrow_base_icons(conn: &Connection, icons: &mut HashMap<i64, (PathBuf, i32)>) {
    let ready = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'item_bases'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .is_ok();
    if !ready {
        return;
    }
    let Ok(mut stmt) = conn.prepare("SELECT item_code, base_code FROM item_bases") else {
        return;
    };
    let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))) else {
        return;
    };
    let pairs: Vec<(i64, i64)> = rows.filter_map(Result::ok).collect();
    for _ in 0..4 {
        let mut added = false;
        for (item, base) in &pairs {
            if icons.contains_key(item) {
                continue;
            }
            if let Some(icon) = icons.get(base).cloned() {
                icons.insert(*item, icon);
                added = true;
            }
        }
        if !added {
            break;
        }
    }
}

impl Catalog {
    pub(in crate::db) fn sprite(&self, code: &str) -> Option<(PathBuf, i32)> {
        self.pictures.get(code).cloned()
    }

    pub fn element_icon(&self, code: &str) -> Option<(PathBuf, i32)> {
        let element: i64 = self
            .conn
            .query_row("SELECT element FROM monsters WHERE code = ?1", [code], |row| row.get(0))
            .ok()?;
        let relative = element_file(element)?;
        let path = self.client_root.join(relative);
        path.is_file().then_some((path, 0))
    }

    pub(in crate::db) fn open_item(&self, code: i64, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let row = self.conn.query_row(
            "SELECT i.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''), price, weight,
                    description, large_image, large_frame
             FROM items i
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE i.code = ?1",
            [code],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        );
        let Ok((ko, en, zh, price, weight, description, image, frame)) = row else {
            return (phrase("missing-item", lang), Vec::new(), None);
        };
        let name = stack3(lang, &en, &zh, &ko);
        let lines = self.item_lines(code, lang, price, weight);
        let note = self.item_note(self.item_desc_gts(code), &description, lang);
        let mut text = name.clone();
        for (label, value) in &lines {
            text.push_str(&format!("\n{label}  {value}"));
        }
        if !note.is_empty() {
            text.push_str(&format!("\n{note}"));
        }
        let links = self.item_relations(code, lang);
        let icon = self.icon_path(&image, frame);
        (text, links, icon)
    }

    /// Alternate names for item browsing come from the loaded index, not one SQL query per row.
    pub fn item_alternate_name(&self, key: &str, lang: &str) -> String {
        let Some((kind, id)) = key.split_once(':') else { return String::new() };
        if matches!(kind, "icat" | "isub") {
            let (en, zh) = if kind == "icat" { item_category_pair(id) } else { weapon_family_pair(id) };
            return trail3(lang, en, zh, &chrome_text("ko", zh, en));
        }
        let names = match kind {
            "item" => id.parse().ok().and_then(|code| self.index.find_item(code))
                .map(|item| (item.en.as_str(), item.zh.as_str(), item.ko.as_str())),
            "set" => id.parse::<i64>().ok().and_then(|code| self.index.sets.iter().find(|set| set.code == code))
                .map(|set| (set.en.as_str(), set.zh.as_str(), set.ko.as_str())),
            _ => None,
        };
        names.map(|(en, zh, ko)| trail3(lang, en, zh, ko)).unwrap_or_default()
    }

    pub fn item_sheet(&self, code: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String)>, String)> {
        let row = self.conn.query_row(
            "SELECT i.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''), price, weight, description, IFNULL(desc_gts, 0)
             FROM items i
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE i.code = ?1",
            [code],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        );
        let Ok((ko, en, zh, price, weight, description, desc_gts)) = row else {
            return None;
        };
        Some((
            lead3(lang, &en, &zh, &ko),
            trail3(lang, &en, &zh, &ko),
            format!("#{code}"),
            self.item_lines(code, lang, price, weight),
            self.item_note(desc_gts, &description, lang),
        ))
    }

    pub(in crate::db) fn item_desc_gts(&self, code: i64) -> i64 {
        self.conn
            .query_row("SELECT IFNULL(desc_gts, 0) FROM items WHERE code = ?1", [code], |row| row.get(0))
            .unwrap_or(0)
    }

    pub(in crate::db) fn item_note(&self, desc_gts: i64, korean: &str, lang: &str) -> String {
        let zh = self.online_line(desc_gts, "zh");
        let en = self.online_line(desc_gts, "en");
        let korean = if korean.trim() == "0" { "" } else { korean };
        lead3(lang, &en, &zh, korean)
    }

    pub(in crate::db) fn online_line(&self, id: i64, lang: &str) -> String {
        self.conn
            .query_row(
                "SELECT text FROM strings WHERE object = 'OnlineText.dat' AND lang = ?1 AND id = printf('%08d', ?2)",
                (lang, id),
                |row| row.get(0),
            )
            .unwrap_or_default()
    }

    pub(in crate::db) fn item_lines(&self, code: i64, lang: &str, price: i64, weight: i64) -> Vec<(String, String)> {
        let mut lines = vec![(phrase("code", lang), code.to_string())];
        let vendor = price / 3;
        if vendor > 0 {
            lines.push((phrase("vendor", lang), commas(vendor)));
        }
        if weight > 0 {
            lines.push((phrase("weight", lang), commas(weight)));
        }
        if let Some(item) = self.index.items.iter().find(|item| item.code_str == code.to_string()) {
            let mut shown = item_category_name(&item.category, lang);
            if !item.subtype.is_empty() {
                shown = format!("{shown} · {}", weapon_family_name(&item.subtype, lang));
            }
            lines.push((phrase("item-sort", lang), shown));
        }
        lines.extend(self.enchant_lines(code, lang));
        lines.extend(self.gear_lines(code, lang));
        lines.extend(self.affix_lines(code, lang));
        lines.extend(self.set_lines(code, lang));
        lines.extend(self.enhance_lines(code, lang));
        lines
    }

    pub(in crate::db) fn set_rows(&self, item_code: i64) -> Vec<(i64, i64, i64, i64)> {
        if !self.has_table("item_sets") {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT s.code, s.kind, s.pieces, m.enhance
             FROM item_set_members m
             JOIN item_sets s ON s.code = m.set_code
             WHERE m.item_code = ?1
             ORDER BY s.kind, s.code",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([item_code], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    pub(in crate::db) fn set_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        let mut lines = Vec::new();
        for (set_code, kind, _pieces, enhance) in self.set_rows(code) {
            let name = self.set_name(set_code, lang);
            let shown = if enhance > 0 { format!("{name}  +{enhance}") } else { name };
            lines.push((set_kind_label(kind, lang), shown));
            lines.extend(self.set_effect_lines(set_code, lang));
        }
        lines
    }

    pub(in crate::db) fn set_name(&self, code: i64, lang: &str) -> String {
        self.index
            .sets
            .iter()
            .find(|set| set.code == code)
            .map(|set| set_title_lang(lang, &set.ko, &set.en, &set.zh))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("#{code}"))
    }

    pub(in crate::db) fn gear_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        let mut bag = StatBag::default();
        bag.add_gear(&self.gear_values(code), lang);
        if let Some((base, _)) = self.base_link(code) {
            if base != code {
                bag.add_gear(&self.gear_values(base), lang);
            }
        }
        bag.rows(lang)
    }

    pub(in crate::db) fn affix_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        let Some(text) = self.affix_text(code, lang) else {
            return Vec::new();
        };
        vec![(phrase("item-fx", lang), text)]
    }

    pub(in crate::db) fn gear_values(&self, code: i64) -> [i64; 10] {
        if !self.has_table("item_gear") {
            return [0; 10];
        }
        self.conn
            .query_row(
                "SELECT strength, dexterity, vitality, intelligence, hp_max, mp_max, attack, armor, hit_resist, magic_resist
                 FROM item_gear WHERE item_code = ?1",
                [code],
                |row| {
                    Ok([
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                    ])
                },
            )
            .unwrap_or([0; 10])
    }

    pub(in crate::db) fn affix_values(&self, code: i64) -> Option<[i64; 10]> {
        if !self.has_table("item_affix") {
            return None;
        }
        self.conn
            .query_row(
                "SELECT strength, dexterity, vitality, intelligence, element, land, attack, reduce, crit, crit_damage
                 FROM item_affix WHERE item_code = ?1",
                [code],
                |row| {
                    Ok([
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                    ])
                },
            )
            .ok()
    }

    pub(in crate::db) fn affix_text(&self, code: i64, lang: &str) -> Option<String> {
        let values = self.affix_values(code)?;
        let mut bag = StatBag::default();
        bag.add_affix(&values, lang);
        let text = bag.text(lang);
        if text.is_empty() { None } else { Some(text) }
    }

    pub(in crate::db) fn set_breakdown(&self, code: i64, lang: &str) -> Vec<(String, String, String, String)> {
        let samples = self.set_piece_samples(code);
        let special = samples.iter().any(|(_, item)| self.affix_values(*item).is_some());
        let mut lines = Vec::new();
        let mut total = StatBag::default();
        if !samples.is_empty() {
            lines.push((phrase("item-fx", lang), String::new(), "fold:item".into(), String::new()));
        }
        for (slot, item) in samples {
            let copies = if slot == "R" { 2 } else { 1 };
            let mut name = self.item_name(item, lang);
            if copies == 2 {
                name = format!("{name} ×2");
            }
            lines.push((String::new(), name, format!("item:{item}"), "item".into()));
            if !special {
                continue;
            }
            let mut bag = self.enhance_bag(item, lang);
            bag.add_gear(&self.gear_values(item), lang);
            if let Some((base, _)) = self.base_link(item) {
                bag.add_gear(&self.gear_values(base), lang);
            }
            if let Some(values) = self.affix_values(item) {
                bag.add_affix(&values, lang);
            }
            if bag.is_empty() {
                continue;
            }
            lines.extend(bag.rows(lang).into_iter().map(|(label, value)| (label, value, String::new(), "item".into())));
            total.absorb(&bag, copies);
        }
        let effects = self.set_effect_rows(code);
        if !effects.is_empty() {
            lines.push((phrase("set-fx", lang), String::new(), "fold:set".into(), String::new()));
        }
        for (pieces, stat, value, magic) in effects {
            let mut rows = Vec::new();
            if let Some(name) = set_stat_name(&stat, lang) {
                if value != 0 {
                    rows.push((name.clone(), format!("+{value}")));
                    total.add(&name, value);
                }
            }
            if magic > 0 {
                if damage_percent_magic(magic) {
                    let pct = self.index.set_magic.get(&magic).map(|row| row.value1).unwrap_or(0);
                    if pct != 0 {
                        rows.push((phrase("damage", lang), format!("+{pct}%")));
                        total.add_damage_pct(pct);
                    }
                } else {
                    let note = self.magic_line(magic, lang);
                    if !note.is_empty() {
                        rows.push((note.clone(), String::new()));
                        total.note(note);
                    }
                }
            }
            if rows.is_empty() {
                continue;
            }
            lines.push((set_piece_label(pieces, lang), String::new(), String::new(), "set".into()));
            lines.extend(rows.into_iter().map(|(label, value)| (label, value, String::new(), "set".into())));
        }
        if special && !total.is_empty() {
            lines.push((phrase("total-fx", lang), String::new(), "fold:total".into(), String::new()));
            lines.extend(total.rows(lang).into_iter().map(|(label, value)| (label, value, String::new(), "total".into())));
        }
        lines
    }

    pub(in crate::db) fn set_piece_samples(&self, code: i64) -> Vec<(String, i64)> {
        let mut seen = HashSet::new();
        let mut samples = Vec::new();
        for (slot, item, _) in self.set_member_slots(code) {
            if seen.insert(slot.clone()) {
                samples.push((slot, item));
            }
        }
        samples
    }

    pub(in crate::db) fn enhance_bag(&self, code: i64, lang: &str) -> StatBag {
        let mut bag = StatBag::default();
        if !self.has_table("enhance") {
            return bag;
        }
        let row = self.conn.query_row(
            "SELECT dmin, dmax, move, atk_speed, magic_resist, hit_resist, element_damage, crit, mana, sight,
                    air, ground, cost, merc_def, merc_atk, spell, reflect, invuln
             FROM enhance WHERE item_code = ?1 ORDER BY plus LIMIT 1",
            [code],
            |row| {
                Ok(EnhanceRow {
                    plus: 0,
                    dmin: row.get(0)?,
                    dmax: row.get(1)?,
                    move_speed: row.get(2)?,
                    atk_speed: row.get(3)?,
                    magic_resist: row.get(4)?,
                    hit_resist: row.get(5)?,
                    element: String::new(),
                    element_damage: row.get(6)?,
                    attack_range: 0,
                    skill_range: 0,
                    crit: row.get(7)?,
                    mana: row.get(8)?,
                    sight: row.get(9)?,
                    air: row.get(10)?,
                    ground: row.get(11)?,
                    cost: row.get(12)?,
                    merc_def: row.get(13)?,
                    merc_atk: row.get(14)?,
                    spell: row.get(15)?,
                    reflect: row.get(16)?,
                    invuln: row.get(17)?,
                })
            },
        );
        if let Ok(row) = row {
            if row.dmin != 0 || row.dmax != 0 {
                bag.dmin += row.dmin;
                bag.dmax += row.dmax;
            }
            bag.add(&phrase("move-speed", lang), row.move_speed);
            bag.add(&phrase("atk-speed", lang), row.atk_speed);
            bag.add(&phrase("magic-resist", lang), row.magic_resist);
            bag.add(&phrase("hit-resist", lang), row.hit_resist);
            bag.add(&phrase("ela", lang), row.element_damage);
            bag.add(&phrase("crit", lang), row.crit);
            bag.add(&phrase("mana-cut", lang), row.mana);
            bag.add(&phrase("sight", lang), row.sight);
            bag.add(&phrase("air-dmg", lang), row.air);
            bag.add(&phrase("ground-dmg", lang), row.ground);
            bag.add(&phrase("stigma", lang), row.cost);
            bag.add(&phrase("merc-def", lang), row.merc_def);
            bag.add(&phrase("merc-atk", lang), row.merc_atk);
            bag.add(&phrase("spell-rate", lang), row.spell);
            bag.add(&phrase("reflect", lang), row.reflect);
            bag.add(&phrase("invuln", lang), row.invuln);
        }
        bag
    }

    pub(in crate::db) fn set_effect_rows(&self, code: i64) -> Vec<(i64, String, i64, i64)> {
        if !self.has_table("item_set_effects") {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT pieces, stat, value, magic FROM item_set_effects WHERE set_code = ?1 ORDER BY pieces",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([code], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    pub(in crate::db) fn set_effect_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        if !self.has_table("item_set_effects") {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT pieces, stat, value, magic FROM item_set_effects WHERE set_code = ?1 ORDER BY pieces",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([code], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
        })
        .map(|rows| {
            rows.filter_map(Result::ok)
                .filter_map(|(pieces, stat, value, magic)| {
                    let text = self.effect_text(&stat, value, magic, lang);
                    if text.is_empty() {
                        None
                    } else {
                        Some((set_piece_label(pieces, lang), text))
                    }
                })
                .collect()
        })
        .unwrap_or_default()
    }

    pub(in crate::db) fn effect_text(&self, stat: &str, value: i64, magic: i64, lang: &str) -> String {
        let mut parts = Vec::new();
        if let Some(text) = set_stat_line(stat, value, lang) {
            parts.push(text);
        }
        if magic > 0 {
            let text = self.magic_line(magic, lang);
            if !text.is_empty() {
                parts.push(text);
            }
        }
        parts.join("、")
    }

    pub(in crate::db) fn magic_line(&self, code: i64, lang: &str) -> String {
        let Some(magic) = self.index.set_magic.get(&code) else {
            return String::new();
        };
        let named = set_magic_line(code, magic.value1, magic.value2, magic.percent, lang);
        if named.is_empty() { magic.note.clone() } else { named }
    }

    pub(in crate::db) fn item_set_links(&self, code: i64, lang: &str) -> Vec<Hit> {
        self.set_rows(code)
            .into_iter()
            .map(|(set_code, kind, pieces, _enhance)| Hit {
                title: self.set_name(set_code, lang),
                subtitle: set_piece_label(pieces, lang),
                key: format!("set:{set_code}"),
                icon: self.index.sets.iter().find(|set| set.code == set_code).and_then(|set| set.icon.clone()),
                solo: false,
                aside: if kind == 0 { String::new() } else { set_kind_label(kind, lang) },
                badge: None,
                chip: String::new(),
            })
            .collect()
    }

    pub fn set_sheet(&self, code: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String, String, String)>, String)> {
        let set = self.index.sets.iter().find(|set| set.code == code)?;
        let mut lines = Vec::new();
        if set.kind != 0 {
            lines.push((phrase("kind", lang), set_kind_label(set.kind, lang), String::new(), String::new()));
        }
        if set.pieces > 0 {
            lines.push((phrase("set-size", lang), set.pieces.to_string(), String::new(), String::new()));
        }
        let slots = self.set_slot_line(code, lang);
        if !slots.is_empty() {
            lines.push((phrase("slots", lang), slots, String::new(), String::new()));
        }
        lines.extend(self.set_breakdown(code, lang));
        Some((
            set_title_lang(lang, &set.ko, &set.en, &set.zh),
            trail3(lang, &set.en, &set.zh, &set.ko),
            format!("#{code}"),
            lines,
            String::new(),
        ))
    }

    pub(in crate::db) fn open_set(&self, code: i64, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let Some((name, alt, id, lines, _)) = self.set_sheet(code, lang) else {
            return (phrase("missing-set", lang), Vec::new(), None);
        };
        let mut text = name.clone();
        if !alt.is_empty() {
            text.push_str(&format!("\n{alt}"));
        }
        text.push_str(&format!("\n{id}"));
        for (label, value, _, _) in &lines {
            text.push_str(&format!("\n{label}  {value}"));
        }
        let icon = self.index.sets.iter().find(|set| set.code == code).and_then(|set| set.icon.clone());
        (text, self.set_piece_hits(code, lang), icon)
    }

    pub(in crate::db) fn set_member_slots(&self, code: i64) -> Vec<(String, i64, i64)> {
        if !self.has_table("item_set_members") {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT m.item_code, m.enhance, IFNULL(i.category, ''), IFNULL(i.name_ko, ''),
                    IFNULL(i.name_gts, 0), IFNULL(i.large_image, '')
             FROM item_set_members m
             LEFT JOIN items i ON i.code = m.item_code
             WHERE m.set_code = ?1",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let mut rows = stmt
            .query_map([code], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        rows.sort_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));
        let extras = load_set_extra();
        let mut grouped: HashMap<String, Vec<(i64, i64, bool, bool)>> = HashMap::new();
        for (item, enhance, category, name, name_gts, image) in rows {
            let slot = piece_slot(&category, &name);
            let shell = name_gts <= 0 && image.is_empty();
            let extra = set_extra_name(&name);
            grouped.entry(slot).or_default().push((item, enhance, shell, extra));
        }
        let mut kept = Vec::new();
        for (slot, items) in grouped {
            let named = items.iter().any(|(_, _, shell, extra)| !*shell && !*extra);
            for (item, enhance, shell, extra) in items {
                if extra && !extras {
                    continue;
                }
                if shell && named {
                    continue;
                }
                kept.push((slot.clone(), item, enhance));
            }
        }
        kept.sort_by(|left, right| set_slot_order(&left.0).cmp(&set_slot_order(&right.0)).then(left.2.cmp(&right.2)).then(left.1.cmp(&right.1)));
        kept
    }

    pub(in crate::db) fn set_slot_line(&self, code: i64, lang: &str) -> String {
        let mut slots: Vec<String> = self.set_member_slots(code).into_iter().map(|(slot, _, _)| slot).collect();
        slots.sort_by_key(|slot| set_slot_order(slot));
        slots.dedup();
        slots
            .into_iter()
            .map(|slot| set_slot_label(&slot, lang))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub(in crate::db) fn set_piece_hits(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut grouped: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
        for (slot, item, enhance) in self.set_member_slots(code) {
            grouped.entry(slot).or_default().push((item, enhance));
        }
        let mut slots: Vec<String> = grouped.keys().cloned().collect();
        slots.sort_by_key(|slot| set_slot_order(slot));
        let mut hits = Vec::new();
        for slot in slots {
            let Some(items) = grouped.remove(&slot) else { continue };
            hits.push(Hit {
                title: set_slot_label(&slot, lang),
                subtitle: items.len().to_string(),
                key: format!("section:{slot}"),
                icon: items.first().and_then(|(item, _)| self.item_icon(*item)),
                solo: false,
                aside: String::new(),
                badge: None,
                chip: "section".into(),
            });
            for (item, enhance) in items {
                hits.push(Hit {
                    title: self.item_name(item, lang),
                    subtitle: format!("#{item}"),
                    key: format!("item:{item}"),
                    icon: self.item_icon(item),
                    solo: false,
                    aside: if enhance > 0 { format!("+{enhance}") } else { String::new() },
                    badge: None,
                    chip: String::new(),
                });
            }
        }
        hits
    }

    pub(in crate::db) fn enhance_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        if !self.has_table("enhance") {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT plus, dmin, dmax, move, atk_speed, magic_resist, hit_resist, element, element_damage,
                    attack_range, skill_range, crit, mana, sight, air, ground, cost,
                    merc_def, merc_atk, spell, reflect, invuln
             FROM enhance
             WHERE base_key = (SELECT base_key FROM enhance WHERE item_code = ?1 LIMIT 1)
             ORDER BY plus, item_code",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt
            .query_map([code], |row| {
                Ok(EnhanceRow {
                    plus: row.get(0)?,
                    dmin: row.get(1)?,
                    dmax: row.get(2)?,
                    move_speed: row.get(3)?,
                    atk_speed: row.get(4)?,
                    magic_resist: row.get(5)?,
                    hit_resist: row.get(6)?,
                    element: row.get(7)?,
                    element_damage: row.get(8)?,
                    attack_range: row.get(9)?,
                    skill_range: row.get(10)?,
                    crit: row.get(11)?,
                    mana: row.get(12)?,
                    sight: row.get(13)?,
                    air: row.get(14)?,
                    ground: row.get(15)?,
                    cost: row.get(16)?,
                    merc_def: row.get(17)?,
                    merc_atk: row.get(18)?,
                    spell: row.get(19)?,
                    reflect: row.get(20)?,
                    invuln: row.get(21)?,
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        if rows.is_empty() || rows.iter().all(|row| row.is_blank()) {
            return Vec::new();
        }
        rows.into_iter()
            .enumerate()
            .map(|(index, row)| {
                let label = if index == 0 { phrase("enhance", lang) } else { String::new() };
                (label, row.summary(lang))
            })
            .collect()
    }

    pub(in crate::db) fn item_droppers(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT m.code, m.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''), MIN(d.slot),
                        IFNULL(pe.text, ''), IFNULL(pz.text, '')
                 FROM monster_drops d
                 JOIN monsters m ON m.code = d.monster_code
                 LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                    AND m.name_gts > 0 AND se.id = printf('%08d', m.name_gts)
                 LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                    AND m.name_gts > 0 AND sz.id = printf('%08d', m.name_gts)
                 LEFT JOIN monster_locations ml ON ml.monster_code = m.code
                    AND ml.location_gts = (
                        SELECT MIN(location_gts) FROM monster_locations WHERE monster_code = m.code
                    )
                 LEFT JOIN strings pe ON pe.object = 'OnlineText.dat' AND pe.lang = 'en'
                    AND pe.id = printf('%08d', ml.location_gts)
                 LEFT JOIN strings pz ON pz.object = 'OnlineText.dat' AND pz.lang = 'zh'
                    AND pz.id = printf('%08d', ml.location_gts)
                 WHERE d.item_code = ?1
                 GROUP BY m.code",
            )
            .expect("item droppers");
        let mut rows: Vec<Hit> = stmt
            .query_map([code], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .expect("dropper rows")
            .filter_map(Result::ok)
            .map(|(id, ko, en, zh, slot, place_en, place_zh)| {
                let solo = slot >= 9;
                let place = lead(lang, &place_en, &place_zh);
                let mut meta = if place.is_empty() { id.clone() } else { format!("{place}  {id}") };
                if solo {
                    meta.push_str(&format!("  {}", phrase("solo-drop", lang)));
                }
                Hit {
                    title: lead3(lang, &en, &zh, &ko),
                    subtitle: caption(&trail3(lang, &en, &zh, &ko), meta),
                    key: format!("monster:{id}"),
                    icon: self.sprite(&id),
                    solo,
                    aside: String::new(),
                badge: None,
                    chip: String::new(),
                }
            })
            .collect();
        rows.sort_by(|a, b| a.title.cmp(&b.title));
        rows
    }

    pub(in crate::db) fn item_sellers(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        for town in &self.index.towns {
            let Some(good) = town.goods.iter().find(|good| good.item == code) else { continue };
            hits.push(Hit {
                title: lead3(lang, &town.en, &town.zh, &town.ko),
                subtitle: caption(&trail3(lang, &town.en, &town.zh, &town.ko), self.realm_name(&town.region, lang)),
                key: format!("town:{}", town.code),
                icon: None,
                solo: false,
                aside: if good.specialty { phrase("specialty", lang) } else { String::new() },
                badge: None,
                chip: String::new(),
            });
        }
        hits.sort_by(|a, b| b.aside.len().cmp(&a.aside.len()).then(a.title.cmp(&b.title)));
        hits
    }

    pub(in crate::db) fn enchant_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        if !self.has_table("enchant_recipes") {
            return Vec::new();
        }
        let Ok((rate, level, key)) = self.conn.query_row(
            "SELECT success_rate, level, recipe_key FROM enchant_recipes WHERE item_code = ?1",
            [code],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)),
        ) else {
            return Vec::new();
        };
        let mut lines = vec![
            (phrase("success", lang), format!("{}%", commas(rate))),
            (phrase("min-level", lang), commas(level)),
        ];
        let gear = self.enchant_gear(code, lang);
        if !gear.is_empty() {
            let separator = if primary_lang(lang) == "zh" { "、" } else { ", " };
            lines.push((phrase("enchant-gear", lang), gear.join(separator)));
        }
        for (item, qty) in self.enchant_materials(&key) {
            lines.push((phrase("materials", lang), format!("{} × {}", self.item_name(item, lang), commas(qty))));
        }
        lines
    }

    fn enchant_gear(&self, code: i64, lang: &str) -> Vec<String> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT ty.label_gts, ty.item_code
             FROM enchant_recipe_targets t
             JOIN enchant_target_types ty ON ty.token = t.token
             WHERE t.item_code = ?1
             ORDER BY t.slot",
        ) else {
            return Vec::new();
        };
        stmt.query_map([code], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .map(|rows| {
                rows.filter_map(Result::ok)
                    .map(|(text_id, item)| {
                        if text_id > 0 {
                            let en = self.online_line(text_id, "en");
                            let zh = self.online_line(text_id, "zh");
                            let shown = lead(lang, &en, &zh);
                            if !shown.is_empty() {
                                return shown;
                            }
                        }
                        if item > 0 {
                            return self.item_name(item, lang);
                        }
                        String::new()
                    })
                    .filter(|name| !name.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn enchant_materials(&self, key: &str) -> Vec<(i64, i64)> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT item_code, qty FROM enchant_materials WHERE recipe_key = ?1 ORDER BY slot",
        ) else {
            return Vec::new();
        };
        stmt.query_map([key], |row| Ok((row.get(0)?, row.get(1)?)))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    fn enchant_material_hits(&self, code: i64, lang: &str) -> Vec<Hit> {
        if !self.has_table("enchant_recipes") {
            return Vec::new();
        }
        let Ok(key) = self.conn.query_row(
            "SELECT recipe_key FROM enchant_recipes WHERE item_code = ?1",
            [code],
            |row| row.get::<_, String>(0),
        ) else {
            return Vec::new();
        };
        self.enchant_materials(&key)
            .into_iter()
            .map(|(item, qty)| self.enchant_hit(item, format!("× {}", commas(qty)), lang))
            .collect()
    }

    fn enchant_scroll_hits(&self, code: i64, lang: &str) -> Vec<Hit> {
        if !self.has_table("enchant_materials") {
            return Vec::new();
        }
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT r.item_code, m.qty
             FROM enchant_materials m
             JOIN enchant_recipes r ON r.recipe_key = m.recipe_key
             WHERE m.item_code = ?1
             ORDER BY r.level, r.item_code",
        ) else {
            return Vec::new();
        };
        stmt.query_map([code], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .map(|rows| {
                rows.filter_map(Result::ok)
                    .map(|(item, qty)| self.enchant_hit(item, format!("× {}", commas(qty)), lang))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn enchant_hit(&self, code: i64, subtitle: String, lang: &str) -> Hit {
        Hit {
            title: self.item_name(code, lang),
            subtitle,
            key: format!("item:{code}"),
            icon: self.item_icon(code),
            solo: false,
            badge: None,
            aside: String::new(),
            chip: String::new(),
        }
    }

    pub(in crate::db) fn item_relations(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        self.push_section(&mut hits, lang, "sold-in", self.item_sellers(code, lang));
        self.push_section(&mut hits, lang, "dropped-by", self.item_droppers(code, lang));
        push_rows(&mut hits, lang, "enchant-need", self.enchant_material_hits(code, lang));
        push_rows(&mut hits, lang, "enchant-for", self.enchant_scroll_hits(code, lang));
        self.push_section(&mut hits, lang, "craft-how", self.recipes_for(code, lang));
        self.push_section(&mut hits, lang, "make", self.production_for(code, lang));
        let mut uses = self.item_quest_uses(code, lang);
        uses.extend(self.recipes_using(code, lang));
        uses.extend(self.formulas_using(code, lang));
        self.push_section(&mut hits, lang, "used-for", uses);
        self.push_section(&mut hits, lang, "also-from", self.item_quest_gains(code, lang));
        self.push_section(&mut hits, lang, "set", self.item_set_links(code, lang));
        self.push_section(&mut hits, lang, "roster", self.index.roster_hits(code, lang));
        hits
    }

    pub(in crate::db) fn item_pair(&self, code: i64) -> (String, String) {
        self.conn
            .query_row(
                "SELECT IFNULL(se.text, ''), IFNULL(sz.text, '')
                 FROM items i
                 LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                    AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
                 LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                    AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
                 WHERE i.code = ?1",
                [code],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap_or_default()
    }

    pub(in crate::db) fn item_names3(&self, code: i64) -> (String, String, String) {
        if let Some(item) = self.index.find_item(code) {
            return (item.en.clone(), item.zh.clone(), item.ko.clone());
        }
        let (en, zh) = self.item_pair(code);
        let ko = self.conn.query_row("SELECT name_ko FROM items WHERE code = ?1", [code], |row| row.get(0)).unwrap_or_default();
        (en, zh, ko)
    }

    pub(in crate::db) fn item_name(&self, code: i64, lang: &str) -> String {
        self.item_name_linked(code, lang, true)
    }

    pub(in crate::db) fn item_name_linked(&self, code: i64, lang: &str, follow_base: bool) -> String {
        let found = self.conn.query_row(
            "SELECT i.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, '')
             FROM items i
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE i.code = ?1",
            [code],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        );
        let Ok((ko, en, zh)) = found else {
            return code.to_string();
        };
        let shown = if primary_lang(lang) == "ko" { lead3(lang, &en, &zh, &ko) } else { lead(lang, &en, &zh) };
        if !shown.is_empty() {
            return shown;
        }
        if follow_base {
            if let Some((base, prefix_gts)) = self.base_link(code) {
                let base_name = self.item_name_linked(base, lang, false);
                let base_ko = self.conn
                    .query_row("SELECT name_ko FROM items WHERE code = ?1", [base], |row| row.get::<_, String>(0))
                    .unwrap_or_default();
                let prefix = self.prefix_name(prefix_gts, &ko, &base_ko, lang);
                if !base_name.is_empty() {
                    return if prefix.is_empty() { base_name } else { format!("{prefix} {base_name}") };
                }
            }
        }
        if ko.trim().is_empty() || ko.trim() == "0" { code.to_string() } else { ko }
    }

    pub(in crate::db) fn base_link(&self, code: i64) -> Option<(i64, i64)> {
        if !self.has_table("item_bases") {
            return None;
        }
        let prefixed = self.conn.query_row(
            "SELECT 1 FROM pragma_table_info('item_bases') WHERE name = 'prefix_gts'",
            [],
            |row| row.get::<_, i64>(0),
        ).is_ok();
        if prefixed {
            self.conn
                .query_row(
                    "SELECT base_code, IFNULL(prefix_gts, 0) FROM item_bases WHERE item_code = ?1",
                    [code],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .ok()
        } else {
            self.conn
                .query_row("SELECT base_code FROM item_bases WHERE item_code = ?1", [code], |row| {
                    Ok((row.get::<_, i64>(0)?, 0))
                })
                .ok()
        }
    }

    pub(in crate::db) fn prefix_name(&self, prefix_gts: i64, ko: &str, base_ko: &str, lang: &str) -> String {
        if prefix_gts > 0 {
            let en = self.online_line(prefix_gts, "en");
            let zh = self.online_line(prefix_gts, "zh");
            let shown = lead(lang, &en, &zh);
            if !shown.is_empty() {
                return shown;
            }
        }
        piece_prefix(ko, base_ko, lang)
    }

    pub(in crate::db) fn item_icon(&self, code: i64) -> Option<(PathBuf, i32)> {
        self.index.item_icons.get(&code).cloned()
    }

    pub fn linked_icon(&self, key: &str) -> Option<(PathBuf, i32)> {
        if let Some(code) = key.strip_prefix("monster:") {
            return self.sprite(code);
        }
        if let Some(code) = key.strip_prefix("item:") {
            return code.parse().ok().and_then(|id| self.item_icon(id));
        }
        None
    }

    pub(in crate::db) fn icon_path(&self, image: &str, frame: i64) -> Option<(PathBuf, i32)> {
        if image.is_empty() || frame < 0 {
            return None;
        }
        let folder = self.client_root.join("Assets").join("Textures").join("Items");
        let wanted = Path::new(image).file_name()?.to_string_lossy().to_lowercase();
        let entry = std::fs::read_dir(&folder).ok()?.filter_map(Result::ok).find(|entry| {
            entry.file_name().to_string_lossy().to_lowercase() == wanted
        })?;
        Some((entry.path(), frame as i32))
    }
}

fn push_rows(hits: &mut Vec<Hit>, lang: &str, key: &str, mut rows: Vec<Hit>) {
    if rows.is_empty() {
        return;
    }
    hits.push(Hit {
        title: phrase(key, lang),
        subtitle: rows.len().to_string(),
        key: format!("section:{key}"),
        icon: None,
        solo: false,
        badge: None,
        aside: String::new(),
        chip: "section".into(),
    });
    for row in &mut rows {
        if row.chip.is_empty() {
            row.chip = key.to_string();
        }
    }
    hits.extend(rows);
}

#[cfg(test)]
mod language_tests {
    use super::*;

    #[test]
    fn item_sheet_and_clipboard_use_korean_source_and_optional_secondary() {
        let catalog = Catalog::open().expect("catalog");
        let item = catalog.index.items.iter().find(|item| !item.en.is_empty() && !item.zh.is_empty() && !item.ko.is_empty() && item.ko != "0").expect("trilingual item");
        let key = format!("item:{}", item.code);
        for lang in ["en", "zh", "ko", "en-zh", "en-ko", "zh-en", "zh-ko", "ko-en", "ko-zh"] {
            let (name, alternate, _, _, _) = catalog.item_sheet(item.code, lang).expect("sheet");
            assert_eq!(catalog.copyable_name(&key, lang, false).as_deref(), Some(name.as_str()));
            assert_eq!(catalog.copyable_name(&key, lang, true), (!alternate.is_empty()).then_some(alternate));
        }
        assert_eq!(catalog.item_sheet(item.code, "ko").unwrap().0, item.ko.trim());
        assert!(catalog.item_sheet(item.code, "ko").unwrap().1.is_empty());
    }

    #[test]
    fn korean_descriptions_work_without_a_translation_id_and_omit_placeholders() {
        let catalog = Catalog::open().expect("catalog");
        assert_eq!(catalog.item_note(0, "한국어 설명", "ko"), "한국어 설명");
        assert!(catalog.item_note(0, "0", "ko").is_empty());
        let (code, description): (i64, String) = catalog.conn.query_row(
            "SELECT code, description FROM items WHERE description NOT IN ('', '0') LIMIT 1", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(catalog.item_sheet(code, "ko").unwrap().4, description.trim());
    }
}
