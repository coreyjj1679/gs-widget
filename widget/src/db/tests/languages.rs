use super::*;

#[test]
fn korean_search_details_and_clipboard_agree_in_every_selection() {
    let catalog = Catalog::open().expect("db");
    let item = catalog.index.find_item(140).expect("chicken");
    assert_eq!(item.ko, "닭고기");
    for primary in ["zh", "en", "ko"] {
        for secondary in ["", "zh", "en", "ko"] {
            if primary == secondary { continue; }
            let lang = with_secondary_lang(primary, secondary);
            let selected = |language| match language {
                "zh" => item.zh.as_str(), "en" => item.en.as_str(), _ => item.ko.as_str(),
            };
            let rows = catalog.search("item", "닭고기", &lang, "", "");
            let row = rows.iter().find(|row| row.key == "item:140").expect("Korean query in every display language");
            assert_eq!(row.title, selected(primary));
            let sheet = catalog.item_sheet(140, &lang).expect("sheet");
            assert_eq!(sheet.0, row.title);
            assert_eq!(sheet.1, if secondary.is_empty() { "" } else { selected(secondary) });
            assert_eq!(catalog.copyable_name("item:140", &lang, false).as_deref(), Some(sheet.0.as_str()));
            assert_eq!(catalog.copyable_name("item:140", &lang, true), (!sheet.1.is_empty()).then_some(sheet.1));
        }
    }
}

#[test]
fn single_language_removes_alternates_in_related_records_and_quest_previews() {
    let catalog = Catalog::open().expect("db");
    let ids: Vec<_> = catalog.index.quests.iter().map(|quest| quest.id).collect();
    for lang in ["zh", "en", "ko"] {
        let cards = catalog.monster_cards(&["M24".into()], lang);
        assert!(cards["M24"].alternate_name.is_empty());
        for references in catalog.quest_item_previews(&ids, lang).values() {
            for item in references.required.iter().chain(&references.rewards) {
                assert!(item.alternate.is_empty(), "{} {lang}", item.key);
                assert!(!item.primary.is_empty());
            }
        }
        for quest in &catalog.index.quests {
            assert!(catalog.quest_alternate_title(quest.id, lang).is_empty());
        }
        let dispatch = catalog.quest_sheet(1, lang).expect("dispatch with missing Korean title");
        assert!(!dispatch.0.is_empty());
        assert!(dispatch.1.is_empty());
    }
    assert_eq!(catalog.search("monster", "M24", "ko", "", "").iter().find(|row| row.key == "monster:M24").unwrap().title, "사슴");
}

#[test]
fn calculator_language_changes_preserve_the_saved_snapshot_and_costs() {
    let catalog = Catalog::open().expect("db");
    let book = catalog.calculator_catalog().expect("calculator");
    let original = catalog.calculator_document(&book, 1020, "zh-en").expect("recipe");
    let saved = original.clone();
    let translated = catalog.localized_calculator_document(&original, "ko");
    assert_eq!(original, saved);
    assert_eq!(translated.language, "ko");
    assert_eq!(translated.calculate().unwrap().known_total, original.calculate().unwrap().known_total);
    assert_eq!(translated.title, catalog.item_name(original.item_code, "ko"));
    for material in translated.materials {
        assert!(!material.title.contains(" · "));
    }
}
