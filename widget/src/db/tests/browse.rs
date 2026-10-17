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
