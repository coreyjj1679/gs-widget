use super::alias::{alias_rows_from, set_query};
use super::names::{fuzzy_budget, fuzzy_dist, haystack, nicknames};
use super::paths::pictures_ready;
use super::user_data::preferences_dir;
use super::search::{is_floor_suffix, place_split};
use super::*;

mod browse;
mod performance;

#[test]
fn display_modes_keep_chinese_and_english() {
    assert_eq!(lead("zh-en", "Fire", "火"), "火");
    assert_eq!(trail("zh-en", "Fire", "火"), "Fire");
    assert_eq!(show("zh-en", "Fire", "火"), "火 · Fire");
    assert_eq!(lead("en-zh", "Fire", "火"), "Fire");
    assert_eq!(trail("en-zh", "Fire", "火"), "火");
    assert_eq!(show("en-zh", "Fire", "火"), "Fire · 火");
    assert_eq!(show("zh-en", "Same", "Same"), "Same");
    assert_eq!(show("en-zh", "", "火"), "火");
    assert_eq!(stack("en-zh", "Fire", "火"), "Fire\n火");
    assert_eq!(phrase("hp", "zh-en"), "體力");
    assert_eq!(phrase("hp", "en-zh"), "HP");
    assert_eq!(phrase("ela", "zh-en"), "屬性值");
    assert_eq!(phrase("ela", "en-zh"), "ELA");
}

#[test]
fn monster_related_lists_kill_and_gather_quests() {
    let catalog = Catalog::open().expect("db");
    let deer = catalog.monster_quests("M24", "en-zh");
    assert!(
        deer.iter().any(|hit| hit.subtitle.contains("30201")),
        "deer should list the antler quest"
    );
    assert!(deer.iter().all(|hit| hit.key.starts_with("quest:")));
    let wolf = catalog.monster_quests("M25", "en-zh");
    assert!(
        wolf.iter().any(|hit| hit.subtitle.contains("50002") && hit.aside.contains('×')),
        "wolf should list the kill quest"
    );
}

#[test]
fn monster_sheet_shows_attribute_value() {
    let catalog = Catalog::open().expect("db");
    let zh = catalog.monster_sheet("K63", "zh-en").expect("K63");
    assert!(zh.3.iter().any(|(label, value)| label == "屬性值" && value == "90"));
    let en = catalog.monster_sheet("K63", "en-zh").expect("K63");
    assert!(en.3.iter().any(|(label, value)| label == "ELA" && value == "90"));
}

#[test]
fn search_pages_follow_the_index() {
    let catalog = Catalog::open().expect("db");
    let items = catalog.search("item", "", "zh", "", "");
    assert_eq!(items.len(), 27);
    assert_eq!(items[0].key, "icat:set");
    assert_eq!(items[0].title, "套裝");
    assert!(catalog.search("item", "zzzz-not-a-real-item", "zh", "", "").is_empty());
    let stones = catalog.search("item", "SSA", "zh", "", "");
    for code in ["6213", "6214", "6215", "6216", "6943"] {
        assert!(stones.iter().any(|hit| hit.key == format!("item:{code}")), "{code}");
    }
    assert!(catalog.search("item", "古土", "zh", "", "").iter().any(|hit| hit.key == "item:8900"));
    assert!(catalog.search("item", "EOS", "en", "", "").iter().any(|hit| hit.key == "item:11656"));
    assert!(catalog.search("item", "星之精随", "zh", "", "").iter().any(|hit| hit.key == "item:11656"));
    assert!(catalog.search("item", "essense of stars", "en", "", "").iter().any(|hit| hit.key == "item:11656"));

    let nations = catalog.search("monster", "", "zh", "", "");
    assert!(nations.iter().any(|hit| hit.key.starts_with("nation:")));
    let nation = nations[0].key.trim_start_matches("nation:");
    let places = catalog.search("monster", "", "en", "", nation);
    assert!(!places.is_empty());
    assert!(places.iter().all(|hit| {
        hit.key.starts_with("region:") || hit.key.starts_with("area:") || hit.key.starts_with("span:")
    }));

    assert_eq!(
        keys(&catalog.search("item", "龍", "zh", "", "")),
        item_keys(&catalog, "龍")
    );
    assert_eq!(
        keys(&catalog.search("item", "zzzz", "en", "", "")),
        item_keys(&catalog, "zzzz")
    );
    assert_eq!(
        keys(&catalog.search("recipe", "a", "ko", "", "")),
        recipe_keys(&catalog, "a")
    );
    let crafts = catalog.search("recipe", "", "zh", "", "");
    assert!(crafts.iter().all(|hit| hit.key.starts_with("craft:")));
    assert!(crafts.iter().any(|hit| hit.key == "craft:alchemy"));
    let alchemy = catalog.search("recipe", "", "zh", "alchemy", "");
    assert!(!alchemy.is_empty());
    assert!(alchemy.iter().all(|hit| hit.key.starts_with("recipe:")));
    assert_eq!(
        keys(&catalog.search("monster", "a", "en", "", "")),
        monster_keys(&catalog, "a", "")
    );
    let quests = catalog.search("quest", "龍", "zh", "", "");
    let quest_ids = quest_ids(&catalog, "龍");
    assert_eq!(keys(&quests), quest_ids.iter().map(|id| format!("quest:{id}")).collect::<Vec<_>>());
    for (hit, id) in quests.iter().zip(quest_ids) {
        let (title, code): (String, String) = catalog
            .conn
            .query_row("SELECT title, code FROM quests WHERE id = ?1", [id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        let shown = catalog.quest_title(id, &title, &code, "zh");
        assert_eq!(hit.title, shown.lines().next().unwrap_or(""));
    }
}

fn keys(hits: &[Hit]) -> Vec<String> {
    hits.iter().map(|hit| hit.key.clone()).collect()
}

fn item_keys(catalog: &Catalog, query: &str) -> Vec<String> {
    let like = format!("%{query}%");
    let mut stmt = catalog
        .conn
        .prepare(
            "SELECT i.code
             FROM items i
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE ?1 = ''
                OR CAST(i.code AS TEXT) = ?1
                OR i.name_ko LIKE ?2
                OR se.text LIKE ?2
                OR sz.text LIKE ?2
             ORDER BY i.code
             LIMIT 40",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![query, like], |row| row.get::<_, i64>(0))
        .unwrap()
        .filter_map(Result::ok)
        .map(|code| format!("item:{code}"))
        .collect()
    }

fn recipe_keys(catalog: &Catalog, query: &str) -> Vec<String> {
    let like = format!("%{query}%");
    let mut stmt = catalog
        .conn
        .prepare(
            "SELECT r.id
             FROM recipes r
             JOIN items i ON i.code = r.result_item
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE ?1 = ''
                OR CAST(r.result_item AS TEXT) = ?1
                OR i.name_ko LIKE ?2
                OR se.text LIKE ?2
                OR sz.text LIKE ?2
             ORDER BY r.id
             LIMIT 40",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![query, like], |row| row.get::<_, i64>(0))
        .unwrap()
        .filter_map(Result::ok)
        .map(|id| format!("recipe:{id}"))
        .collect()
    }

fn monster_keys(catalog: &Catalog, query: &str, place: &str) -> Vec<String> {
    let like = format!("%{query}%");
    let mut stmt = catalog
        .conn
        .prepare(
            "SELECT code
             FROM v_monster
             WHERE (?3 = '' OR EXISTS (
                    SELECT 1 FROM monster_locations ml
                    WHERE ml.monster_code = v_monster.code AND ml.location_gts = ?3
                ))
               AND (?1 = ''
                OR code = ?1 COLLATE NOCASE
                OR name_ko LIKE ?2
                OR IFNULL(name_en, '') LIKE ?2
                OR IFNULL(name_zh, '') LIKE ?2)
             ORDER BY code
             LIMIT 80",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![query, like, place], |row| row.get::<_, String>(0))
        .unwrap()
        .filter_map(Result::ok)
        .map(|code| format!("monster:{code}"))
        .collect()
    }

fn quest_ids(catalog: &Catalog, query: &str) -> Vec<i64> {
    let like = format!("%{query}%");
    let mut stmt = catalog
        .conn
        .prepare(
            "SELECT id
             FROM quests
             WHERE ?1 = '' OR code = ?1 OR title LIKE ?2 OR detail LIKE ?2
                OR EXISTS (
                    SELECT 1 FROM quest_lines ql
                    JOIN strings s ON s.object = ql.text_object AND s.id = printf('%08d', ql.gts_id)
                    WHERE ql.quest_id = quests.id AND s.text LIKE ?2
                )
             ORDER BY id
             LIMIT 40",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![query, like], |row| row.get(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect()
    }

#[test]
fn king_nicknames_find_the_set_and_the_slot() {
    assert_eq!(set_query("龜天").map(|(_, _, slot)| slot), Some(None));
    assert_eq!(set_query("龜天手").map(|(_, ko, slot)| (ko, slot)), Some(("다문천왕", Some("G"))));
    assert!(set_query("龍").is_none());
    let catalog = Catalog::open().expect("db");
    let turtle = catalog.search("item", "龜天", "zh", "", "");
    for code in ["6097", "6098", "6099", "6100", "6101", "6525"] {
        assert!(turtle.iter().any(|hit| hit.key == format!("item:{code}")), "{code}");
    }
    assert!(turtle.len() > 40, "set nickname should list every matching item, got {}", turtle.len());
    let glove = catalog.search("item", "龜天手", "zh", "", "");
    assert_eq!(glove.first().map(|hit| hit.key.as_str()), Some("item:6099"));
    assert!(glove.iter().all(|hit| hit.key != "item:6097"));
    assert_eq!(catalog.search("item", "雀天手", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6240"));
    assert_eq!(catalog.search("item", "龍天甲", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6109"));
    assert_eq!(catalog.search("item", "虎天頭", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6243"));
    assert_eq!(catalog.search("item", "龜天戒", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6525"));
    assert_eq!(catalog.search("item", "雀天腰", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6241"));
    assert_eq!(catalog.search("item", "虎天鞋", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6247"));
}

#[test]
fn nicknames_shorten_seals_and_english_names() {
    assert!(nicknames("古代地之印章(土)").iter().any(|alias| alias == "古土"));
    assert!(nicknames("Essence of Stars").iter().any(|alias| alias == "eos"));
    assert!(nicknames("Ancient Earth Seal").iter().any(|alias| alias == "aes"));
    let book = parse_alias_book("SSA 6213, 6214 6215 6216 6943\n8900 古土印");
    assert!(book.get("ssa").is_some_and(|codes| codes.contains("6213") && codes.contains("6943")));
    assert!(book.get("古土印").is_some_and(|codes| codes.contains("8900")));
    let rows = alias_rows_from("SSA 6213 6214 6215 6216 6943");
    assert_eq!(rows, vec![("SSA".into(), "6213 6214 6215 6216 6943".into())]);
    let seal = haystack(&["고대 땅의 인장(土)", "Ancient Earth Seal", "古代地之印章(土)"]);
    assert!(seal.split('\n').any(|line| line == "古土"));
    assert!(seal.split('\n').any(|line| line == "aes"));
    let star = haystack(&["별의정수", "Essence of Stars", "星之精髓"]);
    assert!(star.split('\n').any(|line| line == "eos"));
    assert!(star.split('\n').any(|line| line == "星精"));
    assert_eq!(fuzzy_budget("eo"), 0);
    assert!(fuzzy_dist("essense", "essence of stars", 2).is_some());
    assert!(fuzzy_dist("essense", "essence of stars", 1).is_some());
    assert!(fuzzy_dist("星之精随", "星之精髓", 1).is_some());
    assert!(fuzzy_dist("zzzz", "essence of stars", 1).is_none());
}

#[test]
fn place_split_keeps_floor_suffix() {
    assert_eq!(place_split("冰城 - 1層"), ("冰城".into(), "1層".into()));
    assert_eq!(place_split("帝王的陵寢 -  3層"), ("帝王的陵寢".into(), "3層".into()));
    assert_eq!(place_split("激戰香巴拉2樓"), ("激戰香巴拉".into(), "2樓".into()));
    assert_eq!(place_split("鬼曲城，神仙谷"), ("鬼曲城，神仙谷".into(), String::new()));
    assert_eq!(place_split("Ice Castle - 2nd floor"), ("Ice Castle".into(), "2nd floor".into()));
    assert!(!is_floor_suffix("Glorious Tomb"));
    assert!(is_floor_suffix("1層"));
    assert!(is_floor_suffix("2nd floor"));
}

#[test]
fn similar_places_merge_and_sort_floors() {
    let catalog = Catalog::open().expect("db");
    let places = catalog.search("monster", "", "zh", "", "2");
    let ice = places.iter().find(|hit| hit.key == "area:冰城").expect("merged ice castle");
    assert_eq!(ice.title, "冰城");
    assert!(places.iter().all(|hit| !hit.title.starts_with("冰城 -")));
    let floors = catalog.search_scoped("monster", "", "zh", "", "2", "", "冰城");
    assert_eq!(floors[0].title, "冰城 - 所有");
    assert_eq!(floors[0].key, "span:冰城");
    let names: Vec<_> = floors.iter().skip(1).map(|hit| hit.title.as_str()).collect();
    assert_eq!(names, vec!["冰城 - 1層", "冰城 - 2層", "冰城 - 3層"]);
    let mut taj = false;
    for nation in ["1", "2", "3", "4", "5", "6"] {
        let places = catalog.search("monster", "", "en", "", nation);
        if places.iter().any(|hit| hit.title.contains("Taj")) {
            taj = true;
            assert!(places.iter().all(|hit| !hit.title.contains("Glorious")));
            assert!(places.iter().any(|hit| hit.title.contains("Taj") && !hit.title.contains('-')));
        }
    }
    assert!(taj, "taj mahal garden was not grouped");
    let all = catalog.search_scoped("monster", "", "zh", "span:冰城", "2", "", "冰城");
    let one = catalog.search("monster", "", "zh", "32092", "2");
    assert!(!one.is_empty());
    assert!(one.iter().all(|hit| all.iter().any(|row| row.key == hit.key)));
    assert!(all.len() > one.len());
}

#[test]
fn favourites_search_is_not_cut_off_at_eighty() {
    let catalog = Catalog::open().expect("db");
    let all = catalog.search("monster-all", "", "zh", "", "");
    assert!(all.len() > 80, "favourite scan stopped at {}", all.len());
    assert!(all.iter().any(|hit| hit.key == "monster:E190"));
    assert_eq!(catalog.search("monster", "a", "zh", "", "").len(), 80);
}

#[test]
fn saved_monster_shows_on_outer_pages() {
    use std::collections::HashSet;
    let catalog = Catalog::open().expect("db");
    let monster = catalog
        .search("monster", "", "zh", "32092", "2")
        .into_iter()
        .find(|hit| hit.key.starts_with("monster:"))
        .expect("ice castle monster");
    let code = monster.key.trim_start_matches("monster:").to_string();
    let codes = HashSet::from([code]);
    assert!(catalog.scope_saved("region:32092", "2", &codes));
    assert!(catalog.scope_saved("area:冰城", "2", &codes));
    assert!(catalog.scope_saved("span:冰城", "2", &codes));
    assert!(catalog.scope_saved("nation:2", "2", &codes));
    assert!(!catalog.scope_saved("nation:1", "2", &codes));
    assert!(!catalog.scope_saved("region:32092", "2", &HashSet::new()));
}

#[test]
fn quest_sheet_links_the_chain_and_targets() {
    let catalog = Catalog::open().expect("db");
    let id = quest_code_id(&catalog, "main", "50001");
    let prev = quest_code_id(&catalog, "main", "50000");
    let next = quest_code_id(&catalog, "main", "50002");
    let (text, links, icon) = catalog.open_key(&format!("quest:{id}"), "zh");
    assert_eq!(icon, catalog.quest_npc_icon(id));
    assert!(icon.is_some());
    assert!(!text.is_empty());
    assert!(links.iter().all(|hit| !hit.key.starts_with("quest:")));
    let (prev_hit, next_hits) = catalog.quest_chain(id, "zh");
    assert_eq!(prev_hit.expect("prev").key, format!("quest:{prev}"));
    assert_eq!(next_hits.len(), 1);
    assert_eq!(next_hits[0].key, format!("quest:{next}"));
    assert!(links.iter().all(|hit| hit.key != "item:140"));
    assert!(links.iter().any(|hit| hit.key == "item:8762"));
    let (kill_text, kill_links, _) = catalog.open_key(&format!("quest:{next}"), "zh");
    assert!(!kill_text.is_empty());
    assert!(kill_links.iter().all(|hit| hit.key != "monster:M25"));
    let sheet = catalog.quest_sheet(id, "zh").expect("sheet");
    assert!(!sheet.0.is_empty());
    assert!(sheet.2.contains("50001"));
    assert!(sheet.3.iter().any(|(label, value, key)| label == "要物品" && key == "item:140" && value.contains('×')));
    assert!(catalog.linked_icon("item:140").is_some());
    assert!(sheet.3.iter().any(|(label, value, _)| label == "其他" && value == "等級 2"));
    let rewards = catalog.quest_reward_lines(id, "zh");
    assert!(rewards.iter().any(|(label, value)| label == "經驗" && value == "150,000"));
    assert!(rewards.iter().any(|(label, value)| label == "信用" && value == "100"));
    let kill_sheet = catalog.quest_sheet(next, "zh").expect("kill sheet");
    assert!(kill_sheet.3.iter().any(|(label, value, key)| label == "打怪物" && key == "monster:M25" && value.contains('×')));
    assert!(catalog.linked_icon("monster:M25").is_some());
    let sources = catalog.search("quest", "", "zh", "", "");
    assert_eq!(
        sources.iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
        vec!["qsrc:main", "qsrc:letter", "qsrc:intro", "qsrc:mercenary", "qsrc:dispatch"]
    );
    let starters = catalog.search_scoped("quest", "", "zh", "main", "", "", "0");
    assert!(!starters.is_empty());
    assert!(starters.iter().all(|hit| hit.key.starts_with("quest:")));
    assert!(starters.len() < 40);
    let (quest_id, npc_code): (i64, i64) = catalog
        .conn
        .query_row(
            "SELECT q.id, q.npc_code FROM quests q JOIN npcs n ON n.code = q.npc_code WHERE q.source = 'main' AND n.portrait > 0 LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let (_, links, icon) = catalog.open_key(&format!("quest:{quest_id}"), "zh");
    assert!(icon.is_some());
    assert_eq!(catalog.quest_giver(quest_id), format!("npc:{npc_code}"));
    assert!(links.iter().all(|hit| !hit.key.starts_with("npc:")));
    let (_, npc_links, _) = catalog.open_key(&format!("npc:{npc_code}"), "zh");
    assert!(npc_links.iter().any(|hit| hit.key == format!("quest:{quest_id}")));
    let story = quest_code_id(&catalog, "mercenary", "1");
    let note = catalog.quest_sheet(story, "zh").expect("story").4;
    assert!(note.contains('\n'));
    assert!(!note.contains("\\n"));
    let letter = quest_code_id(&catalog, "letter", "10101");
    let follow = quest_code_id(&catalog, "letter", "10102");
    let (_, letter_links, _) = catalog.open_key(&format!("quest:{letter}"), "zh");
    assert!(letter_links.iter().all(|hit| hit.key != format!("quest:{follow}")));
    let (_, letter_next) = catalog.quest_chain(letter, "zh");
    assert!(letter_next.iter().any(|hit| hit.key == format!("quest:{follow}")));
    let token = quest_code_id(&catalog, "main", "53026");
    let before = quest_code_id(&catalog, "main", "53025");
    let after = quest_code_id(&catalog, "main", "53027");
    let token_sheet = catalog.quest_sheet(token, "zh").expect("53026");
    assert!(token_sheet.3.iter().any(|(label, value, key)| label == "要物品" && key == "item:2263" && value.contains('×')));
    assert!(token_sheet.3.iter().any(|(label, value, key)| label == "其他" && value == "等級 120" && key.is_empty()));
    assert!(token_sheet.3.iter().all(|(label, _, _)| label != "經驗" && label != "信用"));
    let token_rewards = catalog.quest_reward_lines(token, "zh");
    assert!(token_rewards.iter().any(|(label, value)| label == "經驗" && value == "5,000,000"));
    assert!(token_rewards.iter().any(|(label, value)| label == "信用" && value == "5,000"));
    let (_, token_links, _) = catalog.open_key(&format!("quest:{token}"), "zh");
    assert!(token_links.iter().any(|hit| hit.key == "item:5713"));
    assert!(token_links.iter().any(|hit| hit.key == "item:11487"));
    assert!(token_links.iter().all(|hit| hit.aside.is_empty()));
    let (token_prev, token_next) = catalog.quest_chain(token, "zh");
    assert_eq!(token_prev.expect("53025").key, format!("quest:{before}"));
    assert_eq!(token_next.len(), 1);
    assert_eq!(token_next[0].key, format!("quest:{after}"));
}

#[test]
fn npc_sheet_shows_name_nation_map_and_place() {
    let catalog = Catalog::open().expect("db");
    let hits = catalog.search("npc", "金春森", "zh-en", "", "");
    let hit = hits.iter().find(|row| row.title == "金春森").expect("kim choonsam");
    assert!(hit.icon.is_some());
    assert_eq!(hit.aside, "全區");
    let code: i64 = hit.key.trim_start_matches("npc:").parse().unwrap();
    let (name, alt, map, facts, note) = catalog.npc_sheet(code, "zh-en").unwrap();
    assert_eq!(name, "金春森");
    assert_eq!(alt, "Kim Choonsam");
    assert!(!map.is_empty());
    assert!(facts.iter().any(|(label, value)| label == "國籍" && value == "全區"));
    assert!(facts.iter().any(|(label, value)| label == "地圖" && value == &map));
    assert!(facts.iter().any(|(label, value)| label == "座標" && value.contains(',')));
    assert!(note.contains("平壤"));
}

#[test]
fn hero_sheet_shows_rank_nation_and_hire() {
    let catalog = Catalog::open().expect("db");
    let ranks = catalog.search("hero", "", "zh-en", "", "");
    assert!(ranks.iter().any(|hit| hit.key == "rank:0" && hit.title == "傭兵"));
    let (name, alt, heading, facts, note) = catalog.hero_sheet("U00", "zh-en").expect("porter");
    assert!(!name.is_empty());
    assert!(!alt.is_empty() || name != "U00");
    assert!(heading.contains("傭兵"));
    assert!(heading.contains("U00"));
    assert!(facts.iter().any(|(label, value)| label == "國籍" && value == "朝鮮"));
    assert!(facts.iter().any(|(label, _)| label == "雇用"));
    assert!(note.contains("行李"));
    let kings = catalog.search_scoped("hero", "", "zh-en", "9", "", "", "");
    assert!(kings.iter().any(|hit| hit.title == "持國天王"));
    let lands = catalog.search_scoped("hero", "", "zh-en", "", "", "land", "");
    assert!(lands.iter().any(|hit| hit.key == "land:1" && hit.title == "朝鮮"));
    let tiers = catalog.search_scoped("hero", "", "zh-en", "1", "", "land", "");
    assert!(tiers.iter().any(|hit| hit.key == "band:0" && hit.title == "傭兵"));
    assert!(tiers.iter().any(|hit| hit.key == "band:1" && hit.title == "一轉"));
    assert!(tiers.iter().any(|hit| hit.key == "band:2" && hit.title == "二轉"));
    let swords = catalog.search_scoped("hero", "", "zh-en", "1", "", "land", "0");
    assert!(swords.iter().any(|hit| hit.key == "hero:U86"));
    assert!(catalog.hero_mark().is_some());
    let (_, chain, _) = catalog.open_key("hero:U86", "zh-en");
    assert_eq!(chain.len(), 4);
    assert_eq!(chain[0].key, "hero:U86");
    assert_eq!(chain[0].aside, "原本");
    assert_eq!(chain[1].key, "hero:U87");
    assert_eq!(chain[1].aside, "一轉");
    assert_eq!(chain[2].aside, "二轉");
    assert_eq!(chain[3].aside, "改造");
    assert!(chain[0].subtitle.contains("轉職"));
    assert!(chain.iter().all(|hit| hit.chip != "label"));
    let (_, kundali, _) = catalog.open_key("hero:T86", "zh");
    let source_at = kundali.iter().position(|hit| hit.key == "hero:T86" && hit.chip != "label").expect("kundali");
    let awa_at = kundali.iter().position(|hit| hit.key == "hero:V29").expect("awa kundali");
    assert!(kundali[source_at..awa_at].iter().all(|hit| !hit.key.starts_with("item:")));
    assert!(kundali.iter().any(|hit| hit.chip == "section" && hit.key == "need"));
    let (_, awa_kundali, _) = catalog.open_key("hero:V29", "zh");
    let heading = awa_kundali.iter().position(|hit| hit.chip == "label" && hit.title.contains("260")).expect("level");
    assert!(!awa_kundali[heading].title.contains(",,") && !awa_kundali[heading].title.contains('['));
    let mats: Vec<_> = awa_kundali[heading + 1..].iter().take_while(|hit| hit.chip != "label").filter(|hit| hit.key.starts_with("item:")).collect();
    assert_eq!(mats.len(), 4);
    let relic = mats.iter().find(|hit| hit.key == "item:12793").expect("relic");
    assert_eq!(relic.aside, "× 1");
    assert!(relic.icon.is_some());
    assert_eq!(mats.iter().find(|hit| hit.key == "item:7063").unwrap().aside, "× 200");
    assert_eq!(mats.iter().find(|hit| hit.key == "item:5860").unwrap().aside, "× 10");
    assert_eq!(mats.iter().find(|hit| hit.key == "item:10880").unwrap().aside, "× 200");
    let (_, kings, _) = catalog.open_key("hero:T49", "zh");
    assert!(kings.iter().any(|hit| hit.key == "item:4066" && hit.aside == "× 1"));
    let (_, qiongqi, _) = catalog.open_key("hero:T65", "zh");
    assert!(qiongqi.iter().any(|hit| hit.key == "item:3669" && hit.aside == "× 75"));
    assert!(qiongqi.iter().any(|hit| hit.key == "item:5798" && hit.aside == "× 2"));
    let (_, promoted, _) = catalog.open_key("hero:D78", "zh");
    let bead = promoted.iter().find(|hit| hit.key == "item:853").expect("seal bead");
    assert_eq!(bead.aside, "× 7");
    assert!(promoted.iter().any(|hit| hit.chip == "label" && hit.title.contains("轉職")));
    let (_, virudhaka, _) = catalog.open_key("hero:T51", "zh");
    let line_end = virudhaka.iter().position(|hit| hit.chip == "section" && hit.key == "need").unwrap_or(virudhaka.len());
    assert!(virudhaka[..line_end].iter().all(|hit| !hit.key.starts_with("item:")));
    assert!(virudhaka.iter().all(|hit| !hit.title.contains(",,") && !hit.title.contains('[')));
    assert!(virudhaka.iter().any(|hit| hit.key == "hero:E85"));
    assert!(virudhaka.iter().all(|hit| hit.key != "item:10745"));
    let (_, awa_viru, _) = catalog.open_key("hero:V09", "zh");
    assert!(awa_viru.iter().any(|hit| hit.chip == "label" && hit.title.contains("260") && !hit.title.contains('[') && !hit.title.contains(",,")));
    assert!(awa_viru.iter().any(|hit| hit.key == "item:10745"));
    let (_, king, _) = catalog.open_key("hero:T49", "zh");
    assert!(king.iter().any(|hit| hit.key == "item:5789"));
    assert!(king.iter().any(|hit| hit.chip == "label" && hit.title.contains("持國天王")));
    let (_, kirin, _) = catalog.open_key("hero:E85", "zh");
    assert!(kirin.iter().all(|hit| hit.key != "item:5789"));
    assert!(kirin.iter().any(|hit| hit.key == "hero:T49" && hit.subtitle == "組合" && hit.chip != "label"));
    let (_, awa, _) = catalog.open_key("hero:T62", "zh");
    let section = awa.iter().position(|hit| hit.chip == "section" && hit.key == "need").expect("requirements");
    assert!(awa[..section].iter().all(|hit| !hit.key.starts_with("item:")));
    let hire = awa.iter().position(|hit| hit.chip == "label" && hit.title.contains("僱")).expect("hire");
    let route = awa.iter().position(|hit| hit.key == "hero:T86" && hit.subtitle == "組合").expect("kundali route");
    assert!(section < hire && hire < route);
    assert!(awa.iter().all(|hit| hit.key != "hero:H47"));
    let (_, airavata, _) = catalog.open_key("hero:T79", "zh");
    let related = airavata.iter().position(|hit| hit.chip == "section" && hit.key == "related").expect("related");
    let route = airavata.iter().position(|hit| hit.key == "hero:T113" && hit.subtitle == "組合").expect("acala");
    assert!(related < route);
    assert!(airavata[..related].iter().all(|hit| hit.key != "hero:T113"));
    let (_, tortoise, _) = catalog.open_key("hero:E77", "zh");
    assert!(tortoise.iter().any(|hit| hit.title.contains("不同幻獸石像") && hit.aside == "× 3"));
    assert!(tortoise.iter().any(|hit| hit.chip == "label" && hit.subtitle.contains("95")));
    for statue in [3817, 3828, 3839, 3850] {
        let row = tortoise.iter().find(|hit| hit.key == format!("item:{statue}")).expect("statue");
        assert!(row.icon.is_some(), "{statue}");
        assert!(row.title.contains("200"), "{statue}");
        assert_eq!(row.aside, "任選");
    }
    assert!(tortoise.iter().any(|hit| hit.key == "item:3897" && hit.aside == "× 10"));
    assert!(tortoise.iter().any(|hit| hit.key == "item:3506" && hit.aside == "× 3"));
    let (_, kirin_make, _) = catalog.open_key("hero:E85", "zh");
    assert!(kirin_make.iter().any(|hit| hit.chip == "label" && hit.subtitle.contains("100")));
    assert!(kirin_make.iter().any(|hit| hit.key == "item:3894" && hit.aside == "× 1" && hit.title.contains("200")));
    assert!(kirin_make.iter().any(|hit| hit.key == "item:3861" && hit.aside == "× 1" && hit.title.contains("200")));
    assert!(kirin_make.iter().any(|hit| hit.key == "item:3872" && hit.aside == "× 1" && hit.title.contains("200")));
    assert!(kirin_make.iter().any(|hit| hit.key == "item:3883" && hit.aside == "× 1" && hit.title.contains("200")));
    assert!(kirin_make.iter().any(|hit| hit.key == "item:3899" && hit.aside == "× 10"));
    assert!(kirin_make.iter().any(|hit| hit.key == "item:3506" && hit.aside == "× 4"));
    let (_, turtle, _) = catalog.open_key("hero:T40", "zh");
    assert!(turtle.iter().any(|hit| hit.chip == "label" && hit.title.contains("改造")));
    let (_, awakened, _) = catalog.open_key("hero:V01", "zh");
    assert!(awakened.iter().any(|hit| hit.chip == "label" && hit.title.contains("覺醒")));
    let (_, jumong, _) = catalog.open_key("hero:T80", "zh");
    assert!(jumong.iter().any(|hit| hit.chip == "label" && hit.title.contains("轉職")));
    let (_, fox, _) = catalog.open_key("hero:M12", "zh");
    assert!(fox.iter().any(|hit| hit.title.contains("訓練所")));
    assert!(fox.iter().all(|hit| !hit.title.contains("封印珠")));
    let (_, bokuden, _) = catalog.open_key("hero:T118", "zh");
    assert!(bokuden.iter().any(|hit| hit.key == "item:6831" && hit.aside == "× 4"));
    assert!(bokuden.iter().any(|hit| hit.key == "hero:T63"));
    assert!(bokuden.iter().any(|hit| hit.key == "hero:T57"));
    assert!(bokuden.iter().any(|hit| hit.key == "item:5747" && hit.aside == "× 150"));
    let (_, relic, _) = catalog.open_key("item:12793", "zh");
    assert!(relic.iter().any(|hit| hit.chip == "section" && hit.title == "角色"));
    assert!(relic.iter().any(|hit| hit.key == "hero:V29" && hit.subtitle.contains("覺醒")));
    let (_, none, _) = catalog.open_key("hero:U00", "zh-en");
    assert!(none.is_empty());
    let japan = catalog.search_scoped("hero", "", "zh", "10", "", "land", "0");
    let ronin = japan.iter().find(|hit| hit.key == "hero:U09").expect("ronin");
    assert_eq!(ronin.title, "劍術浪人");
    assert_eq!(ronin.aside, "日本");
    assert!(ronin.icon.is_some());
    assert!(japan.iter().all(|hit| hit.aside == "日本"));
    let (name, _, _, facts, _) = catalog.hero_sheet("U09", "zh").unwrap();
    assert_eq!(name, "劍術浪人");
    assert!(facts.iter().any(|(label, value)| label == "國籍" && value == "日本"));
    for (land, label) in [("100", "中國"), ("1000", "台灣"), ("10000", "印度")] {
        let hits = catalog.search_scoped("hero", "", "zh", land, "", "land", "0");
        assert!(!hits.is_empty(), "{land}");
        assert!(hits.iter().all(|hit| hit.aside == label), "{land}");
    }
}

#[test]
fn production_lists_buildings_then_goods() {
    let catalog = Catalog::open().expect("db");
    let buildings = catalog.search("works", "", "zh", "", "");
    assert!(buildings.iter().all(|hit| hit.key != "yard:LMB" && hit.key != "yard:MIN"));
    let factory = buildings.iter().find(|hit| hit.key == "yard:FAC").expect("factory");
    assert!(!factory.title.is_empty());
    let pasture = buildings.iter().find(|hit| hit.key == "yard:RAN").expect("pasture");
    assert!(!pasture.title.is_empty());
    let farm = catalog.search_scoped("works", "", "zh", "FAR", "", "", "");
    let orchard = farm.iter().find(|hit| hit.key == "bay:FRU").expect("orchard");
    assert!(!orchard.title.is_empty());
    assert!(orchard.icon.is_some());
    assert!(orchard.aside.is_empty());
    let goods = catalog.search_scoped("works", "", "zh", "FAR", "", "", "FRU");
    let apple = goods.iter().find(|hit| hit.key == "item:12").expect("orchard goods");
    assert!(apple.icon.is_some());
    assert!(apple.aside.contains("蒙古"));
    let everywhere = goods.iter().find(|hit| hit.key == "item:11").expect("all regions");
    assert_eq!(everywhere.aside, "所有地區");
    assert!(goods.iter().all(|hit| hit.key.starts_with("item:") && hit.icon.is_some()));
    let butcher = catalog.search_scoped("works", "", "zh", "RAN", "", "", "");
    assert!(butcher.iter().any(|hit| hit.key == "bay:BUT"));
    let stable = butcher.iter().find(|hit| hit.key == "bay:BAN").expect("stable");
    assert!(stable.icon.is_some());
    let woven = catalog.search_scoped("works", "", "zh", "FAC", "", "", "WEA");
    let silk = woven.iter().find(|hit| hit.key == "item:597").expect("china weave");
    assert_eq!(silk.aside, "中國·台灣");
    assert!(woven.iter().any(|hit| hit.aside == "日本"));
    assert!(woven.iter().any(|hit| hit.aside == "所有地區"));
    assert!(woven.iter().all(|hit| hit.aside != "朝鮮 · 日本 · 中國·台灣"));
    assert!(woven.iter().all(|hit| hit.aside != "Weaving Shop"));
    let (_, links, _) = catalog.open_key("item:597", "zh");
    assert!(links.iter().any(|hit| hit.key == "section:make"));
    assert!(!links.iter().any(|hit| hit.chip == "note"));
    assert!(links.iter().any(|hit| hit.key == "bay:WEA" && hit.aside == "中國·台灣"));
    assert!(links.iter().any(|hit| hit.key == "item:63" && hit.chip == "make" && hit.subtitle.contains("200")));
    let (_, fang, _) = catalog.open_key("item:4550", "zh");
    assert!(fang.iter().any(|hit| hit.key == "bay:SPC" && hit.title.contains("特殊武器")));
    let (_, club, _) = catalog.open_key("item:73", "zh");
    let sword = club.iter().find(|hit| hit.key == "bay:SWD").expect("sword factory");
    assert_eq!(sword.aside, "中國");
    assert!(sword.subtitle.contains("武器"));
    assert_eq!(club.iter().filter(|hit| hit.key.starts_with("bay:")).count(), 1);
    assert!(!catalog.work_parent("SWD").is_empty());
    assert!(club.iter().any(|hit| hit.chip == "make" && hit.key.is_empty() && hit.title.starts_with("勞動")));
    let (_, skin, _) = catalog.open_key("item:6886", "zh");
    let crafts: Vec<_> = skin.iter().filter(|hit| hit.chip == "craft-how" && hit.key.starts_with("recipe:")).collect();
    assert_eq!(crafts.len(), 1);
    assert_eq!(crafts[0].key, "recipe:659");
    let (_, quested, _) = catalog.open_key("item:110", "zh");
    assert!(quested.iter().any(|hit| hit.chip == "used-for" && hit.aside == "任務"));
}

#[test]
fn town_shops_show_on_the_item_and_the_map() {
    let catalog = Catalog::open().expect("db");
    let (_, links, _) = catalog.open_key("item:12", "zh");
    assert!(links.iter().any(|hit| hit.chip == "section" && hit.title == "在賣"));
    assert!(links.iter().any(|hit| hit.key == "town:KY000001" && hit.title == "漢陽" && hit.aside == "特產"));
    let realms = catalog.search("atlas", "", "zh", "", "");
    assert!(realms.iter().any(|hit| hit.key == "realm:joseon" && hit.title == "朝鮮"));
    assert!(realms.iter().all(|hit| hit.key != "realm:field"));
    let towns = catalog.search_scoped("atlas", "", "zh", "joseon", "", "", "");
    assert!(towns.iter().any(|hit| hit.key == "town:KY000001" && hit.title == "漢陽"));
    let goods = catalog.search_scoped("atlas", "", "zh", "joseon", "", "", "KY000001");
    assert!(goods.iter().any(|hit| hit.key == "item:12" && hit.aside == "特產"));
}

#[test]
fn item_sheet_lists_the_enhance_curve() {
    let catalog = Catalog::open().expect("db");
    let (_, _, _, lines, _) = catalog.item_sheet(9077, "zh-en").expect("star weapon");
    let curve: Vec<_> = lines
        .iter()
        .filter(|(_, value)| value.starts_with('+') && value.contains(' '))
        .map(|(_, value)| value.as_str())
        .collect();
    assert!(curve.len() >= 10);
    assert!(curve[0].contains("+1"));
    assert!(curve[0].contains("60"));
    assert!(curve.iter().any(|line| line.contains("+10") && line.contains("600")));
    let (_, _, _, bracelet, _) = catalog.item_sheet(6163, "zh-en").expect("bracelet");
    let fire: Vec<_> = bracelet
        .into_iter()
        .filter(|(_, value)| value.starts_with('+') && value.contains(' '))
        .map(|(_, value)| value)
        .collect();
    assert!(fire.iter().any(|line| line.contains("+5") && line.contains('5')));
    assert!(fire.iter().any(|line| line.contains('火')));
}

#[test]
fn item_page_groups_by_category() {
    let catalog = Catalog::open().expect("db");
    let kinds = catalog.search("item", "", "zh-en", "", "");
    assert!(kinds.iter().any(|hit| hit.key == "icat:W" && hit.title == "武器"));
    assert!(kinds.iter().any(|hit| hit.key == "icat:E" && hit.title == "材料"));
    assert!(kinds.iter().any(|hit| hit.key == "icat:other" && hit.title == "其他"));
    let weapons = catalog.search("item", "", "zh-en", "W", "");
    assert!(weapons.iter().any(|hit| hit.key == "isub:axe" && hit.title == "斧" && hit.icon.is_some()));
    assert!(weapons.iter().any(|hit| hit.key == "isub:bow" && hit.title == "弓"));
    assert!(weapons.iter().any(|hit| hit.key == "isub:sword" && hit.title == "劍"));
    let axes = catalog.search_scoped("item", "斧", "zh-en", "W", "", "", "axe");
    assert!(axes.iter().any(|hit| hit.title.contains('斧')));
    let bows = catalog.search_scoped("item", "斧", "zh-en", "W", "", "", "bow");
    assert!(bows.is_empty());
    let (_, _, beast) = catalog.open_key("item:3895", "zh-en");
    assert!(beast.is_some(), "神獸之根源 should have an inventory image");
    let (_, _, charm) = catalog.open_key("item:1012", "zh-en");
    assert!(charm.is_some(), "護身符 should have an inventory image");
    let (_, relations, _) = catalog.open_key("item:110", "zh-en");
    assert!(relations.iter().any(|hit| hit.chip == "section" && hit.title == "合成"));
    assert!(relations.iter().any(|hit| hit.key.starts_with("recipe:")));
    assert!(relations.iter().any(|hit| hit.chip == "section" && hit.title == "用途"));
    assert!(relations.iter().any(|hit| hit.key.starts_with("quest:")));
    let (_, seal, _) = catalog.open_key("item:3066", "zh-en");
    let ring = seal.iter().find(|hit| hit.key == "item:3588").expect("dui ring");
    assert_eq!(ring.chip, "used-for");
    assert_eq!(ring.aside, "生產");
    assert!(ring.subtitle.contains('2'));
    let (_, made, _) = catalog.open_key("item:3588", "zh-en");
    assert!(made.iter().any(|hit| hit.key == "item:3066" && hit.chip == "make"));
    let sources = catalog.search("recipe", "", "zh-en", "", "");
    assert!(sources.iter().any(|hit| hit.key.starts_with("craft:") && hit.icon.is_some()));
    assert!(kinds.iter().any(|hit| hit.key == "icat:set" && hit.title == "套裝"));
    let sets = catalog.search("item", "夜叉", "zh-en", "set", "");
    let yaksa = sets.iter().find(|hit| hit.key == "set:79").expect("vajrayaksa set");
    assert!(yaksa.title.contains("夜叉"));
    assert_eq!(yaksa.subtitle.contains("6"), true);
    let (_, _, _, sheet, _) = catalog.set_sheet(79, "zh-en").expect("vajrayaksa sheet");
    assert!(sheet.iter().any(|(label, value, _, _)| {
        label == "部位" && value.contains("頭") && value.contains("甲") && value.contains("手") && value.contains("腰") && value.contains("鞋") && value.contains("戒×2")
    }));
    let (_, pieces, icon) = catalog.open_key("set:79", "zh-en");
    assert!(icon.is_some());
    for code in [7126, 7127, 7128, 7129, 7130, 9465] {
        assert!(pieces.iter().any(|hit| hit.key == format!("item:{code}")), "{code}");
    }
    assert!(pieces.iter().any(|hit| hit.key == "section:H" && hit.chip == "section"));
    assert!(pieces.iter().any(|hit| hit.key == "section:R" && hit.title.contains("×2")));
    let belt = pieces.iter().position(|hit| hit.key == "section:B").expect("belt slot");
    let next = pieces.iter().skip(belt + 1).position(|hit| hit.chip == "section").unwrap_or(pieces.len() - belt - 1);
    let belt_items = &pieces[belt + 1..belt + 1 + next];
    assert!(belt_items.iter().any(|hit| hit.key == "item:7129"));
    assert!(belt_items.iter().all(|hit| hit.key != "item:8302"));
    let saved_extra = std::fs::read_to_string(preferences_dir().join("set-extra.txt")).ok();
    save_set_extra(false);
    let (_, amaterasu, _) = catalog.open_key("set:32", "zh-en");
    let shoes = amaterasu.iter().position(|hit| hit.key == "section:S").expect("shoes");
    let shoes_end = amaterasu.iter().skip(shoes + 1).position(|hit| hit.chip == "section").unwrap_or(amaterasu.len() - shoes - 1);
    let shoe_items = &amaterasu[shoes + 1..shoes + 1 + shoes_end];
    assert!(shoe_items.iter().any(|hit| hit.key == "item:5306"));
    assert!(shoe_items.iter().all(|hit| hit.key != "item:8280" && hit.key != "item:8375" && hit.key != "item:7793"));
    let (_, spear, _) = catalog.open_key("set:34", "zh-en");
    assert!(spear.iter().any(|hit| hit.key == "item:5488"), "untradeable set piece stays");
    let recipe_id: i64 = catalog
        .conn
        .query_row(
            "SELECT r.id FROM recipes r
             JOIN recipe_materials m ON m.recipe_id = r.id
             WHERE r.result_item = 11887 AND m.item_code = 6923",
            [],
            |row| row.get(0),
        )
        .expect("choi ring recipe");
    let (_, mats, _) = catalog.open_key(&format!("recipe:{recipe_id}"), "zh-en");
    let ring = mats.iter().find(|hit| hit.title.contains("廣開土")).expect("kwanggaeto ring");
    assert!(ring.title.contains("太皇"));
    assert!(ring.icon.is_some());
    let charm = catalog.search_scoped("item", "", "zh-en", "set", "", "", "1");
    assert!(charm.iter().any(|hit| hit.key == "set:98"));
    assert!(charm.iter().all(|hit| hit.key != "set:79"));
    let normal = catalog.search_scoped("item", "", "zh-en", "set", "", "", "0");
    assert!(normal.iter().any(|hit| hit.key == "set:79"));
    assert!(normal.iter().all(|hit| hit.key != "set:98"));
    let (_, dipper, _) = catalog.open_key("set:98", "zh-en");
    let charm_belt = dipper.iter().find(|hit| hit.key == "item:8421").expect("dipper belt");
    assert!(charm_belt.title.contains("北斗七星"));
    assert!(charm_belt.title.contains("腰帶"));
    let (_, _, _, dipper_sheet, _) = catalog.set_sheet(98, "zh-en").expect("dipper sheet");
    assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "力量" && value == "+25" && section == "item"));
    assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "體質" && value == "+480" && section == "item"));
    assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "智力" && value == "+100" && section == "item"));
    assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "防禦" && value == "+150" && section == "item"));
    assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "打擊抗性" && value == "+30" && section == "item"));
    assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "魔法抗性" && value == "+25" && section == "item"));
    let (_, arhat, arhat_icon) = catalog.open_key("set:140", "zh-en");
    assert!(arhat_icon.is_some(), "arhat set borrows the base icon");
    let necklace = arhat.iter().find(|hit| hit.key == "item:12036").expect("necklace");
    assert!(necklace.icon.is_some());
    assert!(!necklace.title.is_empty());
    let (_, _, _, arhat_sheet, _) = catalog.set_sheet(140, "zh-en").expect("arhat sheet");
    assert!(arhat_sheet.iter().any(|(label, _, key, _)| label == "道具" && key == "fold:item"));
    assert!(arhat_sheet.iter().any(|(_, value, key, section)| key.starts_with("item:") && value.contains("項鍊") && section == "item"));
    assert!(arhat_sheet.iter().any(|(label, value, _, section)| label == "暴擊傷害" && value == "+20" && section == "item"));
    assert!(arhat_sheet.iter().any(|(label, _, key, _)| label == "合計" && key == "fold:total"));
    assert!(arhat_sheet.iter().any(|(label, value, _, section)| label == "暴擊傷害" && value == "+40" && section == "total"));
    assert!(arhat_sheet.iter().any(|(label, value, _, section)| label == "體質" && value == "+200" && section == "set"));
    let (_, deity_pieces, deity_icon) = catalog.open_key("set:137", "zh-en");
    assert!(deity_icon.is_some(), "deity set borrows the base icon");
    let deity_necklace = deity_pieces.iter().find(|hit| hit.key == "item:11943").expect("deity necklace");
    assert!(deity_necklace.icon.is_some(), "deity necklace borrows the base icon");
    let (_, _, _, deity, _) = catalog.set_sheet(137, "zh-en").expect("deity sheet");
    assert!(deity.iter().any(|(_, _, key, _)| key == "item:11943"));
    assert!(deity.iter().any(|(label, value, _, section)| label == "屬性值" && value == "+5" && section == "item"));
    assert!(deity.iter().any(|(label, value, _, section)| label == "土" && value == "+3" && section == "item"));
    let (_, _, _, sage, _) = catalog.set_sheet(138, "zh-en").expect("sage set");
    assert!(sage.iter().any(|(label, value, _, section)| label == "傷害" && value == "+5%" && section == "item"));
    assert!(sage.iter().any(|(label, _, key, _)| label == "合計" && key == "fold:total"));
    assert!(sage.iter().any(|(label, value, _, section)| label == "傷害" && value == "+15%" && section == "total"));
    match saved_extra {
        Some(text) => {
            let _ = std::fs::write(preferences_dir().join("set-extra.txt"), text);
        }
        None => {
            let _ = std::fs::remove_file(preferences_dir().join("set-extra.txt"));
        }
    }
    let (_, _, _, shoe_lines, shoes) = catalog.item_sheet(6257, "zh-en").expect("zhuge shoes");
    assert!(shoes.contains("諸葛"));
    assert!(!shoes.contains("Shoes"));
    assert!(shoe_lines.iter().any(|(label, value)| label == "體質" && value == "+50"));
    assert!(shoe_lines.iter().any(|(label, value)| label == "力量" && value == "+50"));
    assert!(shoe_lines.iter().any(|(label, value)| label == "敏捷" && value == "+50"));
    assert!(shoe_lines.iter().any(|(label, value)| label == "智力" && value == "+50"));
    assert!(shoe_lines.iter().any(|(label, value)| label == "打擊抗性" && value == "+20"));
    assert!(shoe_lines.iter().any(|(label, value)| label == "魔法抗性" && value == "+20"));
    let (_, _, _, shoe_en, shoes_en) = catalog.item_sheet(6257, "en-zh").expect("zhuge shoes en");
    assert!(shoes_en.contains("Shoes"));
    assert!(!shoes_en.contains("諸葛"));
    assert!(shoe_en.iter().any(|(label, value)| label == "VIT" && value == "+50"));
    let (_, _, _, lines, _) = catalog.item_sheet(7126, "zh-en").expect("headband");
    assert!(lines.iter().any(|(label, value)| label == "套裝" && value.contains("夜叉")));
    assert!(lines.iter().any(|(label, value)| label == "2件" && value.contains("智力") && value.contains("75")));
    assert!(lines.iter().any(|(label, value)| label == "6件" && value.contains("425") && value.contains("屬性")));
    let (_, relations, _) = catalog.open_key("item:7126", "zh-en");
    assert!(relations.iter().any(|hit| hit.key == "set:79" && hit.chip == "set"));
    assert!(relations.iter().any(|hit| hit.key == "section:set" && hit.title == "套裝"));
    assert_eq!(relation_label("set", "zh-en"), "套裝");
    assert_eq!(relation_label("set", "en-zh"), "Set");
    let kinds_icon = kinds.iter().find(|hit| hit.key == "icat:W").unwrap();
    assert!(kinds_icon.icon.is_some());
    let (_, _, _, lines, _) = catalog.item_sheet(58, "zh-en").expect("belt");
    assert!(lines.iter().any(|(label, value)| label == "分類" && value == "腰帶"));
}

#[test]
fn shipped_pictures_open_without_the_game_client() {
    let db = find_db().expect("db");
    let client = locate_client(PathBuf::from(r"Z:\no-such-client"), db.parent().unwrap());
    assert!(pictures_ready(&client), "{}", client.display());
    assert!(!client.join("gersang.gcs").is_file());
    assert!(client.join("Assets").join("Textures").join("Items").is_dir());
}

fn quest_code_id(catalog: &Catalog, source: &str, code: &str) -> i64 {
    catalog
        .conn
        .query_row(
            "SELECT id FROM quests WHERE source = ?1 AND code = ?2",
            (source, code),
            |row| row.get(0),
        )
        .unwrap()
}
