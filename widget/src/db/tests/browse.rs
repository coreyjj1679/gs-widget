use super::*;

#[test]
fn patch_notes_open_the_selected_notice() {
    let catalog = Catalog::open().expect("db");
    let notes = catalog.search("patch", "", "zh-en", "", "");
    assert!(!notes.is_empty());
    assert!(notes.iter().all(|hit| hit.key.starts_with("patch:") && hit.icon.is_none()));
    let (text, links, icon) = catalog.open_key(&notes[0].key, "zh-en");
    assert!(text.contains('V'), "{text}");
    assert!(links.is_empty());
    assert!(icon.is_none());
    assert_eq!(catalog.open_key("patch:V0:nope", "zh-en").0, phrase("missing", "zh-en"));
}

#[test]
fn unknown_page_says_it_is_missing() {
    let catalog = Catalog::open().expect("db");
    let (text, links, icon) = catalog.open_key("nope:1", "zh-en");
    assert_eq!(text, phrase("missing", "zh-en"));
    assert!(links.is_empty());
    assert!(icon.is_none());
    assert!(catalog.search("nope", "", "zh", "", "").is_empty());
    assert!(catalog.recipe_sheet(-1, "zh-en").is_none());
    assert_eq!(catalog.open_key("recipe:0", "zh-en").0, phrase("missing-recipe", "zh-en"));
    assert_eq!(catalog.open_key("set:0", "zh-en").0, phrase("missing-set", "zh-en"));
}

#[test]
fn recipe_page_lists_kind_and_materials() {
    let catalog = Catalog::open().expect("db");
    let (name, _alt, code, lines) = catalog.recipe_sheet(659, "zh-en").expect("recipe 659");
    assert!(!name.is_empty());
    assert!(code.starts_with('#'));
    assert!(lines.iter().any(|(label, _)| label == "代碼"));
    assert!(lines.iter().any(|(label, value)| label == "類型" && !value.is_empty()));
    let (text, links, icon) = catalog.open_key("recipe:659", "zh-en");
    assert!(text.contains("材料"));
    assert!(links.iter().any(|hit| hit.key.starts_with("item:")));
    assert!(icon.is_some());
}

#[test]
fn set_page_lists_the_yaksha_pieces() {
    let catalog = Catalog::open().expect("db");
    let (text, links, icon) = catalog.open_key("set:79", "zh-en");
    assert!(text.contains("夜叉"));
    assert!(text.contains("#79"));
    assert!(links.iter().any(|hit| hit.key.starts_with("item:")));
    assert!(icon.is_some());
    let sheet = catalog.set_sheet(79, "zh-en").expect("yaksha");
    assert!(sheet.3.iter().any(|(label, value, _, _)| label == "件數" && value != "0"));
}

#[test]
fn npc_search_finds_the_sergeant() {
    let catalog = Catalog::open().expect("db");
    let hits = catalog.search("npc", "韓逸", "zh", "", "");
    assert!(
        hits.iter().any(|hit| hit.key == "npc:1193"),
        "{}",
        hits.iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>().join(", ")
    );
    let (text, links, icon) = catalog.open_key("npc:1193", "zh-en");
    assert!(text.contains("韓逸") || links.iter().any(|hit| hit.title.contains("韓逸")));
    assert!(icon.is_some());
}

#[test]
fn labels_follow_the_display_mode() {
    assert_eq!(nation_label("1", "zh-en"), "朝鮮 · Joseon");
    assert_eq!(nation_label("1", "en-zh"), "Joseon · 朝鮮");
    assert_eq!(source_label("alchemy", "zh-en"), "鍊金");
    assert_eq!(source_label("alchemy", "en-zh"), "Alchemy");
    assert_eq!(item_category_label("W", "zh-en"), "武器");
    assert_eq!(item_category_label("W", "en-zh"), "Weapons");
    assert_eq!(weapon_family_label("axe", "zh-en"), "斧");
    assert_eq!(weapon_family_label("axe", "en-zh"), "Axe");
    assert_eq!(hero_rank_label("0", "zh-en"), "傭兵");
    assert_eq!(hero_rank_label("9", "en-zh"), "Four Kings");
    assert_eq!(hero_land_label("1", "zh-en"), "朝鮮");
    assert_eq!(hero_land_label("10", "en-zh"), "Japan");
    assert_eq!(hero_tier_label("1", "zh-en"), "一轉");
    assert_eq!(hero_tier_label("2", "en-zh"), "2nd class");
    assert_eq!(relation_label("dropped-by", "zh-en"), "掉落");
    assert_eq!(relation_label("dropped-by", "en-zh"), "Drops");
    assert!(!client_label().is_empty());
    assert!(client_label().starts_with('V'));
}

#[test]
fn monster_cards_summarize_stats_and_drops() {
    let catalog = Catalog::open().expect("db");
    let cards = catalog.monster_cards(&["K63".into(), "M24".into(), "M96".into()], "zh-en");
    let tiger = cards.get("K63").expect("K63");
    assert!(tiger.element.contains("屬性值"));
    assert!(tiger.element.contains("90"));
    assert!(!tiger.hp.is_empty());
    assert!(!tiger.hit.is_empty());
    let deer = cards.get("M24").expect("M24");
    assert!(deer.drop_total > 0);
    assert!(!deer.drops.is_empty());
    let sorceress = cards.get("M96").expect("M96");
    if let Some((path, frame)) = &sorceress.battle {
        assert!(path.is_file(), "{}", path.display());
        assert_eq!(*frame, 0);
    }
    assert!(catalog.monster_cards(&[], "zh-en").is_empty());
}

#[test]
fn place_names_follow_the_grouped_map() {
    let catalog = Catalog::open().expect("db");
    let area = catalog.area_name("2", "冰城", "zh-en");
    assert!(area.contains("冰城"), "{area}");
    let span = catalog.span_name("2", "冰城", "zh-en");
    assert!(span.contains("所有"), "{span}");
    let region = catalog.region_name("32092", "zh");
    assert!(region.contains("冰城") || !region.is_empty(), "{region}");
}

#[test]
fn monster_directory_groups_places_under_each_nation() {
    let catalog = Catalog::open().expect("db");
    let hits = catalog.directory_hits("zh-en");
    let joseon = hits.iter().position(|hit| hit.chip == "label" && hit.title.starts_with("朝鮮")).expect("joseon");
    let china = hits.iter().position(|hit| hit.chip == "label" && hit.title.starts_with("中國")).expect("china");
    assert!(joseon < china);
    assert!(hits[joseon + 1].key.starts_with("jump:1|"));
    assert!(hits.iter().any(|hit| hit.key.contains("|area:") || hit.key.contains("|region:")));
}

#[test]
fn full_monster_directory_starts_with_all_and_saved_and_keeps_country_groups_contiguous() {
    let catalog = Catalog::open().expect("db");
    let all = catalog.monster_records("", "zh-en", "", "", "");
    for (lang, title) in [("zh-en", "所有怪物"), ("en-zh", "All monsters")] {
        let hits = catalog.directory_hits(lang);
        assert_eq!(hits[0].key, "all:");
        assert_eq!(hits[0].title, title);
        assert_eq!(hits[0].subtitle, all.len().to_string());
        assert_eq!(hits[1].key, "saved:");
        let mut current_nation = "";
        for hit in hits.iter().skip(2) {
            if let Some(nation) = hit.key.strip_prefix("nation:") {
                current_nation = nation;
                assert_eq!(hit.chip, "label");
            } else {
                assert!(!current_nation.is_empty());
                assert!(hit.key.starts_with(&format!("jump:{current_nation}|")), "{} must stay in nation:{current_nation}", hit.key);
                assert_ne!(hit.chip, "label");
            }
        }
    }
    let widget = catalog.search("monster", "", "zh-en", "", "");
    assert!(widget.iter().all(|hit| hit.key.starts_with("nation:")), "widget keeps its original nation directory");
}

#[test]
fn region_navigation_searches_both_languages_without_changing_country_membership() {
    let catalog = Catalog::open().expect("db");
    let keys = |rows: Vec<Hit>| rows.into_iter().map(|hit| hit.key).collect::<Vec<_>>();
    let chinese = keys(catalog.region_rows("冰城", 3, "en-zh"));
    let english = keys(catalog.region_rows("  ICE CASTLE  ", 3, "zh-en"));
    assert_eq!(chinese, english);
    assert_eq!(chinese.first().map(String::as_str), Some("nation:2"));
    assert!(chinese.iter().any(|key| key == "jump:2|area:冰城"));
    assert!(chinese.iter().all(|key| key == "nation:2" || key.starts_with("jump:2|")));
    assert!(catalog.region_rows("冰城", 2, "zh-en").is_empty());
    assert!(catalog.region_rows("zz-no-such-region", 0, "zh-en").is_empty());
    assert!(catalog.region_rows("", 99, "zh-en").is_empty());
    let china = keys(catalog.region_rows("", 2, "zh-en"));
    assert_eq!(china.first().map(String::as_str), Some("nation:3"));
    assert_eq!(keys(catalog.region_rows("China", 0, "zh-en")), china);
    assert!(!china.iter().any(|key| key == "all:" || key == "saved:"));
    let camp = catalog.region_rows("Dark Guild Basecamp", 0, "zh-en");
    assert!(camp.iter().any(|hit| hit.key == "jump:1|area:黑色商團營地"));
    assert!(camp.iter().any(|hit| hit.key == "nation:1"));
}

#[test]
fn region_navigation_does_not_change_the_active_monster_query_or_all_scope() {
    let catalog = Catalog::open().expect("db");
    let query = MonsterQuery {
        element: 4, ela_min: 300, hit_min: 400, magic_min: 400,
        sort_key: 1, sort_desc: true, ..Default::default()
    };
    let before = catalog.refine_monsters(catalog.scope_monsters("all:", "", "zh-en", -1, -1, -1, ""), &query);
    let navigation = catalog.region_rows("Ice Castle", 3, "en-zh");
    assert!(!navigation.is_empty());
    let after = catalog.refine_monsters(catalog.scope_monsters("all:", "", "zh-en", -1, -1, -1, ""), &query);
    let keys = |rows: Vec<Hit>| rows.into_iter().map(|hit| hit.key).collect::<Vec<_>>();
    assert_eq!(keys(before), keys(after));
    let all = catalog.scope_monsters("all:", "", "zh-en", -1, -1, -1, "");
    assert_eq!(all.len(), catalog.monster_records("", "zh-en", "", "", "").len());
    assert!(all.len() > 300, "All must not inherit the saved-list limit");
}

#[test]
fn full_scope_lists_monsters_instead_of_another_directory() {
    let catalog = Catalog::open().expect("db");
    let joseon = catalog.scope_monsters("nation:1", "", "zh-en", -1, -1, -1, "");
    assert!(joseon.len() > 1);
    assert!(joseon.iter().all(|hit| hit.key.starts_with("monster:")));
    let nested = catalog.scope_monsters("jump:2|area:冰城", "", "zh-en", -1, -1, -1, "");
    assert!(!nested.is_empty());
    assert!(nested.iter().all(|hit| hit.key.starts_with("monster:")));
    let high = catalog.scope_monsters("nation:1", "", "zh-en", -1, 9_999_999, -1, "");
    assert!(high.len() < joseon.len());
    let missing = catalog.scope_monsters("nation:1", "", "zh-en", -1, -1, -1, "zzzz-no-such-drop");
    assert!(missing.is_empty());
    let crafts = catalog.search("recipe", "", "zh-en", "", "");
    assert!(!crafts.is_empty());
    let recipes = catalog.section_records(&crafts[0].key, "zh-en");
    assert!(!recipes.is_empty() && recipes.iter().all(|hit| hit.key.starts_with("recipe:")));
    let ranks = catalog.search("hero", "", "zh-en", "", "");
    assert!(!ranks.is_empty());
    let heroes = catalog.section_records(&ranks[0].key, "zh-en");
    assert!(heroes.iter().all(|hit| hit.key.starts_with("hero:")));
}

fn listed(code: &str) -> Hit {
    Hit {
        title: String::new(),
        subtitle: String::new(),
        key: format!("monster:{code}"),
        icon: None,
        solo: false,
        badge: None,
        aside: String::new(),
        chip: String::new(),
    }
}

fn element_id(text: &str) -> i64 {
    if text.contains('火') {
        1
    } else if text.contains('水') {
        2
    } else if text.contains('風') {
        3
    } else if text.contains('雷') {
        4
    } else if text.contains('地') {
        5
    } else {
        0
    }
}

#[test]
fn monster_filters_keep_element_range_and_drop_name() {
    let catalog = Catalog::open().expect("db");
    let cards = catalog.monster_cards(&["K63".into(), "M24".into()], "zh-en");
    let tiger = element_id(&cards.get("K63").expect("K63").element);
    let deer = element_id(&cards.get("M24").expect("M24").element);
    let kept = catalog.filter_monsters(vec![listed("K63"), listed("M24")], tiger, -1, -1, "");
    assert!(kept.iter().any(|hit| hit.key == "monster:K63"));
    if tiger != deer {
        assert!(kept.iter().all(|hit| hit.key != "monster:M24"));
    }
    assert_eq!(catalog.filter_monsters(vec![listed("K63")], -1, 90, 90, "").len(), 1);
    assert!(catalog.filter_monsters(vec![listed("K63")], -1, 91, -1, "").is_empty());
    assert!(catalog.filter_monsters(vec![listed("K63")], -1, -1, 89, "").is_empty());
    let drop = catalog
        .monster_drops("M24", "zh-en")
        .into_iter()
        .find(|hit| !hit.solo && hit.title.chars().count() >= 2)
        .expect("deer drop");
    let named = catalog.filter_monsters(vec![listed("M24"), listed("NOPE")], -1, -1, -1, &drop.title);
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].key, "monster:M24");
    assert!(catalog.filter_monsters(vec![listed("M24"), listed("K63")], -1, -1, -1, "%").is_empty());
    assert!(catalog.filter_monsters(vec![listed("M24")], -1, -1, -1, "_").is_empty());
    assert_eq!(catalog.filter_monsters(vec![listed("NOPE")], -1, -1, -1, "").len(), 1);
}

#[test]
fn full_sections_list_every_record() {
    let catalog = Catalog::open().expect("db");
    let weapons = catalog.section_records("icat:W", "zh-en");
    assert!(weapons.len() > 80, "weapons stopped at {}", weapons.len());
    assert!(weapons.iter().all(|hit| hit.key.starts_with("item:")));
    let sources = catalog.search("quest", "", "zh-en", "", "");
    let source = sources.iter().find(|hit| hit.key.starts_with("qsrc:")).expect("quest source");
    let quests = catalog.section_records(&source.key, "zh-en");
    assert!(!quests.is_empty());
    assert!(quests.iter().all(|hit| hit.key.starts_with("quest:")));
    let goods = catalog.section_records("yard:FAC", "zh-en");
    assert!(!goods.is_empty());
    let shops = catalog.section_records("realm:joseon", "zh-en");
    assert!(!shops.is_empty());
    assert!(catalog.section_records("nope", "zh-en").is_empty());
}
