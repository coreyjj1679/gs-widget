use super::*;

fn server() -> GersangMcp {
    GersangMcp::new(db::Catalog::open_for_mcp().unwrap()).unwrap()
}

#[test]
fn tools_are_read_only_and_reject_unknown_arguments() {
    assert_eq!(tools().len(), 7);
    for tool in tools() {
        let annotations = tool.annotations.unwrap();
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
        assert_eq!(tool.input_schema["additionalProperties"], false);
    }
    let server = server();
    for (tool, arguments) in [
        ("catalog_info", json!({"sql": "DELETE FROM items"})),
        ("search_catalog", json!({"category": "item", "limit": 0})),
        ("search_catalog", json!({"category": "item", "limit": 51})),
        ("search_catalog", json!({"category": "item", "offset": -1})),
        (
            "search_catalog",
            json!({"category": "item", "query": "x".repeat(201)}),
        ),
        ("search_catalog", json!({"category": "not-a-category"})),
        (
            "search_catalog",
            json!({"category": "item", "language": "unsupported"}),
        ),
        ("get_entity", json!({"key": "item:999999999999"})),
        ("get_entity", json!({"key": "file:/etc/passwd"})),
        ("find_item_drops", json!({"item_code": 0})),
        (
            "get_quest_dependencies",
            json!({"quest_id": 1, "direction": "arbitrary"}),
        ),
        ("get_recipes", json!({"item_code": 6213, "sql": "SELECT 1"})),
    ] {
        let result = server.result(tool, arguments);
        assert_eq!(result.is_error, Some(true), "{tool}: {result:?}");
        assert!(result.structured_content.unwrap()["error"].is_string());
    }
}

#[test]
fn search_browses_records_and_pages_beyond_gui_limits() {
    let server = server();
    for category in ["item", "monster", "recipe", "quest", "hero", "npc", "set"] {
        let first = server
            .invoke("search_catalog", json!({"category": category, "limit": 2}))
            .unwrap();
        assert!(first["total"].as_u64().unwrap() > 2, "{category}");
        assert_eq!(first["next_offset"], 2);
        let second = server
            .invoke(
                "search_catalog",
                json!({"category": category, "limit": 2, "offset": 2}),
            )
            .unwrap();
        assert_ne!(first["results"][0]["key"], second["results"][0]["key"]);
        for row in first["results"].as_array().unwrap() {
            assert!(row["key"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{category}:")));
            server
                .invoke("get_entity", json!({"key": row["key"]}))
                .unwrap();
        }
    }
    let rows = server
        .invoke(
            "search_catalog",
            json!({"category": "item", "offset": 80, "limit": 50}),
        )
        .unwrap();
    assert_eq!(rows["results"].as_array().unwrap().len(), 50);
    let empty = server
        .invoke(
            "search_catalog",
            json!({"category": "item", "query": "zzzz-not-an-item"}),
        )
        .unwrap();
    assert_eq!(empty["total"], 0);
    assert!(empty["next_offset"].is_null());
}

#[test]
fn monster_drops_match_reverse_lookup_and_combat_stats() {
    let server = server();
    let monster = server
        .invoke(
            "get_entity",
            json!({"key": "monster:M24", "language": "en"}),
        )
        .unwrap();
    assert!(monster["stats"]["hp"].as_i64().unwrap() > 0);
    let item = monster["related"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|row| {
            row["key"]
                .as_str()?
                .strip_prefix("item:")?
                .parse::<i64>()
                .ok()
        })
        .unwrap();
    let sources = server
        .invoke("find_item_drops", json!({"item_code": item, "limit": 50}))
        .unwrap();
    assert!(sources["results"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["key"] == "monster:M24"));
    let licorice = server
        .invoke("find_item_drops", json!({"item_code": 143, "limit": 50}))
        .unwrap();
    let deer = licorice["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == "monster:M24")
        .unwrap();
    assert_eq!(deer["drop_rate"], 0.055);
    assert_eq!(deer["drop_rate_percent"], 5.5);
    assert_eq!(deer["drop_rate_per_100000"], 5500);
    assert_eq!(deer["drops"].as_array().unwrap().len(), 1);
    assert_eq!(deer["drops"][0]["slot"], 1);
    assert_eq!(deer["drops"][0]["solo_only"], false);
    assert_eq!(deer["drops"][0]["drop_rate_per_100000"], 5500);
}

#[test]
fn calculator_preserves_unknown_prices_and_checks_materials() {
    let server = server();
    let items = server
        .invoke("search_catalog", json!({"category": "recipe", "limit": 50}))
        .unwrap();
    let (id, document) = items["results"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|row| {
            let id = row["key"]
                .as_str()?
                .strip_prefix("recipe:")?
                .parse::<i64>()
                .ok()?;
            let result = server
                .invoke("calculate_recipe", json!({"recipe_id": id, "quantity": 10}))
                .ok()?;
            Some((id, result))
        })
        .expect("calculable recipe");
    assert!(document["total"].is_null());
    assert_eq!(document["complete"], false);
    let prices: Vec<_> = document["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            json!({
                "item_code": row["item_code"], "enhance": row["enhance"], "unit_price": 0,
            })
        })
        .collect();
    let free = server
        .invoke(
            "calculate_recipe",
            json!({"recipe_id": id, "quantity": 10, "prices": prices}),
        )
        .unwrap();
    assert_eq!(free["complete"], true);
    assert_eq!(free["total"], free["crafting_fee"]);
    assert!(server
        .invoke(
            "calculate_recipe",
            json!({"recipe_id": id, "prices": [prices[0].clone(), prices[0].clone()]})
        )
        .is_err());
    assert!(server
        .invoke(
            "calculate_recipe",
            json!({"recipe_id": id, "prices": [{"item_code": 9999999, "unit_price": 1}]})
        )
        .is_err());
    assert!(server
        .invoke("calculate_recipe", json!({"recipe_id": id, "quantity": -1}))
        .is_err());
    let output = server
        .invoke("get_recipes", json!({"item_code": document["item_code"]}))
        .unwrap();
    assert!(output["results"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["recipe_id"] == id && row["calculable"] == true));
}

#[test]
fn quest_dependencies_use_internal_ids_and_return_both_directions() {
    let server = server();
    let rows = server
        .invoke("search_catalog", json!({"category": "quest", "query": "50001", "limit": 50}))
        .unwrap();
    let (child, parent) = rows["results"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|row| {
            let id = row["key"]
                .as_str()?
                .strip_prefix("quest:")?
                .parse::<i64>()
                .ok()?;
            let dependencies = server
                .invoke("get_quest_dependencies", json!({"quest_id": id}))
                .ok()?;
            let parent = dependencies["results"].as_array()?.first()?["key"]
                .as_str()?
                .strip_prefix("quest:")?
                .parse::<i64>()
                .ok()?;
            Some((id, parent))
        })
        .expect("quest with a prerequisite");
    let children = server
        .invoke(
            "get_quest_dependencies",
            json!({"quest_id": parent, "direction": "unlocks", "limit": 50}),
        )
        .unwrap();
    assert!(children["results"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["key"] == format!("quest:{child}")));
    let chain = server
        .invoke(
            "get_quest_dependencies",
            json!({"quest_id": child, "direction": "chain"}),
        )
        .unwrap();
    assert!(chain["total"].as_u64().unwrap() >= 2);
}
