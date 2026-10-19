use super::*;

fn edit_distance(query: &[char], text: &[char]) -> usize {
    let mut previous: Vec<usize> = (0..=query.len()).collect();
    for (row, ch) in text.iter().enumerate() {
        let mut current = vec![row + 1; query.len() + 1];
        for column in 1..=query.len() {
            current[column] = (previous[column] + 1).min(current[column - 1] + 1)
                .min(previous[column - 1] + usize::from(query[column - 1] != *ch));
        }
        previous = current;
    }
    previous[query.len()]
}

#[test]
fn bounded_fuzzy_matching_preserves_unicode_edit_distance() {
    let mut words = vec![String::new()];
    for length in 1..=5 {
        for mask in 0..1 << length {
            words.push((0..length).map(|bit| if mask & (1 << bit) == 0 { 'a' } else { '虎' }).collect());
        }
    }
    for query in &words {
        let chars: Vec<char> = query.chars().collect();
        for text in &words {
            let distance = edit_distance(&chars, &text.chars().collect::<Vec<_>>());
            for budget in 0..=3 {
                let expected = (distance > 0 && distance <= budget as usize).then_some(distance as u8);
                assert_eq!(super::super::names::bounded_edit(&chars, text, budget), expected,
                    "query={query:?} text={text:?} budget={budget}");
            }
        }
    }
    let long: Vec<char> = "a".repeat(48).chars().collect();
    assert_eq!(super::super::names::bounded_edit(&long, &format!("{}虎", "a".repeat(47)), 1), Some(1));
    assert_eq!(super::super::names::bounded_edit(&long, &"a".repeat(46), 2), Some(2));
    assert_eq!(super::super::names::bounded_edit(&long, &"a".repeat(45), 2), None);
}

#[test]
fn numeric_item_lookup_preserves_all_catalog_references() {
    let catalog = Catalog::open().expect("db");
    assert!(catalog.index.items.windows(2).all(|pair| pair[0].code < pair[1].code));
    for item in &catalog.index.items {
        let found = catalog.index.find_item(item.code).expect("indexed item");
        assert_eq!(found.code_str, item.code_str);
        assert_eq!(found.en, item.en);
        assert_eq!(found.zh, item.zh);
    }
    assert!(catalog.index.find_item(i64::MIN).is_none());
    assert!(catalog.index.find_item(i64::MAX).is_none());
}
