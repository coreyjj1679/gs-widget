use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::{Catalog, Hit};
use super::alias::alias_codes;
use super::names::{haystack, name_match};
use super::quest::quest_source_rank;
use super::rows::{MonsterRow, PlaceRow};
use super::store::online;
use super::text::{caption, chrome_text, commas, commas_text, element_file, element_name, lead, lead3, phrase, pick, pick_place, trail3};

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
            ko,
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

pub struct MonsterBrief {
    pub alternate_name: String,
    pub kind: String,
    pub element: String,
    pub hp: String,
    pub mp: String,
    pub hit: String,
    pub magic: String,
    pub atk: String,
    pub exp: String,
    pub summary: String,
    pub spawn_places: String,
    pub drop_items: Vec<MonsterDropBrief>,
    pub drops: Vec<(PathBuf, i32)>,
    pub drop_total: i32,
    pub battle: Option<(PathBuf, i32)>,
}

pub struct MonsterDropBrief {
    pub key: String,
    pub title: String,
    pub drop_rate: String,
    pub icon: Option<(PathBuf, i32)>,
}

// Older user-supplied catalogs still contain drops, even without rate enrichment.
pub(in crate::db) fn drop_rate_column(conn: &Connection) -> &'static str {
    if conn.prepare("SELECT d.drop_rate_per_100000 FROM monster_drops d LIMIT 0").is_ok() {
        "d.drop_rate_per_100000"
    } else {
        "NULL"
    }
}

pub(in crate::db) fn drop_rate_text(rate: Option<i64>, lang: &str) -> String {
    let Some(rate) = rate.filter(|value| (0..=100_000).contains(value)) else {
        return chrome_text(lang, "未記錄", "Not recorded");
    };
    let whole = rate / 1_000;
    let remainder = rate % 1_000;
    if remainder == 0 {
        format!("{whole}%")
    } else {
        let fraction = format!("{remainder:03}");
        format!("{whole}.{}%", fraction.trim_end_matches('0'))
    }
}

#[derive(Clone, Debug)]
pub struct MonsterQuery {
    pub element: i64,
    pub ela_min: i64,
    pub ela_max: i64,
    pub hit_min: i64,
    pub hit_max: i64,
    pub magic_min: i64,
    pub magic_max: i64,
    pub drop: String,
    pub sort_key: i32,
    pub sort_desc: bool,
}

impl Default for MonsterQuery {
    fn default() -> Self {
        Self {
            element: -1, ela_min: -1, ela_max: -1,
            hit_min: -1, hit_max: -1, magic_min: -1, magic_max: -1,
            drop: String::new(), sort_key: 0, sort_desc: true,
        }
    }
}

impl MonsterQuery {
    pub fn has_sort(&self) -> bool {
        (1..=7).contains(&self.sort_key)
            || (self.sort_key == 8 && drop_item_code(&self.drop).is_some())
    }

    pub fn is_active(&self) -> bool {
        self.element >= 0 || self.ela_min >= 0 || self.ela_max >= 0
            || self.hit_min >= 0 || self.hit_max >= 0 || self.magic_min >= 0 || self.magic_max >= 0
            || !self.drop.trim().is_empty() || self.has_sort()
    }
}

fn drop_item_code(query: &str) -> Option<i64> {
    let query = query.trim();
    let digits = query.strip_prefix("item:").or_else(|| query.strip_prefix('#')).unwrap_or(query).trim();
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok().filter(|code| *code > 0)
}

fn invalid_drop_code(query: &str) -> bool {
    (query.starts_with("item:") || query.starts_with('#')) && drop_item_code(query).is_none()
}

fn within_bounds(value: Option<i64>, min: i64, max: i64) -> bool {
    if min < 0 && max < 0 {
        return true;
    }
    value.is_some_and(|value| (min < 0 || value >= min) && (max < 0 || value <= max))
}

fn compare_optional_stat(a: Option<i64>, b: Option<i64>, descending: bool) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => if descending { b.cmp(&a) } else { a.cmp(&b) },
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn sort_monsters_by_drop_rate(conn: &Connection, hits: &mut [Hit], item: i64, descending: bool) {
    let rate_column = drop_rate_column(conn);
    // Different slots may have separate conditions or rolls. Rank by the highest
    // recorded slot rate; adding them would invent a combined probability.
    let sql = format!(
        "SELECT d.monster_code, MAX(CASE WHEN {rate_column} BETWEEN 0 AND 100000 THEN {rate_column} END)
         FROM monster_drops d WHERE d.item_code = ?1 GROUP BY d.monster_code"
    );
    let mut rates: HashMap<String, Option<i64>> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare(&sql) {
        if let Ok(rows) = stmt.query_map([item], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))) {
            rates.extend(rows.filter_map(Result::ok));
        }
    }
    let rate = |hit: &Hit| hit.key.strip_prefix("monster:").and_then(|code| rates.get(code)).copied().flatten();
    hits.sort_by(|a, b| compare_optional_stat(rate(a), rate(b), descending).then(a.key.cmp(&b.key)));
}

fn monster_brief_names(values: &[String], lang: &str) -> String {
    if values.is_empty() {
        chrome_text(lang, "未記錄", "Not recorded")
    } else {
        values.join(&chrome_text(lang, "、", ", "))
    }
}

fn monster_alternate_name(lang: &str, en: &str, zh: &str, ko: &str) -> String {
    trail3(lang, en, zh, ko)
}

fn monster_brief_summary(drops: &[String], places: &[String], lang: &str) -> String {
    let colon = chrome_text(lang, "：", ": ");
    format!(
        "{}{colon}{}\n{}{colon}{}",
        phrase("drops", lang), monster_brief_names(drops, lang),
        phrase("spawn-place", lang), monster_brief_names(places, lang)
    )
}

#[cfg(test)]
mod brief_tests {
    use super::{drop_item_code, drop_rate_column, drop_rate_text, monster_alternate_name, monster_brief_summary, sort_monsters_by_drop_rate, Catalog, Hit, MonsterQuery};
    use std::collections::HashSet;

    fn drop_sort_hit(code: &str) -> Hit {
        Hit {
            title: code.into(), subtitle: String::new(), key: format!("monster:{code}"),
            icon: None, solo: false, badge: None, aside: String::new(), chip: String::new(),
        }
    }

    #[test]
    fn exact_drop_codes_enable_rate_sort_without_changing_default_sort_precedence() {
        for query in ["11608", "#11608", "item:11608", "  #0011608  "] {
            assert_eq!(drop_item_code(query), Some(11608));
            assert!(MonsterQuery { drop: query.into(), sort_key: 8, ..Default::default() }.has_sort());
        }
        for query in ["", "Holy Star", "item:", "#", "item:-1", "#0", "0", "-1", "#abc", "item:1x", "9223372036854775808"] {
            assert_eq!(drop_item_code(query), None);
            assert!(!MonsterQuery { drop: query.into(), sort_key: 8, ..Default::default() }.has_sort());
        }
        for sort_key in 1..=7 {
            assert!(MonsterQuery { sort_key, ..Default::default() }.has_sort());
        }
        assert!(!MonsterQuery::default().has_sort());
    }

    #[test]
    fn drop_probability_sort_uses_maximum_slot_rate_and_keeps_unknowns_last() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE monster_drops (monster_code TEXT, slot INTEGER, item_code INTEGER, drop_rate_per_100000 INTEGER);
            INSERT INTO monster_drops VALUES
                ('A', 1, 11608, 100), ('A', 9, 11608, 100),
                ('B', 1, 11608, 150), ('C', 1, 11608, 0), ('D', 1, 11608, NULL),
                ('E', 1, 11608, NULL), ('E', 9, 11608, 100), ('E', 2, 999, 100000),
                ('F', 1, 11608, -1), ('G', 1, 11608, 100001);").unwrap();
        for (descending, expected) in [
            (false, ["C", "A", "E", "B", "D", "F", "G"]),
            (true, ["B", "A", "E", "C", "D", "F", "G"]),
        ] {
            let mut hits = ["G", "E", "D", "C", "F", "B", "A"].map(drop_sort_hit);
            sort_monsters_by_drop_rate(&conn, &mut hits, 11608, descending);
            assert_eq!(hits.iter().map(|hit| hit.title.as_str()).collect::<Vec<_>>(), expected);
        }
    }

    #[test]
    fn drop_probability_sort_on_older_catalogs_uses_deterministic_unknown_order() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE monster_drops (monster_code TEXT, slot INTEGER, item_code INTEGER);
            INSERT INTO monster_drops VALUES ('B', 1, 11608), ('A', 9, 11608);").unwrap();
        for descending in [false, true] {
            let mut hits = ["B", "A"].map(drop_sort_hit);
            sort_monsters_by_drop_rate(&conn, &mut hits, 11608, descending);
            assert_eq!(hits.iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(), ["monster:A", "monster:B"]);
        }
    }

    #[test]
    fn exact_drop_search_accepts_all_code_forms_in_both_languages_and_excludes_nondrops() {
        let catalog = Catalog::open().expect("db");
        let item = catalog.index.items.iter().find(|item| item.code_str == "11608").expect("holy star");
        let nondrop: i64 = catalog.conn.query_row(
            "SELECT i.code FROM items i WHERE NOT EXISTS (
                SELECT 1 FROM monster_drops d JOIN monsters m ON m.code = d.monster_code WHERE d.item_code = i.code
             ) ORDER BY i.code LIMIT 1", [], |row| row.get(0),
        ).unwrap();
        for lang in ["zh-en", "en-zh"] {
            let all = catalog.monster_records("", lang, "", "", "");
            let expected = catalog.narrow_monsters(all.clone(), -1, -1, -1, "item:11608");
            for query in ["11608", "#11608", "item:11608"] {
                let choices = catalog.drop_options(query, lang);
                assert_eq!(choices.len(), 1);
                assert_eq!(choices[0].key, "item:11608");
                assert_eq!(choices[0].aside, "#11608");
                assert_eq!(choices[0].subtitle, "13");
                assert_eq!(choices[0].title, super::lead3(lang, &item.en, &item.zh, &item.ko));
                let filtered = catalog.narrow_monsters(all.clone(), -1, -1, -1, query);
                assert_eq!(filtered.iter().map(|hit| &hit.key).collect::<Vec<_>>(), expected.iter().map(|hit| &hit.key).collect::<Vec<_>>());
            }
            for name in [&item.en, &item.zh] {
                assert!(catalog.drop_options(name, lang).iter().any(|hit| hit.key == "item:11608"));
            }
            for query in [nondrop.to_string(), format!("#{nondrop}"), format!("item:{nondrop}")] {
                assert!(catalog.drop_options(&query, lang).is_empty());
            }
            for invalid in ["#abc", "#0", "item:", "item:-1"] {
                assert!(catalog.drop_options(invalid, lang).is_empty());
                assert!(catalog.narrow_monsters(all.clone(), -1, -1, -1, invalid).is_empty());
            }
        }
    }

    #[test]
    fn holy_star_drop_probability_sort_uses_confirmed_rates_and_preserves_unknowns() {
        let catalog = Catalog::open().expect("db");
        let known_desc = ["O12", "O142", "O192", "O13", "O143", "O193", "O14", "O144", "O194"];
        let known_asc = ["O14", "O144", "O194", "O13", "O143", "O193", "O12", "O142", "O192"];
        let unknown = ["O156", "O157", "O38", "O39"];
        for lang in ["zh-en", "en-zh"] {
            for (descending, known) in [(false, known_asc), (true, known_desc)] {
                let mut all = catalog.monster_records("", lang, "", "", "");
                all.reverse();
                let query = MonsterQuery { drop: "#11608".into(), sort_key: 8, sort_desc: descending, ..Default::default() };
                let actual = catalog.refine_monsters(all, &query);
                let expected: Vec<_> = known.iter().chain(&unknown).map(|code| format!("monster:{code}")).collect();
                assert_eq!(actual.iter().map(|hit| &hit.key).collect::<Vec<_>>(), expected.iter().collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn lightning_ranges_sort_real_ela_and_resistance_values_with_deterministic_ties() {
        let catalog = Catalog::open().expect("db");
        let all = catalog.monster_records("", "zh-en", "", "", "");
        let keys = |rows: Vec<super::Hit>| rows.into_iter().map(|hit| hit.key.trim_start_matches("monster:").to_string()).collect::<Vec<_>>();
        let mut ela = MonsterQuery { element: 4, ela_min: 300, ela_max: 320, sort_key: 1, sort_desc: false, ..Default::default() };
        assert_eq!(keys(catalog.refine_monsters(all.clone(), &ela)), ["L253", "O21", "N200", "O151", "P100"]);
        ela.sort_desc = true;
        assert_eq!(keys(catalog.refine_monsters(all.clone(), &ela)), ["N200", "O151", "P100", "O21", "L253"]);
        for sort_key in [2, 3] {
            let mut resistance = MonsterQuery {
                element: 4, hit_min: 300, hit_max: 315, magic_min: 300, magic_max: 315,
                sort_key, sort_desc: false, ..Default::default()
            };
            assert_eq!(keys(catalog.refine_monsters(all.clone(), &resistance)), ["H08", "J133", "H115", "I134", "I135", "K67", "K66"]);
            resistance.sort_desc = true;
            assert_eq!(keys(catalog.refine_monsters(all.clone(), &resistance)), ["K66", "H115", "I134", "I135", "K67", "H08", "J133"]);
        }
        let combined = MonsterQuery {
            element: 4, ela_min: 20, ela_max: 80, hit_min: 300, hit_max: 350, magic_min: 300, magic_max: 350,
            drop: "item:4106".into(), sort_key: 2, ..Default::default()
        };
        assert_eq!(keys(catalog.refine_monsters(all, &combined)), ["I134"]);
    }

    #[test]
    fn unknown_numeric_stats_stay_last_and_are_excluded_by_ranges_without_hiding_zero() {
        let catalog = Catalog::open().expect("db");
        let all = catalog.monster_records("", "zh-en", "", "", "");
        let zero = all.iter().find(|hit| hit.key == "monster:I07").expect("valid zero ELA").clone();
        let high = all.iter().find(|hit| hit.key == "monster:P54").expect("high ELA").clone();
        let mut missing = zero.clone();
        missing.key = "monster:__missing__".into();
        let records = vec![missing.clone(), zero, high];
        for descending in [false, true] {
            let query = MonsterQuery { sort_key: 1, sort_desc: descending, ..Default::default() };
            let sorted = catalog.refine_monsters(records.clone(), &query);
            assert_eq!(sorted.last().unwrap().key, missing.key);
            assert_eq!(sorted.first().unwrap().key, if descending { "monster:P54" } else { "monster:I07" });
        }
        let bounded = catalog.refine_monsters(records, &MonsterQuery { hit_min: 0, sort_key: 1, sort_desc: false, ..Default::default() });
        assert_eq!(bounded.len(), 2);
        assert_eq!(bounded[0].key, "monster:I07");
        assert!(super::within_bounds(Some(0), 0, 0));
        assert!(!super::within_bounds(None, 0, 0));
        assert!(!super::within_bounds(Some(10), 20, 5));
    }

    #[test]
    fn numeric_sort_keys_match_database_values_and_name_sort_uses_the_display_title() {
        let catalog = Catalog::open().expect("db");
        let all = catalog.monster_records("", "en-zh", "", "", "");
        for (key, column) in [(4, "hp"), (5, "mp"), (6, "exp")] {
            for descending in [false, true] {
                let query = MonsterQuery { sort_key: key, sort_desc: descending, ..Default::default() };
                let actual: Vec<String> = catalog.refine_monsters(all.clone(), &query).into_iter().map(|hit| hit.key).collect();
                let direction = if descending { "DESC" } else { "ASC" };
                let sql = format!("SELECT code FROM monsters ORDER BY {column} {direction}, code ASC");
                let mut stmt = catalog.conn.prepare(&sql).expect("sort oracle");
                let expected: Vec<String> = stmt.query_map([], |row| row.get::<_, String>(0)).expect("rows")
                    .map(|row| format!("monster:{}", row.expect("code"))).collect();
                assert_eq!(actual, expected);
            }
        }
        let mut names = all.into_iter().take(4).collect::<Vec<_>>();
        for (hit, title) in names.iter_mut().zip(["Zulu", "alpha", "Alpha", ""]) {
            hit.title = title.into();
        }
        let tied: Vec<_> = names[1..3].iter().map(|hit| hit.key.clone()).collect();
        for descending in [false, true] {
            let sorted = catalog.refine_monsters(names.clone(), &MonsterQuery { sort_key: 7, sort_desc: descending, ..Default::default() });
            assert!(sorted.last().unwrap().title.is_empty());
            let actual_ties: Vec<_> = sorted.iter().filter(|hit| hit.title.eq_ignore_ascii_case("alpha")).map(|hit| hit.key.clone()).collect();
            assert_eq!(actual_ties, tied);
            assert_eq!(sorted[0].title, if descending { "Zulu" } else { "alpha" });
        }
    }

    #[test]
    fn alternate_monster_names_follow_language_and_omit_missing_or_duplicate_translations() {
        assert_eq!(monster_alternate_name("zh-en", "Witch", "魔女", "마녀"), "Witch");
        assert_eq!(monster_alternate_name("en-zh", "Witch", "魔女", "마녀"), "魔女");
        assert_eq!(monster_alternate_name("en-ko", "Witch", "魔女", "마녀"), "마녀");
        for lang in ["en", "zh", "ko"] {
            assert!(monster_alternate_name(lang, "Witch", "魔女", "마녀").is_empty());
        }
        for (en, zh) in [("", "魔女"), ("Witch", ""), ("Witch", "witch"), (" ", " ")] {
            for lang in ["zh-en", "en-zh"] {
                assert!(monster_alternate_name(lang, en, zh, "").is_empty());
            }
        }
        let catalog = Catalog::open().expect("db");
        let code = "E59".to_string();
        let monster = catalog.index.monsters.iter().find(|monster| monster.code == code).expect("bilingual monster");
        assert!(!monster.en.is_empty() && !monster.zh.is_empty());
        assert_eq!(catalog.monster_cards(&[code.clone()], "zh-en")[&code].alternate_name, monster.en);
        assert_eq!(catalog.monster_cards(&[code.clone()], "en-zh")[&code].alternate_name, monster.zh);
        assert_eq!(catalog.monster_cards(&[code.clone()], "en-ko")[&code].alternate_name, monster.ko);
        assert!(catalog.monster_cards(&[code.clone()], "ko")[&code].alternate_name.is_empty());
        let (title, alternate, _, _) = catalog.monster_sheet(&code, "ko").unwrap();
        assert_eq!(title, monster.ko.trim());
        assert!(alternate.is_empty());
    }

    #[test]
    fn drop_options_are_unique_localized_dropped_items_and_support_aliases() {
        let catalog = Catalog::open().expect("db");
        let all = catalog.drop_options("", "zh-en");
        assert!(!all.is_empty() && all.len() <= 40);
        assert_eq!(all.iter().map(|hit| &hit.key).collect::<HashSet<_>>().len(), all.len());
        for hit in &all {
            let code: i64 = hit.key.trim_start_matches("item:").parse().expect("item key");
            let count: i64 = catalog.conn.query_row(
                "SELECT COUNT(DISTINCT d.monster_code) FROM monster_drops d JOIN monsters m ON m.code = d.monster_code WHERE d.item_code = ?1",
                [code], |row| row.get(0),
            ).expect("drop count");
            assert!(count > 0);
            assert_eq!(hit.subtitle, super::commas(count));
        }
        let zh = catalog.drop_options("生命的精髓", "zh-en");
        assert!(zh.iter().any(|hit| hit.key == "item:4106" && hit.title == "生命的精髓"));
        let en = catalog.drop_options("essence", "en-zh");
        assert!(en.iter().any(|hit| hit.key == "item:4106" && hit.title.to_lowercase().contains("essence")));
        catalog.index.aliases.borrow_mut().insert("qa-drop-alias".into(), HashSet::from(["4106".into(), "-99".into()]));
        let aliased = catalog.drop_options("qa-drop-alias", "zh-en");
        assert_eq!(aliased.iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(), vec!["item:4106"]);
        assert!(catalog.drop_options("zzzz-no-such-drop", "zh-en").is_empty());
    }

    #[test]
    fn exact_drop_filter_matches_all_catalog_monsters_without_the_search_limit() {
        let catalog = Catalog::open().expect("db");
        let all = catalog.monster_records("", "zh-en", "", "", "");
        assert!(all.len() > 80);
        let filtered = catalog.filter_monsters(all.clone(), -1, -1, -1, "item:4106");
        let actual: HashSet<String> = filtered.into_iter().map(|hit| hit.key).collect();
        let mut stmt = catalog.conn.prepare("SELECT DISTINCT m.code FROM monsters m JOIN monster_drops d ON d.monster_code = m.code WHERE d.item_code = 4106").expect("expected drops");
        let expected: HashSet<String> = stmt.query_map([], |row| row.get::<_, String>(0)).expect("rows")
            .map(|row| format!("monster:{}", row.expect("code"))).collect();
        assert_eq!(actual, expected);
        let first_page: HashSet<String> = all.into_iter().take(80).map(|hit| hit.key).collect();
        assert!(actual.iter().any(|key| !first_page.contains(key)), "matching monsters after the original 80 must be kept");
        assert!(catalog.monster_records("a", "en-zh", "", "", "").len() > 80);
        for invalid in ["item:", "item:abc", "item:-1"] {
            assert!(catalog.filter_monsters(catalog.monster_records("", "zh-en", "", "", ""), -1, -1, -1, invalid).is_empty());
        }
        let text = catalog.filter_monsters(catalog.monster_records("", "zh-en", "", "", ""), -1, -1, -1, "生命的精髓");
        assert!(text.iter().any(|hit| hit.key == "monster:E59"));
    }

    #[test]
    fn full_monster_records_intersect_the_selected_region_with_the_drop_filter() {
        let catalog = Catalog::open().expect("db");
        let (nation, location): (i64, i64) = catalog.conn.query_row(
            "SELECT m.nation, ml.location_gts FROM monsters m JOIN monster_locations ml ON ml.monster_code = m.code WHERE m.code = 'E59' ORDER BY ml.location_gts LIMIT 1",
            [], |row| Ok((row.get(0)?, row.get(1)?)),
        ).expect("witch location");
        let region = catalog.monster_records("", "zh-en", &location.to_string(), &nation.to_string(), "");
        let region_keys: HashSet<String> = catalog.filter_monsters(region, -1, -1, -1, "item:4106").into_iter().map(|hit| hit.key).collect();
        let mut stmt = catalog.conn.prepare(
            "SELECT DISTINCT m.code FROM monsters m JOIN monster_locations ml ON ml.monster_code = m.code JOIN monster_drops d ON d.monster_code = m.code WHERE ml.location_gts = ?1 AND d.item_code = 4106",
        ).expect("region drop query");
        let expected: HashSet<String> = stmt.query_map([location], |row| row.get::<_, String>(0)).expect("region rows")
            .map(|row| format!("monster:{}", row.expect("code"))).collect();
        assert_eq!(region_keys, expected);
        assert!(region_keys.contains("monster:E59"));
        let global: HashSet<String> = catalog.scope_monsters("all:", "", "zh-en", -1, -1, -1, "item:4106")
            .into_iter().map(|hit| hit.key).collect();
        assert!(region_keys.is_subset(&global));
        assert!(global.len() > region_keys.len());
    }

    #[test]
    fn summaries_keep_names_beyond_the_icon_limit_and_mark_missing_records() {
        let drops: Vec<String> = (1..=12).map(|index| format!("Drop {index}")).collect();
        let en = monster_brief_summary(&drops, &["First floor".into(), "Second floor".into()], "en-zh");
        assert!(en.starts_with("Drops: Drop 1, "));
        assert!(en.contains("Drop 11, Drop 12\nFound in: First floor, Second floor"));
        assert_eq!(monster_brief_summary(&[], &[], "zh-en"), "掉落：未記錄\n出沒地：未記錄");
        assert_eq!(monster_brief_summary(&[], &[], "en-zh"), "Drops: Not recorded\nFound in: Not recorded");
    }

    #[test]
    fn card_summaries_use_localized_catalog_drop_names() {
        let catalog = Catalog::open().expect("db");
        for (lang, place) in [("zh-en", "出沒地：朝鮮地圖"), ("en-zh", "Found in: Korea Field")] {
            let cards = catalog.monster_cards(&["M24".into()], lang);
            let card = cards.get("M24").expect("deer");
            let drops = catalog.monster_drops("M24", lang);
            assert_eq!(card.drop_total as usize, drops.len());
            assert_eq!(card.drop_items.len(), drops.len());
            for (link, drop) in card.drop_items.iter().zip(&drops) {
                assert_eq!(link.key, drop.key);
                assert_eq!(link.title, drop.title);
                assert_eq!(link.drop_rate, drop.aside);
            }
            assert!(drops.iter().all(|drop| card.summary.contains(&drop.title)), "{}", card.summary);
            assert!(card.summary.ends_with(place), "{}", card.summary);
            assert!(place.ends_with(&card.spawn_places));
        }
    }

    #[test]
    fn drop_rates_preserve_precision_and_distinguish_zero_from_unknown() {
        for (raw, expected) in [(0, "0%"), (1, "0.001%"), (10, "0.01%"), (100, "0.1%"),
            (1_000, "1%"), (5_500, "5.5%"), (12_345, "12.345%"), (100_000, "100%")] {
            assert_eq!(drop_rate_text(Some(raw), "zh-en"), expected);
            assert_eq!(drop_rate_text(Some(raw), "en-zh"), expected);
        }
        assert_eq!(drop_rate_text(None, "zh-en"), "未記錄");
        assert_eq!(drop_rate_text(None, "en-zh"), "Not recorded");
        assert_eq!(drop_rate_text(Some(-1), "zh-en"), "未記錄");
        assert_eq!(drop_rate_text(Some(100_001), "en-zh"), "Not recorded");
    }

    #[test]
    fn older_catalogs_keep_drop_queries_without_rate_column() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE monster_drops (monster_code TEXT, slot INTEGER, item_code INTEGER);
            INSERT INTO monster_drops VALUES ('M24', 1, 143);").unwrap();
        let rate: Option<i64> = conn.query_row(
            &format!("SELECT {} FROM monster_drops d", drop_rate_column(&conn)), [], |row| row.get(0)
        ).unwrap();
        assert_eq!(rate, None);
        conn.execute_batch("ALTER TABLE monster_drops ADD COLUMN drop_rate_per_100000 INTEGER;
            UPDATE monster_drops SET drop_rate_per_100000 = 5500;").unwrap();
        let rate: Option<i64> = conn.query_row(
            &format!("SELECT {} FROM monster_drops d", drop_rate_column(&conn)), [], |row| row.get(0)
        ).unwrap();
        assert_eq!(rate, Some(5500));
    }

    #[test]
    fn bundled_deer_drops_show_imported_rate_in_cards_and_details() {
        let catalog = Catalog::open().expect("db");
        for lang in ["zh-en", "en-zh"] {
            let cards = catalog.monster_cards(&["M24".into()], lang);
            let card = cards.get("M24").expect("deer");
            let herb = card.drop_items.iter().find(|drop| drop.key == "item:143").expect("licorice");
            assert_eq!(herb.drop_rate, "5.5%");
            let drops = catalog.monster_drops("M24", lang);
            assert_eq!(drops.iter().find(|drop| drop.key == "item:143").unwrap().aside, "5.5%");
            assert!(card.summary.contains("5.5%"));
        }
    }

    #[test]
    fn drop_links_keep_every_local_slot_even_without_an_icon() {
        let catalog = Catalog::open().expect("db");
        let codes: Vec<String> = catalog.index.monsters.iter().map(|monster| monster.code.clone()).collect();
        let cards = catalog.monster_cards(&codes, "zh-en");
        assert_eq!(cards.len(), codes.len());
        assert!(cards.values().any(|card| !card.drop_items.is_empty()));
        for card in cards.values() {
            assert_eq!(card.drop_items.len(), card.drop_total as usize);
            assert!(card.drop_items.iter().all(|drop| drop.key.starts_with("item:") && !drop.title.is_empty()));
            assert_eq!(card.drop_items.iter().filter(|drop| drop.icon.is_some()).count(), card.drops.len());
        }
    }
}

fn like_contains(text: &str) -> String {
    let mut pattern = String::from("%");
    for ch in text.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

impl Catalog {
    pub fn element_icon_for_value(&self, element: i64) -> Option<(PathBuf, i32)> {
        let path = self.client_root.join(element_file(element)?);
        path.is_file().then_some((path, 0))
    }

    pub fn refine_monsters(&self, hits: Vec<Hit>, query: &MonsterQuery) -> Vec<Hit> {
        let mut hits = self.narrow_monsters(hits, query.element, query.ela_min, query.ela_max, &query.drop);
        let ranges = query.hit_min >= 0 || query.hit_max >= 0 || query.magic_min >= 0 || query.magic_max >= 0;
        let numeric_sort = (1..=6).contains(&query.sort_key);
        if ranges || numeric_sort {
            let codes: Vec<&str> = hits.iter().filter_map(|hit| hit.key.strip_prefix("monster:")).collect();
            let mut stats: HashMap<String, [Option<i64>; 6]> = HashMap::new();
            if !codes.is_empty() {
                let marks = std::iter::repeat("?").take(codes.len()).collect::<Vec<_>>().join(",");
                let sql = format!("SELECT code, element_gts, hit_resist, magic_resist, hp, mp, exp FROM monsters WHERE code IN ({marks})");
                if let Ok(mut stmt) = self.conn.prepare(&sql) {
                    if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(codes.iter()), |row| {
                        Ok((row.get::<_, String>(0)?, [
                            row.get::<_, Option<i64>>(1)?, row.get::<_, Option<i64>>(2)?, row.get::<_, Option<i64>>(3)?,
                            row.get::<_, Option<i64>>(4)?, row.get::<_, Option<i64>>(5)?, row.get::<_, Option<i64>>(6)?,
                        ]))
                    }) {
                        stats.extend(rows.filter_map(Result::ok));
                    }
                }
            }
            let stat = |hit: &Hit, slot: usize| {
                hit.key.strip_prefix("monster:").and_then(|code| stats.get(code)).and_then(|values| values[slot])
            };
            if ranges {
                hits.retain(|hit| within_bounds(stat(hit, 1), query.hit_min, query.hit_max)
                    && within_bounds(stat(hit, 2), query.magic_min, query.magic_max));
            }
            if numeric_sort {
                let slot = (query.sort_key - 1) as usize;
                hits.sort_by(|a, b| compare_optional_stat(stat(a, slot), stat(b, slot), query.sort_desc).then(a.key.cmp(&b.key)));
            }
        }
        if query.sort_key == 7 {
            if query.sort_desc {
                hits.sort_by_cached_key(|hit| (hit.title.trim().is_empty(), std::cmp::Reverse(hit.title.to_lowercase()), hit.key.clone()));
            } else {
                hits.sort_by_cached_key(|hit| (hit.title.trim().is_empty(), hit.title.to_lowercase(), hit.key.clone()));
            }
        }
        if query.sort_key == 8 {
            if let Some(item) = drop_item_code(&query.drop) {
                sort_monsters_by_drop_rate(&self.conn, &mut hits, item, query.sort_desc);
            }
        }
        hits
    }

    pub fn drop_options(&self, query: &str, lang: &str) -> Vec<Hit> {
        let query = query.trim();
        if invalid_drop_code(query) {
            return Vec::new();
        }
        let exact_code = drop_item_code(query);
        let folded = query.to_lowercase();
        let aliases = alias_codes(&self.index.aliases.borrow(), &folded);
        let mut stmt = match self.conn.prepare(
            "SELECT d.item_code, COUNT(DISTINCT d.monster_code)
             FROM monster_drops d JOIN monsters m ON m.code = d.monster_code
             GROUP BY d.item_code",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let counts: HashMap<i64, i64> = match stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?))) {
            Ok(rows) => rows.filter_map(Result::ok).collect(),
            Err(_) => return Vec::new(),
        };
        let mut choices: Vec<(i64, i64, String, &super::rows::ItemRow)> = self.index.items.iter().filter_map(|item| {
            let code = item.code_str.parse::<i64>().ok()?;
            let count = *counts.get(&code)?;
            let matches = match exact_code {
                Some(exact) => code == exact,
                None => name_match(query, &folded, &item.code_str, &item.names) || aliases.contains(&item.code_str),
            };
            if !matches {
                return None;
            }
            let title = lead3(lang, &item.en, &item.zh, &item.ko);
            Some((code, count, if title.is_empty() { format!("#{code}") } else { title }, item))
        }).collect();
        choices.sort_by(|a, b| {
            if query.is_empty() {
                b.1.cmp(&a.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0))
            } else {
                let exact_a = a.2.eq_ignore_ascii_case(query) || a.3.code_str == query;
                let exact_b = b.2.eq_ignore_ascii_case(query) || b.3.code_str == query;
                exact_b.cmp(&exact_a).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0))
            }
        });
        choices.into_iter().take(40).map(|(code, count, title, _)| Hit {
            title,
            subtitle: commas(count),
            key: format!("item:{code}"),
            icon: self.item_icon(code),
            solo: false,
            badge: None,
            aside: format!("#{code}"),
            chip: String::new(),
        }).collect()
    }

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
                    row.get::<_, String>(1)?,
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
        let Ok((id, ko, en, zh, hp, mp, hit, magic, atk_min, atk_max, exp, element, ela, boss, unit)) = row else {
            return None;
        };
        Some((
            lead3(lang, &en, &zh, &ko),
            trail3(lang, &en, &zh, &ko),
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

    pub fn monster_cards(&self, codes: &[String], lang: &str) -> HashMap<String, MonsterBrief> {
        let mut cards: HashMap<String, MonsterBrief> = HashMap::new();
        if codes.is_empty() {
            return cards;
        }
        let marks = std::iter::repeat("?").take(codes.len()).collect::<Vec<_>>().join(",");
        let stats = format!(
            "SELECT code, hp, mp, hit_resist, magic_resist, atk_min, atk_max, exp, element, element_gts, is_boss, unit_type
             FROM monsters WHERE code IN ({marks})"
        );
        if let Ok(mut stmt) = self.conn.prepare(&stats) {
            if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(codes.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                ))
            }) {
                for row in rows.flatten() {
                    let (code, hp, mp, hit, magic, atk_min, atk_max, exp, element, ela, boss, unit) = row;
                    let kind = if boss == 1 { phrase("boss", lang) } else { phrase("normal", lang) };
                    let unit_name = if unit == 1 { phrase("air", lang) } else { phrase("ground", lang) };
                    let mut element_text = element_name(element, lang);
                    if ela > 0 {
                        element_text.push_str(&format!(" · {} {}", phrase("ela", lang), commas(ela)));
                    }
                    cards.insert(
                        code,
                        MonsterBrief {
                            alternate_name: String::new(),
                            kind: format!("{kind} · {unit_name}"),
                            element: element_text,
                            hp: commas(hp),
                            mp: commas(mp),
                            hit: commas(hit),
                            magic: commas(magic),
                            atk: format!("{}-{}", commas(atk_min), commas(atk_max)),
                            exp: commas(exp),
                            summary: String::new(),
                            spawn_places: String::new(),
                            drop_items: Vec::new(),
                            drops: Vec::new(),
                            drop_total: 0,
                            battle: None,
                        },
                    );
                }
            }
        }
        let mut drop_names: HashMap<String, Vec<String>> = HashMap::new();
        let rate_column = drop_rate_column(&self.conn);
        let drops = format!(
            "SELECT d.monster_code, d.item_code, d.slot, IFNULL(i.name_ko, ''),
                    IFNULL(se.text, ''), IFNULL(sz.text, ''), {rate_column}
             FROM monster_drops d
             LEFT JOIN items i ON i.code = d.item_code
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE d.monster_code IN ({marks}) ORDER BY d.monster_code, d.slot"
        );
        if let Ok(mut stmt) = self.conn.prepare(&drops) {
            if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(codes.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                ))
            }) {
                for (code, item, slot, ko, en, zh, rate) in rows.flatten() {
                    let Some(card) = cards.get_mut(&code) else { continue };
                    card.drop_total += 1;
                    let icon = self.item_icon(item);
                    if let Some(icon) = icon.clone() {
                        card.drops.push(icon);
                    }
                    let mut name = lead3(lang, &en, &zh, &ko);
                    if name.is_empty() {
                        name = format!("#{item}");
                    }
                    if slot >= 9 {
                        name.push_str(&format!(" ({})", phrase("solo-drop", lang)));
                    }
                    let drop_rate = drop_rate_text(rate, lang);
                    drop_names.entry(code).or_default().push(format!("{name} ({drop_rate})"));
                    card.drop_items.push(MonsterDropBrief {
                        key: format!("item:{item}"),
                        title: name.clone(),
                        drop_rate,
                        icon,
                    });
                }
            }
        }
        let mut place_names: HashMap<String, Vec<String>> = HashMap::new();
        let places = format!(
            "SELECT ml.monster_code, ml.location_gts, IFNULL(le.text, ''), IFNULL(lz.text, '')
             FROM monster_locations ml
             LEFT JOIN strings le ON le.object = 'OnlineText.dat' AND le.lang = 'en'
                AND le.id = printf('%08d', ml.location_gts)
             LEFT JOIN strings lz ON lz.object = 'OnlineText.dat' AND lz.lang = 'zh'
                AND lz.id = printf('%08d', ml.location_gts)
             WHERE ml.monster_code IN ({marks}) ORDER BY ml.monster_code, ml.location_gts"
        );
        if let Ok(mut stmt) = self.conn.prepare(&places) {
            if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(codes.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            }) {
                for (code, gts, en, zh) in rows.flatten() {
                    let name = lead(lang, &en, &zh);
                    place_names.entry(code).or_default().push(if name.is_empty() { format!("#{gts}") } else { name });
                }
            }
        }
        let sprites = format!("SELECT code, path FROM monster_sprites WHERE code IN ({marks})");
        if let Ok(mut stmt) = self.conn.prepare(&sprites) {
            if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(codes.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }) {
                for (code, relative) in rows.flatten() {
                    let Some(card) = cards.get_mut(&code) else { continue };
                    card.battle = self.character_file(&relative).map(|path| (path, 0));
                }
            }
        }
        for monster in &self.index.monsters {
            if let Some(card) = cards.get_mut(&monster.code) {
                card.alternate_name = monster_alternate_name(lang, &monster.en, &monster.zh, &monster.ko);
            }
        }
        for (code, card) in &mut cards {
            let places = place_names.get(code).map(Vec::as_slice).unwrap_or(&[]);
            card.spawn_places = monster_brief_names(places, lang);
            card.summary = monster_brief_summary(
                drop_names.get(code).map(Vec::as_slice).unwrap_or(&[]),
                places,
                lang,
            );
        }
        cards
    }

    pub fn battle_sprite(&self, code: &str) -> Option<(PathBuf, i32)> {
        let relative: String = self
            .conn
            .query_row("SELECT path FROM monster_sprites WHERE code = ?1", [code], |row| row.get(0))
            .ok()?;
        self.character_file(&relative).map(|path| (path, 0))
    }

    pub(in crate::db) fn character_file(&self, relative: &str) -> Option<PathBuf> {
        let relative = relative.replace('\\', "/");
        let name = Path::new(&relative).file_name()?.to_string_lossy().to_lowercase();
        let mut cache = self.character_files.borrow_mut();
        if cache.is_none() {
            let data = self.conn.path().map(Path::new).and_then(Path::parent).unwrap_or(&self.client_root);
            *cache = Some(super::paths::character_file_index(&self.client_root, data, self.characters.as_deref()));
        }
        let files = cache.as_ref()?;
        files.get(&relative.to_lowercase()).or_else(|| files.get(&name)).cloned()
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
        let rate_column = drop_rate_column(&self.conn);
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT d.slot, d.item_code, i.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''),
                        i.large_image, i.large_frame, {rate_column}
                 FROM monster_drops d
                 JOIN items i ON i.code = d.item_code
                 LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                    AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
                 LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                    AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
                 WHERE d.monster_code = ?1
                 ORDER BY d.slot",
            ))
            .expect("drops");
        let rows: Vec<(i64, i64, String, String, String, String, i64, Option<i64>)> = stmt
            .query_map([code], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            })
            .expect("drop rows")
            .filter_map(Result::ok)
            .collect();
        rows.into_iter()
            .map(|(slot, item, ko, en, zh, image, frame, rate)| {
                let solo = slot >= 9;
                let name = lead3(lang, &en, &zh, &ko);
                Hit {
                    title: if solo {
                        format!("{name}  {}", phrase("solo-drop", lang))
                    } else {
                        name
                    },
                    subtitle: caption(&trail3(lang, &en, &zh, &ko), format!("#{item}")),
                    key: format!("item:{item}"),
                    icon: self.icon_path(&image, frame),
                    solo,
                    aside: drop_rate_text(rate, lang),
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

    pub(in crate::db) fn narrow_monsters(&self, hits: Vec<super::Hit>, element: i64, ela_min: i64, ela_max: i64, drop: &str) -> Vec<super::Hit> {
        let drop = drop.trim();
        if invalid_drop_code(drop) {
            return Vec::new();
        }
        let exact_drop = drop_item_code(drop);
        if element < 0 && ela_min < 0 && ela_max < 0 && drop.is_empty() {
            return hits;
        }
        let codes: Vec<String> = hits
            .iter()
            .filter_map(|hit| hit.key.strip_prefix("monster:").map(str::to_string))
            .collect();
        if codes.is_empty() {
            return hits;
        }
        let marks = std::iter::repeat("?").take(codes.len()).collect::<Vec<_>>().join(",");
        let mut sql = format!("SELECT m.code FROM monsters m WHERE m.code IN ({marks})");
        let mut extra: Vec<String> = Vec::new();
        if element >= 0 {
            sql.push_str(&format!(" AND m.element = ?{}", codes.len() + extra.len() + 1));
            extra.push(element.to_string());
        }
        if ela_min >= 0 {
            sql.push_str(&format!(" AND m.element_gts >= ?{}", codes.len() + extra.len() + 1));
            extra.push(ela_min.to_string());
        }
        if ela_max >= 0 {
            sql.push_str(&format!(" AND m.element_gts <= ?{}", codes.len() + extra.len() + 1));
            extra.push(ela_max.to_string());
        }
        if let Some(item) = exact_drop {
            let param = codes.len() + extra.len() + 1;
            sql.push_str(&format!(" AND EXISTS (SELECT 1 FROM monster_drops d WHERE d.monster_code = m.code AND d.item_code = ?{param})"));
            extra.push(item.to_string());
        } else if !drop.is_empty() {
            let raw = codes.len() + extra.len() + 1;
            let folded = raw + 1;
            sql.push_str(&format!(
                " AND EXISTS (
                    SELECT 1 FROM monster_drops d
                    JOIN items i ON i.code = d.item_code
                    LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                        AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
                    LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                        AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
                    WHERE d.monster_code = m.code
                      AND (i.name_ko LIKE ?{raw} ESCAPE '\\' OR IFNULL(se.text, '') LIKE ?{raw} ESCAPE '\\' OR IFNULL(sz.text, '') LIKE ?{raw} ESCAPE '\\'
                        OR lower(i.name_ko) LIKE ?{folded} ESCAPE '\\' OR lower(IFNULL(se.text, '')) LIKE ?{folded} ESCAPE '\\' OR lower(IFNULL(sz.text, '')) LIKE ?{folded} ESCAPE '\\')
                )"
            ));
            extra.push(like_contains(drop));
            extra.push(like_contains(&drop.to_lowercase()));
        }
        let mut params: Vec<String> = codes;
        params.extend(extra);
        let mut stmt = match self.conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(_) => return hits,
        };
        let rows = match stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| row.get::<_, String>(0)) {
            Ok(rows) => rows,
            Err(_) => return hits,
        };
        let keep: HashSet<String> = rows.flatten().collect();
        hits.into_iter()
            .filter(|hit| hit.key.strip_prefix("monster:").is_some_and(|code| keep.contains(code)))
            .collect()
    }

    pub(in crate::db) fn monster_names3(&self, code: &str) -> Option<(String, String, String)> {
        if let Some(monster) = self.index.monsters.iter().find(|monster| monster.code == code) {
            return Some((monster.en.clone(), monster.zh.clone(), monster.ko.clone()));
        }
        let (en, zh) = self.monster_pair(code)?;
        let ko = self.conn.query_row("SELECT name_ko FROM monsters WHERE code = ?1", [code], |row| row.get(0)).unwrap_or_default();
        Some((en, zh, ko))
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
