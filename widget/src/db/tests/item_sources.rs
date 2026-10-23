use std::collections::HashMap;

use super::super::monster::drop_rate_text;
use super::*;

fn section_rows<'a>(links: &'a [Hit], section: &str) -> Vec<&'a Hit> {
    let header = format!("section:{section}");
    links.iter()
        .skip_while(|hit| hit.key != header)
        .skip(1)
        .take_while(|hit| hit.chip != "section")
        .collect()
}

fn highest_rates(catalog: &Catalog, item: i64) -> HashMap<String, Option<i64>> {
    let mut stmt = catalog.conn.prepare(
        "SELECT monster_code, MAX(CASE WHEN drop_rate_per_100000 BETWEEN 0 AND 100000 THEN drop_rate_per_100000 END)
         FROM monster_drops WHERE item_code = ?1 GROUP BY monster_code",
    ).unwrap();
    stmt.query_map([item], |row| Ok((format!("monster:{}", row.get::<_, String>(0)?), row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn item_sources_list_every_record_under_its_true_count() {
    let catalog = Catalog::open().expect("db");
    let monsters: usize = catalog.conn.query_row(
        "SELECT COUNT(DISTINCT monster_code) FROM monster_drops WHERE item_code = 8890", [], |row| row.get(0),
    ).unwrap();
    assert!(monsters > 24, "the fixture must exceed the former 24-row section limit");
    let links = catalog.item_relations(8890, "zh-en");
    let header = links.iter().find(|hit| hit.key == "section:dropped-by").expect("drop section");
    assert_eq!(header.subtitle, monsters.to_string());
    assert_eq!(section_rows(&links, "dropped-by").len(), monsters);

    let recipes: usize = catalog.conn.query_row(
        "SELECT COUNT(*) FROM recipe_materials WHERE item_code = 5507", [], |row| row.get(0),
    ).unwrap();
    assert!(recipes > 24);
    assert_eq!(catalog.recipes_using(5507, "zh-en").len(), recipes);
    let uses = section_rows(&catalog.item_relations(5507, "zh-en"), "used-for").len();
    assert!(uses >= recipes, "{uses} listed uses for {recipes} recipes");
}

#[test]
fn item_droppers_show_the_highest_slot_rate_first_and_unknown_rates_last() {
    let catalog = Catalog::open().expect("db");
    for item in [8890, 10880] {
        let rates = highest_rates(&catalog, item);
        let rows = catalog.item_droppers(item, "zh-en");
        assert_eq!(rows.len(), rates.len());
        let listed: Vec<Option<i64>> = rows.iter().map(|hit| rates[&hit.key]).collect();
        for (hit, rate) in rows.iter().zip(&listed) {
            assert_eq!(hit.aside, drop_rate_text(*rate, "zh-en"), "{}", hit.key);
        }
        let known = listed.iter().take_while(|rate| rate.is_some()).count();
        assert!(listed[known..].iter().all(Option::is_none), "unknown rates must stay last for item {item}");
        assert!(listed[..known].windows(2).all(|pair| pair[0] >= pair[1]), "item {item} is not ordered by rate");
    }
    let best = catalog.item_droppers(8890, "zh-en");
    assert_eq!(best[0].aside, "2.5%");
    assert!(catalog.item_droppers(10880, "en-zh").iter().any(|hit| hit.aside == "Not recorded"));
}
