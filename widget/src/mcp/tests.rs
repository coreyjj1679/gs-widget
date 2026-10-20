use super::*;

fn server() -> GersangMcp {
    GersangMcp::new(db::Catalog::open_for_mcp().unwrap()).unwrap()
}

#[test]
fn tools_are_read_only_and_reject_unknown_arguments() {
    assert_eq!(tools().len(), 9);
    for name in [
        "catalog_info",
        "search_catalog",
        "get_entity",
        "find_item_drops",
        "get_quest_dependencies",
        "get_recipes",
        "calculate_recipe",
        "query_catalog",
        "get_relations",
    ] {
        assert!(
            tools().iter().any(|tool| tool.name == name),
            "missing {name}"
        );
    }
    for tool in tools() {
        let annotations = tool.annotations.as_ref().unwrap();
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert!(
            tool.output_schema.is_some(),
            "missing output schema: {}",
            tool.name
        );
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
fn invalid_calculation_arguments_do_not_load_the_calculator() {
    let server = server();
    for (tool, arguments) in [
        ("get_recipes", json!({"item_code": -1})),
        (
            "calculate_recipe",
            json!({"recipe_id": 1, "prices": [{"item_code": 1,"unit_price": -1}]}),
        ),
        ("query_catalog", json!({"query": "白水晶", "quantity": 0})),
        (
            "query_catalog",
            json!({"query": "雀天", "action": "overview", "sources": ["shops"]}),
        ),
        (
            "query_catalog",
            json!({"query": "雀天", "action": "acquisition", "sections": ["stats"]}),
        ),
        ("query_catalog", json!({"query": "白水晶", "recipe_id": 1})),
    ] {
        let result = server.result(tool, arguments);
        assert_eq!(result.is_error, Some(true));
        assert!(result.structured_content.unwrap()["error_code"].is_string());
        assert!(server.state.lock().unwrap().calculator.is_none());
    }
    let result = server
        .invoke(
            "query_catalog",
            json!({"query": "never-a-real-item-xx", "action": "calculate", "quantity": 10}),
        )
        .unwrap();
    assert_eq!(result["status"], "not_found");
    assert!(server.state.lock().unwrap().calculator.is_none());
}

#[test]
fn scoped_queries_fetch_only_requested_content_and_keep_continuations() {
    let server = server();
    let result = server
        .invoke("query_catalog", json!({"query": "SSA邊度打？", "limit": 1}))
        .unwrap();
    assert_eq!(result["status"], "resolved");
    let sources = &result["results"][0]["acquisition"];
    assert!(sources.get("monster_drops").is_some());
    for key in ["recipes", "shops", "production", "quest_rewards"] {
        assert!(sources.get(key).is_none());
    }
    let page = server
        .invoke(
            "search_catalog",
            json!({"category": "town", "filters": {"region": "japan"}, "limit": 2}),
        )
        .unwrap();
    let next = &page["next_call"];
    let second = server
        .invoke(next["tool"].as_str().unwrap(), next["arguments"].clone())
        .unwrap();
    assert_eq!(second["offset"], 2);
    assert_eq!(page["total"], second["total"]);
    assert_ne!(page["results"][0]["key"], second["results"][0]["key"]);
    let stats = server
        .invoke(
            "get_entity",
            json!({"key": "monster:M24", "sections": ["stats"]}),
        )
        .unwrap();
    assert!(stats["stats"]["hp"].is_number());
    for key in ["text", "related", "facts"] {
        assert!(stats.get(key).is_none());
    }
    let scoped = server.invoke("query_catalog", json!({"keys": ["monster:M24"], "sections": ["stats"]})).unwrap();
    for key in ["text", "related", "facts"] {
        assert!(scoped["results"][0].get(key).is_none());
    }
}

#[test]
fn one_call_materials_match_direct_calculator_and_multiple_routes_require_choice() {
    let server = server();
    let direct = server
        .invoke("calculate_recipe", json!({"recipe_id": 1, "quantity": 10}))
        .unwrap();
    let query = server
        .invoke(
            "query_catalog",
            json!({"keys": ["recipe:1"], "action": "calculate", "quantity": 10}),
        )
        .unwrap();
    let calculation = &query["results"][0]["calculation"];
    assert_eq!(calculation["status"], "calculated");
    assert_eq!(calculation["materials"], direct["materials"]);
    assert_eq!(calculation["total"], Value::Null);
    assert_eq!(calculation["requested_quantity"], 10);
    let ambiguous = server
        .invoke(
            "query_catalog",
            json!({"keys": ["item:1082"], "action": "calculate", "quantity": 3}),
        )
        .unwrap();
    let choice = &ambiguous["results"][0]["calculation"];
    assert_eq!(choice["status"], "choose_recipe");
    let next = &choice["candidates"][0]["next_call"];
    assert_eq!(next["requires_choice"], true);
    let chosen = server
        .invoke(next["tool"].as_str().unwrap(), next["arguments"].clone())
        .unwrap();
    assert!(matches!(
        chosen["results"][0]["calculation"]["status"].as_str(),
        Some("calculated" | "unavailable")
    ));
    assert!(server
        .invoke(
            "query_catalog",
            json!({"keys": ["item:1082"], "action": "calculate", "recipe_id": 1})
        )
        .is_err());
}

#[test]
fn calculation_group_continuation_preserves_quantity_and_prices() {
    let server = server();
    let price = json!({"item_code": 143, "enhance": 0, "unit_price": 5});
    let response = server.invoke("query_catalog", json!({"query": "SSA", "action": "calculate", "quantity": 7, "prices": [price], "limit": 2})).unwrap();
    assert_eq!(response["status"], "ambiguous");
    assert_eq!(response["next_call"]["arguments"]["quantity"], 7);
    assert_eq!(response["next_call"]["arguments"]["prices"], json!([price]));
    assert_eq!(response["next_calls"][0]["arguments"]["quantity"], 7);
    assert!(server.state.lock().unwrap().calculator.is_none());
    let first = server
        .invoke(
            "query_catalog",
            json!({"query": "SSA", "action": "calculate", "quantity": 7, "limit": 4}),
        )
        .unwrap();
    let next = &first["next_call"];
    let last = server
        .invoke(next["tool"].as_str().unwrap(), next["arguments"].clone())
        .unwrap();
    assert_eq!(last["status"], "ambiguous");
    assert_eq!(last["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(last["next_calls"][0]["requires_choice"], true);
    assert!(server.state.lock().unwrap().calculator.is_none());
}

#[test]
fn ambiguous_candidates_are_all_reachable_without_selecting_them() {
    let server = server();
    let mut page = server
        .invoke(
            "query_catalog",
            json!({"query": "副本傳送符(幽冥界3層)", "category": "item", "limit": 1}),
        )
        .unwrap();
    let expected = page["matched_count"].as_u64().unwrap();
    assert!(expected > 1);
    let mut keys = std::collections::HashSet::new();
    loop {
        assert_eq!(page["status"], "ambiguous");
        assert_eq!(page["results"], json!([]));
        assert!(keys.insert(page["candidates"][0]["key"].as_str().unwrap().to_string()));
        assert_eq!(page["next_calls"][0]["requires_choice"], true);
        let next = &page["next_call"];
        if next.is_null() {
            break;
        }
        page = server
            .invoke(next["tool"].as_str().unwrap(), next["arguments"].clone())
            .unwrap();
    }
    assert_eq!(keys.len() as u64, expected);
}

#[test]
fn task_queries_are_bounded_and_reject_ambiguous_argument_shapes() {
    let server = server();
    for arguments in [
        json!({}),
        json!({"query": "   "}),
        json!({"query": "雀天", "keys": ["hero:T49"]}),
        json!({"query": "雀天\n要點合"}),
        json!({"query": "雀".repeat(201)}),
        json!({"query": "雀天", "limit": 0}),
        json!({"query": "雀天", "limit": 11}),
        json!({"query": "雀天", "category": "unknown"}),
        json!({"query": "雀天", "action": "write"}),
        json!({"query": "雀天", "action": "compare"}),
        json!({"query": "雀天", "sql": "SELECT 1"}),
        json!({"keys": ["hero:T49"], "action": "compare"}),
        json!({"keys": ["hero:T49", "hero:T49"], "action": "compare"}),
        json!({"keys": [""]}),
        json!({"keys": ["hero:T49\n"]}),
        json!({"keys": ["k".repeat(101)]}),
        json!({"keys": (1..=9).map(|id| format!("item:{id}")).collect::<Vec<_>>()}),
    ] {
        let result = server.result("query_catalog", arguments.clone());
        assert_eq!(result.is_error, Some(true), "accepted {arguments}");
    }
}

#[test]
fn original_game_question_resolves_acquisition_without_search_round_trips() {
    let server = server();
    let result = server
        .invoke("query_catalog", json!({"query": "雀天要點合？"}))
        .unwrap();
    assert_eq!(result["status"], "resolved");
    assert_eq!(result["action"], "acquisition");
    let hero = &result["results"][0];
    assert_eq!(hero["key"], "hero:T49");
    let routes = hero["acquisition"]["routes"]
        .as_array()
        .expect("acquisition routes");
    assert!(
        routes.iter().any(|route| route["materials"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())),
        "{result}"
    );
    assert!(result["candidates"].as_array().is_some_and(Vec::is_empty));
}

#[test]
fn known_records_can_be_compared_in_one_call() {
    let server = server();
    let result = server
        .invoke(
            "query_catalog",
            json!({
                "keys": ["monster:M24", "monster:M25"], "action": "compare", "language": "en"
            }),
        )
        .unwrap();
    assert_eq!(result["status"], "resolved");
    assert_eq!(result["action"], "compare");
    let rows = result["results"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row["key"] == "monster:M24"));
    assert!(rows.iter().any(|row| row["key"] == "monster:M25"));
    for row in rows {
        assert!(
            row["stats"]["hp"].as_i64().is_some_and(|hp| hp > 0),
            "{row}"
        );
    }
}

#[test]
fn korean_output_is_supported_without_changing_legacy_language_defaults() {
    let server = server();
    let defaults: Search = parse(json!({"category": "item"})).unwrap();
    assert_eq!(defaults.language.code(), "zh-en");
    let english: Search = parse(json!({"category": "item", "language": "en"})).unwrap();
    assert_eq!(english.language.code(), "en-zh");
    let korean = server
        .invoke(
            "get_entity",
            json!({"key": "monster:M24", "language": "ko"}),
        )
        .unwrap();
    assert!(korean["name"].as_str().is_some_and(|name| name
        .chars()
        .any(|ch| ('\u{ac00}'..='\u{d7a3}').contains(&ch))));
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
        .invoke(
            "search_catalog",
            json!({"category": "quest", "query": "50001", "limit": 50}),
        )
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
