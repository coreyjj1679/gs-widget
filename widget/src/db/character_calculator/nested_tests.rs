use super::*;
use crate::db::calculator::{CalcIngredient, MAX_NESTED_DEPTH};

fn document() -> CharacterCalcDocument {
    CharacterCalcDocument {
        version: VERSION,
        language: "zh".into(),
        hero_code: "T49".into(),
        title: "持國天王".into(),
        route_id: "combination:0".into(),
        route_title: "合成".into(),
        conditions: String::new(),
        quantity: 2,
        inputs: vec![
            CharacterCalcInput {
                unit_price: Some(100),
                ..input("hero:E85".into(), "hero", "麒麟".into(), 1)
            },
            CharacterCalcInput {
                unit_price: Some(20),
                ..input("item:140".into(), "item", "材料".into(), 3)
            },
            CharacterCalcInput {
                unit_price: Some(10),
                ..input("fee:acquisition".into(), "fee", "費用".into(), 1)
            },
        ],
        nested: BTreeMap::new(),
    }
}

fn ingredient(code: i64, per_batch: i64, unit_price: Option<i64>) -> CalcIngredient {
    CalcIngredient {
        code,
        enhance: 0,
        title: format!("Material {code}"),
        per_batch,
        unit_price,
        nested: None,
    }
}

fn recipe(code: i64) -> CalcDocument {
    CalcDocument {
        version: 1,
        catalog_version: String::new(),
        language: "zh".into(),
        recipe_id: code,
        item_code: code,
        title: format!("Item {code}"),
        source: "unified".into(),
        output: 4,
        fee_per_batch: 7,
        learn_fee: Some(999),
        success: None,
        quantity: 1,
        materials: vec![ingredient(code + 1, 2, Some(11)), ingredient(code + 2, 3, None)],
    }
}

#[test]
fn nested_demand_rounds_batches_keeps_unknown_leaves_and_restores_manual_price() {
    let mut doc = document();
    doc.set_item_nested("item:140", Some(recipe(140))).unwrap();
    assert_eq!(doc.version, NESTED_VERSION);
    assert_eq!(doc.inputs[1].unit_price, Some(20));
    let cost = doc.calculate().unwrap();
    let line = &cost.inputs[1];
    let nested = line.nested.as_ref().unwrap();
    assert_eq!((line.quantity, line.unit_price, line.line_total), (6, None, None));
    assert_eq!((nested.batches, nested.output_units, nested.fee), (2, 8, 14));
    assert_eq!((line.known_total, cost.known_total, cost.material_total, cost.fee),
        (58, 278, 258, 20));
    assert_eq!((cost.priced, cost.leaf_count, cost.priced_leaves), (2, 4, 3));
    assert_eq!(cost.total, None);

    doc.nested.get_mut("item:140").unwrap().materials[1].unit_price = Some(5);
    assert_eq!(doc.calculate().unwrap().total, Some(308));
    doc.quantity = 3;
    let cost = doc.calculate().unwrap();
    let nested = cost.inputs[1].nested.as_ref().unwrap();
    assert_eq!((nested.batches, nested.output_units, nested.fee), (3, 12, 21));
    assert_eq!(nested.materials.iter().map(|line| line.quantity).collect::<Vec<_>>(), [6, 9]);
    assert_eq!(cost.total, Some(462));
    // Stored child quantity is a snapshot; current parent demand governs costing.
    assert_eq!(doc.nested["item:140"].quantity, 6);
    doc.set_item_nested("item:140", None).unwrap();
    assert_eq!(doc.inputs[1].unit_price, Some(20));
    assert_eq!(doc.calculate().unwrap().total, Some(510));
}

#[test]
fn nested_updates_are_atomic_and_only_matching_item_inputs_accept_recipes() {
    let original = document();
    for (key, child) in [
        ("item:140", recipe(999)),
        ("hero:E85", recipe(140)),
        ("fee:acquisition", recipe(140)),
        ("item:999", recipe(999)),
    ] {
        let mut doc = original.clone();
        assert!(doc.set_item_nested(key, Some(child)).is_err());
        assert_eq!(doc, original);
    }
    let mut doc = original.clone();
    let mut overflowing = recipe(140);
    overflowing.fee_per_batch = i64::MAX;
    assert!(doc.set_item_nested("item:140", Some(overflowing)).is_err());
    assert_eq!(doc, original);
    let mut malformed = original;
    malformed.version = NESTED_VERSION;
    malformed.nested.insert("item:999".into(), recipe(999));
    assert!(malformed.calculate().is_err());
}

#[test]
fn legacy_documents_and_nested_snapshots_round_trip_without_losing_prices() {
    let original = document();
    let json = serde_json::to_string(&original).unwrap();
    assert!(!json.contains("\"nested\""));
    let legacy: CharacterCalcDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(legacy, original);
    assert_eq!(legacy.calculate().unwrap().total, Some(340));

    let mut doc = original;
    doc.set_item_nested("item:140", Some(recipe(140))).unwrap();
    let path = preferences_dir().join("character-nested-test.json");
    save_to(&path, &doc).unwrap();
    assert_eq!(load_from(&path).unwrap(), Some(doc.clone()));
    let mut invalid = doc;
    invalid.version = VERSION;
    let decoded: CharacterCalcDocument =
        serde_json::from_str(&serde_json::to_string(&invalid).unwrap()).unwrap();
    assert!(decoded.calculate().is_err());
    assert!(save_to(&path, &decoded).is_err());
    assert_eq!(load_from(&path).unwrap().unwrap().version, NESTED_VERSION);
}

fn chain(code: i64, depth: usize) -> CalcDocument {
    let mut child = recipe(code);
    child.output = 1;
    child.fee_per_batch = 0;
    child.materials = vec![ingredient(code + 1, 1, Some(1))];
    if depth > 0 {
        child.version = 2;
        child.materials[0].nested = Some(Box::new(chain(code + 1, depth - 1)));
    }
    child
}

#[test]
fn item_subtrees_keep_existing_cycle_depth_and_global_row_limits() {
    let mut doc = document();
    doc.set_item_nested("item:140", Some(chain(140, MAX_NESTED_DEPTH))).unwrap();
    let valid = doc.clone();
    assert!(doc.set_item_nested("item:140", Some(chain(140, MAX_NESTED_DEPTH + 1))).is_err());
    assert_eq!(doc, valid);
    let mut cyclic = chain(140, 1);
    let middle = cyclic.materials[0].nested.as_mut().unwrap();
    middle.version = 2;
    middle.materials[0].code = 140;
    middle.materials[0].nested = Some(Box::new(recipe(140)));
    assert!(doc.set_item_nested("item:140", Some(cyclic)).is_err());
    assert_eq!(doc, valid);

    doc.inputs.clear();
    doc.nested.clear();
    for code in 1000..1004 {
        doc.inputs.push(input(format!("item:{code}"), "item", format!("Item {code}"), 1));
        let mut child = recipe(code);
        child.materials = (0..127).map(|i| ingredient(10_000 + i, 1, Some(1))).collect();
        doc.nested.insert(format!("item:{code}"), child);
    }
    assert_eq!(doc.calculate().unwrap().leaf_count, 508);
    // Four parent rows + four sets of 127 child rows = the exact shared 512 cap.
    doc.inputs.push(input("item:2000".into(), "item", "Extra".into(), 1));
    assert!(doc.calculate().unwrap_err().contains("512"));
}

#[test]
fn fire_energy_marble_uses_real_nested_recipes_and_survives_localization_and_repairs() {
    let catalog = Catalog::open_for_mcp().unwrap();
    let book = catalog.calculator_catalog().unwrap();
    let mut doc = catalog
        .character_calculator_document("V06", "upgrade:T49", "zh")
        .unwrap();
    let marble_input = doc.inputs.iter_mut().find(|row| row.key == "item:6330").unwrap();
    assert_eq!(marble_input.per_unit, 30);
    marble_input.unit_price = Some(777);
    let mut marble = catalog.calculator_document(&book, 652, "zh").unwrap();
    assert_eq!((marble.item_code, marble.output, marble.fee_per_batch), (6330, 1, 2_000_000));
    let powder = catalog.calculator_document(&book, 2613, "zh").unwrap();
    assert_eq!((powder.item_code, powder.output, powder.fee_per_batch), (5241, 10, 500_000));
    let powder_index = marble.materials.iter().position(|row| row.code == 5241).unwrap();
    marble.set_nested(&[powder_index], Some(powder)).unwrap();
    doc.set_item_nested("item:6330", Some(marble)).unwrap();
    doc.quantity = 2;
    let cost = doc.calculate().unwrap();
    let marble_line = cost.inputs.iter().find(|row| row.key == "item:6330").unwrap();
    assert_eq!(marble_line.quantity, 60);
    assert_eq!(marble_line.known_total, 126_000_000);
    let marble_cost = marble_line.nested.as_ref().unwrap();
    assert_eq!(marble_cost.fee, 120_000_000);
    let powder_cost = marble_cost.materials[powder_index].nested.as_ref().unwrap();
    assert_eq!((powder_cost.batches, powder_cost.output_units, powder_cost.fee),
        (12, 120, 6_000_000));
    assert_eq!(marble_line.line_total, None);

    let translated = catalog.localized_character_calculator_document(&doc, "en");
    assert_eq!(translated.nested["item:6330"].language, "en");
    assert_ne!(translated.nested["item:6330"].title, doc.nested["item:6330"].title);
    assert_eq!(translated.calculate().unwrap().known_total, cost.known_total);
    let powder = translated.nested["item:6330"].materials[powder_index].nested.as_ref().unwrap();
    assert_eq!(powder.language, "en");
    let missing = doc.inputs.iter().position(|row| row.kind == "item" && row.key != "item:6330").unwrap();
    let removed = doc.inputs.remove(missing);
    let repaired = catalog.supplement_character_calculator_materials(&doc).unwrap();
    assert!(repaired.inputs.contains(&removed));
    assert_eq!(repaired.nested, doc.nested);
    assert_eq!(repaired.inputs.iter().find(|row| row.key == "item:6330").unwrap().unit_price, Some(777));
}
