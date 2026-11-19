use std::collections::HashSet;

use super::*;

fn item_keys(catalog: &Catalog, query: &str) -> Vec<String> {
    catalog.search("item", query, "zh-en", "", "").into_iter().map(|hit| hit.key).collect()
}

#[test]
fn category_search_lists_every_direct_match_under_its_real_count() {
    let catalog = Catalog::open().expect("db");
    let expected = catalog.index.items.iter().filter(|item| item.names.contains("石")).count();
    assert!(expected > 40, "the fixture must exceed the former 40-result search limit");
    assert_eq!(item_keys(&catalog, "石").len(), expected);
    // Browsing a flat category without a query no longer stops at 80 either.
    assert_eq!(catalog.search_scoped("npc", "", "zh-en", "", "", "", "").len(), catalog.index.npcs.len());
}

#[test]
fn category_search_puts_codes_aliases_and_whole_names_before_partial_matches() {
    let catalog = Catalog::open().expect("db");
    let seals = item_keys(&catalog, "噩夢的印章");
    assert_eq!(seals[0], "item:7270", "the whole name precedes lower-numbered variants");
    assert!(seals.iter().any(|key| key == "item:7258"));
    assert_eq!(item_keys(&catalog, "8890")[0], "item:8890");

    catalog.index.aliases.borrow_mut().insert("ssa".into(), HashSet::from(["6213".into(), "6943".into()]));
    let aliased = item_keys(&catalog, "ssa");
    assert_eq!(aliased[..2], ["item:6213", "item:6943"]);
    assert!(aliased.len() > 2, "ordinary name matches stay listed after the alias targets");
}

#[test]
fn quick_search_resolves_reviewed_nicknames_and_keeps_category_counts() {
    let catalog = Catalog::open().expect("db");
    for (query, key) in [("雀天", "hero:T49"), ("雀天套", "set:66")] {
        let groups = catalog.quick_search(query, "zh-en");
        assert_eq!(groups[0].kind, "nickname", "{query}");
        assert_eq!(groups[0].hits[0].key, key, "{query}");
        assert_eq!(groups[0].hits[0].aside, format!("「{query}」"));
    }
    let stones = catalog.quick_search("石", "zh-en");
    let items = stones.iter().find(|group| group.kind == "item").expect("item group");
    assert_eq!(items.total, item_keys(&catalog, "石").len());
    assert_eq!(items.hits.iter().map(|hit| &hit.key).collect::<Vec<_>>(), item_keys(&catalog, "石").iter().take(QUICK_ROWS).collect::<Vec<_>>());
    let keys: Vec<&String> = stones.iter().flat_map(|group| &group.hits).map(|hit| &hit.key).collect();
    assert_eq!(keys.len(), keys.iter().collect::<HashSet<_>>().len(), "a record is listed once");
    assert!(stones.iter().any(|group| group.kind == "monster"));
    assert!(catalog.quick_search("  ", "zh-en").is_empty());
}

#[test]
fn official_former_names_find_the_current_catalog_records_without_user_aliases() {
    let catalog = Catalog::open().expect("db");
    catalog.index.aliases.borrow_mut().clear();
    for (kind, query, key, current) in [
        ("item", "Sliver Key", "item:4518", "Silver Key"),
        ("item", "Iron–Crutch Li’s Gourd", "item:6340", "Li Tieguai`s Gourd"),
        ("monster", "Evil Taoist_Iron‑Crutch Li", "monster:H00", "Evil Taoist_Li Tieguai"),
        ("hero", "Sorcerer", "hero:U96", "Exorcist"),
        ("npc", "NPC_잠든 영혼(정화)", "npc:1333", "Spring of Purification"),
    ] {
        let rows = catalog.search(kind, query, "en", "", "");
        let hit = rows.iter().find(|hit| hit.key == key).expect(query);
        assert_eq!(hit.title, current, "{query}");
        if kind == "item" || kind == "monster" {
            assert_eq!(rows[0].key, key, "exact historical name is ranked before fuzzy matches: {query}");
        }
        let quick = catalog.quick_search(query, "en");
        assert!(quick.iter().any(|group| group.hits.iter().any(|hit| hit.key == key)), "{query}");
    }
    let chinese = catalog.search("item", "Sliver Key", "zh", "", "");
    assert_eq!((&*chinese[0].key, &*chinese[0].title), ("item:4518", "銀色鑰匙"));
}
