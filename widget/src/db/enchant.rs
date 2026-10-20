//! Client-backed enchant browsing. Zero/missing scroll references stay explicit.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use super::item::push_rows;
use super::language::primary_lang;
use super::text::{commas, lead3, phrase, trail3};
use super::{Catalog, Hit};

type ItemSheet = (String, String, String, Vec<(String, String)>, String);

fn wording(lang: &str, zh: &str, en: &str, ko: &str) -> String {
    match primary_lang(lang) { "zh" => zh, "ko" => ko, _ => en }.to_string()
}

fn search_query(query: &str) -> &str {
    let query = query.trim();
    query.strip_prefix('#').filter(|code| code.parse::<i64>().is_ok()).unwrap_or(query)
}

// RecipeItem's inventory frames define visual colors, not enchant levels.
// Do not infer them from required level or the material recipe key:
// e.g. 1222 uses a 4Cls key with frame 2, while 11636 uses 6Cls with frame 3.
// Client RecipeItem 3096/3097/3098 are explicitly [3주년1/2/3] anniversary
// scrolls. They share frame 2, so the event exception must take precedence.
const EVENT_SCROLLS: [i64; 3] = [3096, 3097, 3098];
const COLOR_SECTIONS: [(&str, i64, &str, &str, &str); 7] = [
    ("blue", 1127, "藍色", "Blue", "파란색"),
    ("green", 1161, "綠色", "Green", "초록색"),
    ("red", 1222, "赤色", "Red", "빨간색"),
    ("yellow", 1128, "黃色", "Yellow", "노란색"),
    ("brown", 6914, "褐色", "Brown", "갈색"),
    ("other", 8165, "其他", "Other", "기타"),
    ("event", 3096, "活動", "Event", "이벤트"),
];

fn scroll_group(code: i64, image: &str, frame: i64) -> &'static str {
    if EVENT_SCROLLS.contains(&code) { return "event"; }
    let filename = image.rsplit(['/', '\\']).next().unwrap_or(image);
    if filename.eq_ignore_ascii_case("enchantrecipe01_i.agf") {
        match frame { 0 => "blue", 1 => "green", 2 => "red", 3 => "yellow", 4 => "brown", _ => "other" }
    } else { "other" }
}

struct Scroll {
    code: i64,
    rate: i64,
    level: i64,
    group: &'static str,
    has_equipment: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnchantScrollInfo {
    pub level: i64,
    pub rate: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnchantAbilityPreview {
    pub summary: String,
    pub recorded: bool,
}

// Shared by previews and sorting so both use the same verified relationships.
// label_gts is a text ID; only validated target item_code references are items.
const APPLICABLE_EQUIPMENT_SQL: &str = "SELECT scroll_item, base_item FROM enchant_equipment
    WHERE scroll_status='linked' AND base_item>0
    UNION
    SELECT t.item_code, ty.item_code FROM enchant_recipe_targets t
        JOIN enchant_target_types ty ON ty.token=t.token WHERE ty.item_code>0";

#[derive(Clone)]
struct Equipment {
    result: i64,
    base: i64,
    scroll: i64,
    revert: i64,
    jackpot: i64,
    source_name: String,
    base_ko: String,
    prefix_ko: String,
    prefix_en: String,
    prefix_zh: String,
    name_en: String,
    name_zh: String,
    effect: String,
    status: String,
    sprite: String,
    frame: i64,
}

const EQUIPMENT_SQL: &str = "SELECT e.result_item, e.base_item, e.scroll_item, e.revert_scroll_item,
    e.jackpot_item, e.result_name_ko, e.base_name_ko, e.prefix_name_ko,
    COALESCE(pe.text, ''), COALESCE(pz.text, ''), COALESCE(ne.text, ''), COALESCE(nz.text, ''),
    e.effect_key, e.scroll_status, e.inventory_sprite, e.inventory_frame
    FROM enchant_equipment e
    LEFT JOIN strings pe ON pe.object='OnlineText.dat' AND pe.lang='en' AND pe.id=printf('%08d',e.prefix_gts) AND e.prefix_gts>0
    LEFT JOIN strings pz ON pz.object='OnlineText.dat' AND pz.lang='zh' AND pz.id=printf('%08d',e.prefix_gts) AND e.prefix_gts>0
    LEFT JOIN strings ne ON ne.object='OnlineText.dat' AND ne.lang='en' AND ne.id=printf('%08d',e.name_gts) AND e.name_gts>0
    LEFT JOIN strings nz ON nz.object='OnlineText.dat' AND nz.lang='zh' AND nz.id=printf('%08d',e.name_gts) AND e.name_gts>0";

fn equipment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Equipment> {
    Ok(Equipment {
        result: row.get(0)?, base: row.get(1)?, scroll: row.get(2)?, revert: row.get(3)?,
        jackpot: row.get(4)?, source_name: row.get(5)?, base_ko: row.get(6)?, prefix_ko: row.get(7)?,
        prefix_en: row.get(8)?, prefix_zh: row.get(9)?, name_en: row.get(10)?, name_zh: row.get(11)?,
        effect: row.get(12)?, status: row.get(13)?, sprite: row.get(14)?, frame: row.get(15)?,
    })
}

impl Catalog {
    pub fn enchant_success_label(&self, lang: &str) -> String {
        phrase("success", lang)
    }

    /// Raw client values for scroll displays; localized text is not a data API.
    pub fn enchant_scroll_info(&self, codes: &[i64]) -> HashMap<i64, EnchantScrollInfo> {
        let wanted: HashSet<_> = codes.iter().copied().collect();
        if wanted.is_empty() { return HashMap::new(); }
        let Ok(mut stmt) = self.conn.prepare("SELECT item_code, level, success_rate FROM enchant_recipes")
            else { return HashMap::new(); };
        stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?,
            EnchantScrollInfo { level: row.get(1)?, rate: row.get(2)? })))
            .map(|rows| rows.filter_map(Result::ok).filter(|(code, _)| wanted.contains(code)).collect())
            .unwrap_or_default()
    }

    fn enchant_scroll_rows(&self) -> Vec<Scroll> {
        let sql = format!("WITH targets AS ({APPLICABLE_EQUIPMENT_SQL})
            SELECT r.item_code, r.success_rate, r.level,
                COALESCE(i.large_image,''), COALESCE(i.large_frame,-1), target.scroll_item IS NOT NULL
            FROM enchant_recipes r LEFT JOIN items i ON i.code=r.item_code
            LEFT JOIN (SELECT DISTINCT scroll_item FROM targets) target ON target.scroll_item=r.item_code");
        let Ok(mut stmt) = self.conn.prepare(&sql) else { return Vec::new() };
        let mut rows: Vec<_> = stmt.query_map([], |row| {
            let code = row.get(0)?;
            let image: String = row.get(3)?;
            Ok(Scroll { code, rate: row.get(1)?, level: row.get(2)?,
                group: scroll_group(code, &image, row.get(4)?), has_equipment: row.get(5)? })
        }).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default();
        rows.sort_by_key(|row| (!row.has_equipment, row.level, row.code));
        rows
    }

    fn enchant_equipment_rows(&self) -> Vec<Equipment> {
        let Ok(mut stmt) = self.conn.prepare(&format!("{EQUIPMENT_SQL} ORDER BY e.result_item")) else { return Vec::new() };
        stmt.query_map([], equipment_row).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
    }

    fn enchant_equipment_row(&self, code: i64) -> Option<Equipment> {
        self.conn.query_row(&format!("{EQUIPMENT_SQL} WHERE e.result_item=?1"), [code], equipment_row).ok()
    }

    fn enchant_equipment_for(&self, code: i64, include_base: bool) -> Vec<Equipment> {
        let sql = format!("{EQUIPMENT_SQL} WHERE e.result_item=?1 OR (e.scroll_item=?1 AND e.scroll_status='linked')
            OR (?2=1 AND (e.base_item=?1 OR e.jackpot_item=?1 OR e.revert_scroll_item=?1)) ORDER BY e.result_item");
        let Ok(mut stmt) = self.conn.prepare(&sql) else { return Vec::new() };
        stmt.query_map((code, include_base), equipment_row)
            .map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
    }

    fn enchant_names(&self, row: &Equipment) -> (String, String, String) {
        let item = self.index.find_item(row.result);
        let base = self.index.find_item(row.base);
        let combine = |prefix: &str, base: &str| {
            let prefix = prefix.trim().trim_matches(['<', '>']);
            if base.is_empty() { String::new() } else if prefix.is_empty() { base.to_string() } else { format!("{prefix} {base}") }
        };
        let localized = |existing: &str, direct: &str, prefix: &str, base: &str| {
            if !existing.trim().is_empty() { existing.to_string() }
            else if !direct.trim().is_empty() { direct.to_string() }
            else { combine(prefix, base) }
        };
        let ko = item.map(|i| i.ko.clone()).unwrap_or_else(|| {
            if row.source_name.contains(&row.base_ko) { row.source_name.clone() }
            else { combine(&row.prefix_ko, &row.base_ko) }
        });
        (
            localized(item.map(|i| i.en.as_str()).unwrap_or(""), &row.name_en, &row.prefix_en, base.map(|i| i.en.as_str()).unwrap_or("")),
            localized(item.map(|i| i.zh.as_str()).unwrap_or(""), &row.name_zh, &row.prefix_zh, base.map(|i| i.zh.as_str()).unwrap_or("")),
            if ko.is_empty() { row.source_name.clone() } else { ko },
        )
    }

    pub(super) fn enchant_source_names(&self, code: i64) -> Option<(String, String, String)> {
        self.enchant_equipment_row(code).map(|row| self.enchant_names(&row))
    }

    fn enchant_icon_for(&self, row: &Equipment) -> Option<(PathBuf, i32)> {
        self.index.item_icons.get(&row.result).cloned()
            .or_else(|| self.icon_path(&row.sprite, row.frame))
            .or_else(|| self.index.item_icons.get(&row.base).cloned())
    }

    pub(super) fn enchant_source_icon(&self, code: i64) -> Option<(PathBuf, i32)> {
        self.enchant_equipment_row(code).and_then(|row| self.enchant_icon_for(&row))
    }

    fn enchant_result_hit(&self, row: &Equipment, lang: &str) -> Hit {
        let (en, zh, ko) = self.enchant_names(row);
        let status = if row.status == "no_scroll" {
            wording(lang, "客戶端未指定秘訣", "No scroll specified by client", "클라이언트 비법 미지정")
        } else if row.status != "linked" {
            wording(lang, "秘訣資料缺失", "Scroll record missing", "비법 자료 없음")
        } else {
            self.index.find_item(row.base).map(|item| lead3(lang, &item.en, &item.zh, &item.ko))
                .unwrap_or_else(|| row.base_ko.clone())
        };
        Hit {
            title: lead3(lang, &en, &zh, &ko), subtitle: status,
            key: format!("item:{}", row.result), icon: self.enchant_icon_for(row), solo: false,
            badge: None, aside: if self.index.find_item(row.result).is_none() {
                wording(lang, "客戶端記錄", "Client record", "클라이언트 기록")
            } else { String::new() }, chip: String::new(),
        }
    }

    pub fn enchant_sections(&self, lang: &str) -> Vec<Hit> {
        [("scrolls", "enchant_recipes", "鬼道秘訣", "Enchant scrolls", "주술 비법"),
         ("equipment", "enchant_equipment", "鬼道裝備", "Enchanted equipment", "주술 장비")]
            .into_iter().filter_map(|(section, table, zh, en, ko)| {
                let count: i64 = self.conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0)).ok()?;
                Some(Hit { title: wording(lang, zh, en, ko), subtitle: commas(count), key: format!("ecat:{section}"),
                    icon: None, solo: false, badge: None, aside: String::new(), chip: String::new() })
            }).collect()
    }

    pub fn enchant_color_sections(&self, lang: &str) -> Vec<Hit> {
        let mut counts: HashMap<&str, i64> = HashMap::new();
        for row in self.enchant_scroll_rows() { *counts.entry(row.group).or_default() += 1; }
        COLOR_SECTIONS.into_iter().map(|(section, representative, zh, en, ko)| Hit {
            title: wording(lang, zh, en, ko), subtitle: commas(counts.get(section).copied().unwrap_or(0)),
            key: format!("ecat:{section}"), icon: self.item_icon(representative), solo: false,
            badge: None, aside: String::new(), chip: String::new(),
        }).collect()
    }

    /// Batch previews of explicitly supported base equipment. Every requested
    /// scroll has an entry, even when its source only names a category such as
    /// ALL. Non-scroll codes are omitted so callers can identify scroll rows.
    pub fn enchant_applicable_equipment(&self, codes: &[i64], lang: &str) -> HashMap<i64, Vec<Hit>> {
        let wanted: HashSet<_> = codes.iter().copied().collect();
        let mut applicable: HashMap<i64, Vec<Hit>> = HashMap::new();
        if wanted.is_empty() { return applicable; }
        // Target label_gts values are text IDs, not item IDs. The importer only
        // sets item_code for verified, same-name equipment references (the five
        // S_*_GEM targets in this client). Never expand ALL/type/level tokens.
        // UNION deduplicates targets also present in the explicit result table.
        let sql = format!("SELECT r.item_code, target.base_item
            FROM enchant_recipes r LEFT JOIN ({APPLICABLE_EQUIPMENT_SQL}) target ON target.scroll_item=r.item_code
            ORDER BY r.item_code, target.base_item");
        let Ok(mut stmt) = self.conn.prepare(&sql) else { return applicable; };
        let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)))
            else { return applicable; };
        let mut cached: HashMap<i64, Hit> = HashMap::new();
        for (scroll, base) in rows.filter_map(Result::ok) {
            if !wanted.contains(&scroll) { continue; }
            let hits = applicable.entry(scroll).or_default();
            let Some(base) = base else { continue; };
            let hit = cached.entry(base).or_insert_with(|| self.enchant_preview_item(base, lang));
            hits.push(hit.clone());
        }
        applicable
    }

    /// Client material slots, with quantities kept separate from item names.
    /// Requested scrolls with no material rows remain present with an empty list.
    pub fn enchant_preview_materials(&self, codes: &[i64], lang: &str) -> HashMap<i64, Vec<Hit>> {
        let wanted: HashSet<_> = codes.iter().copied().collect();
        let mut materials: HashMap<i64, Vec<Hit>> = HashMap::new();
        if wanted.is_empty() { return materials; }
        let Ok(mut stmt) = self.conn.prepare("SELECT r.item_code, m.item_code, m.qty
            FROM enchant_recipes r LEFT JOIN enchant_materials m ON m.recipe_key=r.recipe_key
            ORDER BY r.item_code, m.slot") else { return materials; };
        let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?,
            row.get::<_, Option<i64>>(1)?, row.get::<_, Option<i64>>(2)?))) else { return materials; };
        let mut cached: HashMap<i64, Hit> = HashMap::new();
        for (scroll, material, qty) in rows.filter_map(Result::ok) {
            if !wanted.contains(&scroll) { continue; }
            let hits = materials.entry(scroll).or_default();
            let (Some(material), Some(qty)) = (material, qty) else { continue; };
            let mut hit = cached.entry(material).or_insert_with(|| self.enchant_preview_item(material, lang)).clone();
            hit.subtitle = format!("× {}", commas(qty));
            hits.push(hit);
        }
        materials
    }

    /// Bonuses come only from explicit scroll/result/effect links. An existing
    /// all-zero profile is recorded; an absent link or profile remains unknown.
    pub fn enchant_preview_abilities(&self, codes: &[i64], lang: &str) -> HashMap<i64, EnchantAbilityPreview> {
        let missing = wording(lang, "未記錄", "Not recorded", "기록 없음");
        let mut previews: HashMap<_, _> = self.enchant_scroll_info(codes).into_keys()
            .map(|code| (code, EnchantAbilityPreview { summary: missing.clone(), recorded: false })).collect();
        if previews.is_empty() { return previews; }
        let Ok(mut stmt) = self.conn.prepare("SELECT DISTINCT scroll_item, effect_key, base_item
            FROM enchant_equipment WHERE scroll_status='linked' ORDER BY scroll_item, effect_key, base_item")
            else { return previews; };
        let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?, row.get::<_, i64>(2)?))) else { return previews; };
        let mut groups: HashMap<i64, BTreeMap<String, Vec<i64>>> = HashMap::new();
        for (scroll, effect, base) in rows.filter_map(Result::ok) {
            if previews.contains_key(&scroll) {
                groups.entry(scroll).or_default().entry(effect).or_default().push(base);
            }
        }
        if groups.is_empty() { return previews; }
        let needed: HashSet<_> = groups.values().flat_map(|profiles| profiles.keys().cloned()).collect();
        let mut bonuses_by_effect: HashMap<String, Vec<String>> = HashMap::new();
        if let Ok(mut stmt) = self.conn.prepare("SELECT * FROM enchant_effects") {
            if let Ok(rows) = stmt.query_map([], |row| {
                let key: String = row.get("effect_key")?;
                if !needed.contains(&key) { return Ok(None); }
                Ok(Some((key, effect_lines(row, lang)?.0)))
            }) {
                bonuses_by_effect.extend(rows.filter_map(Result::ok).flatten());
            }
        }
        let no_numeric = wording(lang, "來源未列數值加成", "No numeric bonuses listed", "원본에 수치 보너스 없음");
        let mut base_names: HashMap<i64, String> = HashMap::new();
        for (scroll, profiles) in groups {
            let multiple = profiles.len() > 1;
            let mut recorded = true;
            let mut summaries = Vec::new();
            for (effect, bases) in profiles {
                let summary = match bonuses_by_effect.get(&effect) {
                    Some(bonuses) if bonuses.is_empty() => no_numeric.clone(),
                    Some(bonuses) => bonuses.join(" · "),
                    None => { recorded = false; missing.clone() },
                };
                if multiple {
                    let mut seen = HashSet::new();
                    let names: Vec<_> = bases.into_iter().map(|base| base_names.entry(base).or_insert_with(|| {
                        self.index.find_item(base).map(|item| lead3(lang, &item.en, &item.zh, &item.ko))
                            .unwrap_or_else(|| self.item_name(base, lang))
                    }).clone()).filter(|name| seen.insert(name.clone())).collect();
                    summaries.push(format!("{}:\n{summary}", names.join(", ")));
                } else {
                    summaries.push(summary);
                }
            }
            previews.insert(scroll, EnchantAbilityPreview { summary: summaries.join("\n\n"), recorded });
        }
        previews
    }

    fn enchant_preview_item(&self, code: i64, lang: &str) -> Hit {
        if let Some(item) = self.index.find_item(code) {
            Hit {
                title: lead3(lang, &item.en, &item.zh, &item.ko), subtitle: String::new(),
                key: format!("item:{code}"), icon: self.item_icon(code), solo: false,
                badge: None, aside: String::new(), chip: String::new(),
            }
        } else {
            self.enchant_hit(code, String::new(), lang)
        }
    }

    /// Search scroll names and their explicit equipment/material/target links.
    pub fn enchant_hits(&self, query: &str, lang: &str) -> Vec<Hit> {
        let query = search_query(query);
        let folded = query.to_lowercase();
        self.index.prepare_aliases(&folded);
        let matching_items: HashSet<i64> = if query.is_empty() { HashSet::new() } else {
            self.index.items.iter().filter(|item| self.index.name_hit(query, &folded, &item.code_str, &item.names))
                .map(|item| item.code).collect()
        };
        let mut matched_scrolls = matching_items.clone();
        if !query.is_empty() {
            for equipment in self.enchant_equipment_rows() {
                if equipment.status != "linked" { continue; }
                let (en, zh, ko) = self.enchant_names(&equipment);
                if matching_items.contains(&equipment.base) || matching_items.contains(&equipment.result)
                    || [equipment.jackpot, equipment.revert].into_iter().any(|code| code > 0
                        && (matching_items.contains(&code) || code.to_string() == query))
                    || equipment.result.to_string() == query
                    || [&en, &zh, &ko, &equipment.source_name].iter().any(|text| text.to_lowercase().contains(&folded)) {
                    matched_scrolls.insert(equipment.scroll);
                }
            }
            if let Ok(mut stmt) = self.conn.prepare("SELECT r.item_code, m.item_code FROM enchant_recipes r JOIN enchant_materials m ON m.recipe_key=r.recipe_key") {
                if let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))) {
                    for (scroll, material) in rows.filter_map(Result::ok) {
                        if matching_items.contains(&material) { matched_scrolls.insert(scroll); }
                    }
                }
            }
            if let Ok(mut stmt) = self.conn.prepare("SELECT t.item_code, ty.item_code, ty.token, COALESCE(se.text,''), COALESCE(sz.text,'')
                FROM enchant_recipe_targets t JOIN enchant_target_types ty ON ty.token=t.token
                LEFT JOIN strings se ON se.object='OnlineText.dat' AND se.lang='en' AND se.id=printf('%08d',ty.label_gts)
                LEFT JOIN strings sz ON sz.object='OnlineText.dat' AND sz.lang='zh' AND sz.id=printf('%08d',ty.label_gts)") {
                if let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?))) {
                    for (scroll, item, token, en, zh) in rows.filter_map(Result::ok) {
                        if matching_items.contains(&item) || [&token, &en, &zh].iter().any(|text| text.to_lowercase().contains(&folded)) {
                            matched_scrolls.insert(scroll);
                        }
                    }
                }
            }
        }
        self.enchant_scroll_rows().into_iter().filter(|row| query.is_empty() || matched_scrolls.contains(&row.code))
            .map(|row| Hit { title: self.item_name(row.code, lang),
                subtitle: format!("{} {} · {} {}%", phrase("min-level", lang), commas(row.level), phrase("success", lang), row.rate),
                key: format!("item:{}", row.code), icon: self.item_icon(row.code), solo: false, badge: None,
                aside: String::new(), chip: String::new() }).collect()
    }

    pub fn enchant_records(&self, section: &str, query: &str, lang: &str) -> Vec<Hit> {
        let section = section.strip_prefix("ecat:").unwrap_or(section);
        if section == "scrolls" { return self.enchant_hits(query, lang); }
        if COLOR_SECTIONS.iter().any(|(key, _, _, _, _)| *key == section) {
            let codes: HashSet<_> = self.enchant_scroll_rows().into_iter().filter(|row| row.group == section)
                .map(|row| format!("item:{}", row.code)).collect();
            return self.enchant_hits(query, lang).into_iter().filter(|hit| codes.contains(&hit.key)).collect();
        }
        if !section.is_empty() && section != "equipment" { return Vec::new(); }
        let scrolls = self.enchant_hits(query, lang);
        let matching_scrolls: HashSet<i64> = scrolls.iter().filter_map(|hit| hit.key.strip_prefix("item:")?.parse().ok()).collect();
        let query = search_query(query);
        let folded = query.to_lowercase();
        let equipment = self.enchant_equipment_rows().into_iter().filter(|row| {
            if query.is_empty() { return true; }
            let (en, zh, ko) = self.enchant_names(row);
            [&en, &zh, &ko, &row.base_ko, &row.source_name].iter().any(|text| text.to_lowercase().contains(&folded))
                || row.result.to_string() == query || row.base.to_string() == query
                || [row.jackpot, row.revert].into_iter().any(|code| code > 0
                    && (code.to_string() == query || self.index.find_item(code)
                        .is_some_and(|item| self.index.name_hit(query, &folded, &item.code_str, &item.names))))
                || (row.status == "linked" && matching_scrolls.contains(&row.scroll))
                || self.index.find_item(row.base).is_some_and(|item| self.index.name_hit(query, &folded, &item.code_str, &item.names))
        }).map(|row| self.enchant_result_hit(&row, lang));
        let mut hits = if section.is_empty() { scrolls } else { Vec::new() };
        hits.extend(equipment);
        hits
    }

    pub(super) fn enchant_equipment_relations(&self, code: i64, lang: &str) -> Vec<Hit> {
        let equipment = self.enchant_equipment_for(code, true);
        let mut hits = Vec::new();
        push_rows(&mut hits, lang, "enchant-need", self.enchant_material_hits(code, lang));
        let produced: Vec<_> = equipment.iter().filter(|row| row.scroll == code && row.status == "linked").collect();
        let mut base_codes = HashSet::new();
        let bases = produced.iter().filter(|row| base_codes.insert(row.base)).map(|row| self.enchant_hit(row.base, String::new(), lang)).collect();
        push_rows(&mut hits, lang, "enchant-target", bases);
        let variants: Vec<_> = equipment.iter().filter(|row| row.base == code || row.jackpot == code || row.revert == code
            || (row.scroll == code && row.status == "linked"))
            .map(|row| self.enchant_result_hit(row, lang)).collect();
        push_rows(&mut hits, lang, "enchant-result", variants);
        let mut available = self.enchant_scroll_hits(code, lang);
        let mut scrolls: HashSet<String> = available.iter().map(|hit| hit.key.clone()).collect();
        available.extend(equipment.iter().filter(|row| (row.base == code || row.result == code) && row.status == "linked"
            && self.index.find_item(row.scroll).is_some()).filter(|row| scrolls.insert(format!("item:{}", row.scroll)))
            .map(|row| self.enchant_hit(row.scroll, String::new(), lang)));
        push_rows(&mut hits, lang, "enchant-for", available);
        if let Some(row) = equipment.iter().find(|row| row.result == code) {
            push_rows(&mut hits, lang, "enchant-base", vec![self.enchant_hit(row.base, String::new(), lang)]);
            if row.jackpot > 0 {
                push_rows(&mut hits, lang, "enchant-jackpot", vec![self.enchant_hit(row.jackpot, String::new(), lang)]);
            }
            if row.revert > 0 {
                push_rows(&mut hits, lang, "enchant-revert", vec![self.enchant_hit(row.revert, String::new(), lang)]);
            }
        }
        hits
    }

    pub(super) fn enchant_source_sheet(&self, code: i64, lang: &str) -> Option<ItemSheet> {
        let row = self.enchant_equipment_row(code)?;
        let (en, zh, ko) = self.enchant_names(&row);
        let (description, gts): (String, i64) = self.conn.query_row(
            "SELECT description_ko, description_gts FROM enchant_equipment WHERE result_item=?1", [code],
            |row| Ok((row.get(0)?, row.get(1)?))).ok()?;
        let mut lines = vec![(phrase("code", lang), code.to_string()),
            (wording(lang, "資料來源", "Record", "자료"), wording(lang, "客戶端記錄；道具索引未收錄", "Client record; absent from item index", "클라이언트 기록 · 아이템 목록 미등록")),
            (wording(lang, "原始名稱", "Source name", "원본 이름"), row.source_name)];
        lines.extend(self.enchant_bonus_lines(code, lang));
        Some((lead3(lang, &en, &zh, &ko), trail3(lang, &en, &zh, &ko), format!("#{code}"), lines,
            self.item_note(gts, &description, lang)))
    }

    pub(super) fn enchant_bonus_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        let equipment = self.enchant_equipment_for(code, false);
        let mut lines = Vec::new();
        if equipment.is_empty() && self.conn.query_row("SELECT 1 FROM enchant_recipes WHERE item_code=?1", [code], |_| Ok(())).is_ok() {
            lines.push((wording(lang, "附加能力", "Additional effects", "추가 능력"),
                wording(lang, "客戶端未指定成品關聯", "No result mapping specified by client", "클라이언트 결과 연결 미지정")));
        }
        if let Some(row) = equipment.iter().find(|row| row.result == code) {
            if row.status != "linked" {
                let status = if row.scroll == 0 {
                    wording(lang, "客戶端未指定秘訣", "No scroll specified by client", "클라이언트 비법 미지정")
                } else {
                    format!("{} #{}", wording(lang, "秘訣資料缺失", "Scroll record missing", "비법 자료 없음"), row.scroll)
                };
                lines.push((wording(lang, "鬼道秘訣", "Enchant scroll", "주술 비법"), status));
            }
        }
        let mut profiles: HashMap<&str, Vec<&Equipment>> = HashMap::new();
        for row in &equipment { profiles.entry(&row.effect).or_default().push(row); }
        let mut keys: Vec<_> = profiles.keys().copied().collect();
        keys.sort_unstable();
        let multiple = keys.len() > 1;
        for key in keys {
            let Ok((bonuses, requirements)) = self.conn.query_row("SELECT * FROM enchant_effects WHERE effect_key=?1", [key],
                |row| effect_lines(row, lang)) else { continue; };
            let target = if multiple {
                let names: Vec<_> = profiles[key].iter().map(|row| self.item_name(row.base, lang)).collect();
                format!(" ({})", names.join(", "))
            } else { String::new() };
            if !bonuses.is_empty() {
                lines.push((format!("{}{target}", wording(lang, "鬼道加成", "Enchant bonuses", "주술 추가 능력")), bonuses.join(" · ")));
            }
            if !requirements.is_empty() {
                lines.push((format!("{}{target}", wording(lang, "裝備需求", "Equipment requirements", "장비 요구 조건")), requirements.join(" · ")));
            }
        }
        lines
    }
}

fn effect_lines(row: &rusqlite::Row<'_>, lang: &str) -> rusqlite::Result<(Vec<String>, Vec<String>)> {
    let mut bonuses = Vec::new();
    for (field, zh, en, ko) in [
        ("strength", "力量", "Strength", "힘"), ("dexterity", "敏捷", "Agility", "민첩"),
        ("intelligence", "智力", "Intelligence", "지력"), ("vitality", "生命力", "Vitality", "생명력"),
        ("hp", "體力", "HP", "체력"), ("mp", "法力", "MP", "마법력"),
        ("hit_resist", "打擊抵抗", "Physical resistance", "타격저항"), ("magic_resist", "魔法抵抗", "Magic resistance", "마법저항"),
        ("armor", "防禦力", "Defense", "방어력"), ("damage_min", "最低傷害", "Min damage", "최소 데미지"),
        ("damage_max", "最高傷害", "Max damage", "최대 데미지"), ("attack_speed", "攻擊速度", "Attack speed", "연사속도"),
        ("attack_range", "射程", "Range", "사정거리"), ("magic_damage_min", "最低魔法傷害", "Min magic damage", "마법 최소 데미지"),
        ("magic_damage_max", "最高魔法傷害", "Max magic damage", "마법 최대 데미지"),
        ("mercenary_defense", "傭兵防禦", "Mercenary defense", "용병 방어력"), ("mercenary_attack", "傭兵攻擊", "Mercenary attack", "용병 공격력"),
        ("move_speed", "移動速度", "Movement speed", "이동속도"), ("element", "屬性值", "Element", "속성값"),
        ("attack_damage", "攻擊傷害", "Attack damage", "공격 데미지"), ("damage_reduction", "傷害減免", "Damage reduction", "피해 데미지 감소"),
        ("critical_chance", "暴擊機率", "Critical chance", "치명타 확률"), ("critical_damage", "暴擊傷害", "Critical damage", "치명타 피해"),
    ] {
        let value: i64 = row.get(field)?;
        if value != 0 { bonuses.push(format!("{} {value:+}", wording(lang, zh, en, ko))); }
    }
    for (field, zh, en, ko, suffix) in [
        ("bonus_experience_raw", "額外經驗", "Bonus experience", "보너스 경험치", "%"),
        ("main_enchant_success_raw", "主角鬼道修正（原值）", "Main character enchant modifier (raw)", "주인공 주술 보정 (원본)", ""),
        ("mercenary_enchant_success_raw", "傭兵鬼道修正（原值）", "Mercenary enchant modifier (raw)", "용병 주술 보정 (원본)", ""),
    ] {
        let value: String = row.get(field)?;
        if value.parse::<f64>().is_ok_and(|value| value != 0.0) {
            let sign = if value.starts_with('-') { "" } else { "+" };
            bonuses.push(format!("{} {sign}{value}{suffix}", wording(lang, zh, en, ko)));
        }
    }
    let mut requirements = Vec::new();
    for (field, zh, en, ko) in [
        ("required_strength", "力量", "Strength", "힘"), ("required_dexterity", "敏捷", "Agility", "민첩"),
        ("required_intelligence", "智力", "Intelligence", "지력"), ("required_hp", "體力", "HP", "체력"),
        ("required_level", "等級", "Level", "레벨"), ("required_credit", "信用等級", "Credit", "신용등급"),
    ] {
        let value: i64 = row.get(field)?;
        if value > 0 { requirements.push(format!("{} {}", wording(lang, zh, en, ko), commas(value))); }
    }
    Ok((bonuses, requirements))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enchant_sparse_and_source_only_items_keep_list_detail_names_and_icons_consistent() {
        let catalog = Catalog::open().expect("db");
        let sparse = catalog.index.find_item(1480).expect("sparse indexed result");
        assert!(sparse.en.is_empty() && sparse.zh.is_empty());
        assert!(catalog.index.find_item(1254).is_none());
        for code in [1480, 1254] {
            for lang in ["zh-en", "en-zh", "ko-zh"] {
                let key = format!("item:{code}");
                let rows = catalog.enchant_records("equipment", &code.to_string(), lang);
                let listed = rows.iter().find(|row| row.key == key).expect("listed enchant result");
                let sheet = catalog.item_sheet(code, lang).expect("detail sheet");
                let (detail, _, icon) = catalog.open_key(&key, lang);
                let (en, zh, ko) = catalog.item_names3(code);
                assert_eq!(sheet.0, listed.title, "{key} {lang}");
                assert_eq!(catalog.item_name(code, lang), listed.title, "{key} {lang}");
                assert_eq!(lead3(lang, &en, &zh, &ko), listed.title, "{key} {lang}");
                assert_eq!(sheet.1, catalog.item_alternate_name(&key, lang), "{key} {lang}");
                assert_eq!(detail.lines().next(), Some(listed.title.as_str()), "{key} {lang}");
                assert!(icon.is_some(), "{key} must retain its inherited image");
                assert_eq!(icon, listed.icon, "{key} {lang}");
                assert_eq!(icon, catalog.linked_icon(&key), "{key} {lang}");
            }
        }
        let title = catalog.item_sheet(1480, "zh").expect("translated detail").0;
        assert!(title.contains("武士") && title.contains("大黑天"), "{title}");
    }

    #[test]
    fn enchant_effects_keep_penalties_separate_from_requirements_and_inheritance() {
        let catalog = Catalog::open().expect("db");
        let (bonuses, requirements) = catalog.conn.query_row(
            "SELECT * FROM enchant_effects WHERE effect_key='2Cls_[Cow]'", [], |row| effect_lines(row, "en"))
            .expect("client profile");
        assert!(bonuses.contains(&"Agility -5".to_string()));
        assert!(bonuses.contains(&"Intelligence +10".to_string()));
        assert!(requirements.contains(&"Level 80".to_string()));
        assert!(!bonuses.iter().any(|value| value.contains("Level") || value.ends_with("+0") || value.ends_with("-1")));
    }

    #[test]
    fn enchant_decimal_modifiers_keep_client_values_and_only_explicit_units() {
        let catalog = Catalog::open().expect("db");
        let (bonuses, _) = catalog.conn.query_row(
            "SELECT * FROM enchant_effects WHERE effect_key='BattleParm2_1'", [], |row| effect_lines(row, "en"))
            .expect("client decimal profile");
        // The experience header explicitly says percent. The enchant modifier
        // headers do not, so those raw values must not acquire a percent sign.
        assert!(bonuses.contains(&"Bonus experience +0.05%".to_string()));
        assert!(bonuses.contains(&"Mercenary enchant modifier (raw) +0.5".to_string()));
        assert!(!bonuses.iter().any(|value| value.contains("modifier") && value.contains('%')));
    }
}
