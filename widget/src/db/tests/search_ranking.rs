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
