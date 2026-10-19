use super::*;

#[test]
fn item_bilingual_names_follow_the_selected_language_in_lists_and_details() {
    let catalog = Catalog::open().expect("db");
    let item = catalog.index.find_item(11656).expect("Essence of Stars");
    assert!(!item.en.is_empty() && !item.zh.is_empty() && item.en != item.zh);
    for (lang, primary, alternate) in [("zh-en", &item.zh, &item.en), ("en-zh", &item.en, &item.zh)] {
        let sheet = catalog.item_sheet(item.code, lang).expect("item detail");
        assert_eq!(&sheet.0, primary);
        assert_eq!(&sheet.1, alternate);
        assert_eq!(sheet.2, "#11656");
        assert_eq!(&catalog.item_alternate_name("item:11656", lang), alternate);
        let hits = catalog.search("item", "11656", lang, "", "");
        let hit = hits.iter().find(|hit| hit.key == "item:11656").expect("item search");
        assert_eq!(&hit.title, primary);
    }
    assert_eq!(catalog.item_alternate_name("icat:W", "zh-en"), "Weapons");
    assert_eq!(catalog.item_alternate_name("icat:W", "en-zh"), "武器");
    assert_eq!(catalog.item_alternate_name("isub:axe", "zh-en"), "Axe");
    assert_eq!(catalog.item_alternate_name("isub:axe", "en-zh"), "斧");
    let set = catalog.index.sets.iter().find(|set| !set.en.is_empty() && !set.zh.is_empty() && set.en != set.zh).expect("bilingual set");
    for (lang, alternate) in [("zh-en", &set.en), ("en-zh", &set.zh)] {
        assert_eq!(&catalog.item_alternate_name(&format!("set:{}", set.code), lang), alternate);
        assert_eq!(&catalog.set_sheet(set.code, lang).expect("set detail").1, alternate);
    }
}

#[test]
fn item_bilingual_names_omit_missing_or_duplicate_translations() {
    let mut catalog = Catalog::open().expect("db");
    let item_index = catalog.index.items.iter().position(|item| item.code == 11656).expect("item");
    // Only the in-memory fixture is written; the bundled catalog stays read-only.
    catalog.conn = Connection::open_in_memory().expect("fixture");
    catalog.conn.execute_batch(
        "CREATE TABLE items (code INTEGER, name_ko TEXT, name_gts INTEGER, price INTEGER, weight INTEGER, description TEXT, desc_gts INTEGER);
         CREATE TABLE strings (object TEXT, lang TEXT, id TEXT, text TEXT);
         INSERT INTO items VALUES (11656, '', 1, 0, 0, '', 0);"
    ).expect("fixture schema");
    for (en, zh, expected) in [
        ("Only English", "", ["Only English", "Only English"]),
        ("", "只有中文", ["只有中文", "只有中文"]),
        (" Same ", "same", ["same", "Same"]),
        ("", "", ["", ""]),
    ] {
        catalog.index.items[item_index].ko.clear();
        catalog.index.items[item_index].en = en.into();
        catalog.index.items[item_index].zh = zh.into();
        catalog.conn.execute("DELETE FROM strings", []).expect("reset names");
        for (lang, name) in [("en", en), ("zh", zh)] {
            catalog.conn.execute("INSERT INTO strings VALUES ('OnlineText.dat', ?1, '00000001', ?2)", (lang, name)).expect("name fixture");
        }
        for (lang, primary) in ["zh-en", "en-zh"].into_iter().zip(expected) {
            assert_eq!(catalog.item_alternate_name("item:11656", lang), "");
            let sheet = catalog.item_sheet(11656, lang).expect("item detail");
            assert_eq!(sheet.0, primary);
            assert_eq!(sheet.1, "");
            let hit = catalog.search("item", "11656", lang, "", "").into_iter().find(|hit| hit.key == "item:11656").expect("item search");
            assert_eq!(hit.title, primary);
        }
    }
    for key in ["item:nope", "item:-1", "set:-1", "monster:M24", "unknown"] {
        assert!(catalog.item_alternate_name(key, "zh-en").is_empty(), "{key}");
    }
}
