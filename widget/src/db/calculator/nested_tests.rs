use super::*;

fn leaf(code: i64, price: Option<i64>) -> CalcIngredient {
    CalcIngredient {
        code,
        enhance: 0,
        title: format!("Material {code}"),
        per_batch: 1,
        unit_price: price,
        nested: None,
    }
}

fn simple(item: i64, material: i64, price: Option<i64>) -> CalcDocument {
    CalcDocument {
        version: VERSION,
        catalog_version: "offline".into(),
        language: "en-zh".into(),
        recipe_id: item,
        item_code: item,
        title: format!("Item {item}"),
        source: "Snapshot".into(),
        output: 1,
        fee_per_batch: 0,
        learn_fee: None,
        success: Some(100),
        quantity: 1,
        materials: vec![leaf(material, price)],
    }
}

fn nested_fixture() -> CalcDocument {
    let mut root = simple(100, 200, Some(99));
    root.quantity = 2;
    root.fee_per_batch = 5;
    root.materials[0].per_batch = 3;
    let mut child = simple(200, 300, Some(2));
    child.output = 4;
    child.quantity = 999; // A saved sheet's quantity does not determine demand.
    child.fee_per_batch = 10;
    child.materials[0].per_batch = 7;
    root.set_nested(&[0], Some(child)).unwrap();
    root
}

#[test]
fn nested_batches_charge_each_cost_once_and_preserve_unknown_vs_zero() {
    let mut document = nested_fixture();
    assert_eq!(document.version, NESTED_VERSION);
    assert_eq!(document.materials[0].nested.as_ref().unwrap().quantity, 6);
    // Later top-level edits derive demand again, without trusting the stored child quantity.
    document.materials[0].nested.as_mut().unwrap().quantity = 999;
    let cost = document.calculate().unwrap();
    assert_eq!(
        (cost.fee, cost.material_total, cost.total),
        (10, 48, Some(58))
    );
    assert_eq!(
        (cost.leaf_count, cost.priced_leaves, cost.priced),
        (1, 1, 1)
    );
    assert_eq!(cost.materials[0].quantity, 6);
    assert_eq!(cost.materials[0].unit_price, None);
    assert_eq!(cost.materials[0].line_total, Some(48));
    let child = cost.materials[0].nested.as_ref().unwrap();
    assert_eq!((child.batches, child.output_units, child.fee), (2, 8, 20));
    assert_eq!(child.materials[0].quantity, 14);

    document.ingredient_at_mut(&[0, 0]).unwrap().unit_price = None;
    let cost = document.calculate().unwrap();
    assert_eq!(
        (cost.known_total, cost.total, cost.priced_leaves),
        (30, None, 0)
    );
    assert_eq!(
        (cost.materials[0].known_total, cost.materials[0].line_total),
        (20, None)
    );
    document.ingredient_at_mut(&[0, 0]).unwrap().unit_price = Some(0);
    let cost = document.calculate().unwrap();
    assert_eq!(
        (cost.known_total, cost.total, cost.priced_leaves),
        (30, Some(30), 1)
    );
    document.set_nested(&[0], None).unwrap();
    assert_eq!(document.materials[0].unit_price, Some(99));
    assert_eq!(document.calculate().unwrap().total, Some(604));
}

#[test]
fn partial_child_cost_includes_known_leaves_without_claiming_completion() {
    let mut document = nested_fixture();
    let child = document.materials[0].nested.as_mut().unwrap();
    child.materials[0].unit_price = None;
    child.materials.push(leaf(400, Some(3)));
    let cost = document.calculate().unwrap();
    assert_eq!(
        (cost.material_total, cost.known_total, cost.total),
        (26, 36, None)
    );
    assert_eq!(
        (cost.leaf_count, cost.priced_leaves, cost.priced),
        (2, 1, 0)
    );
    assert_eq!(cost.materials[0].known_total, 26);
}

#[test]
fn top_level_quantity_and_independent_paths_recompute_nested_demand() {
    let mut document = nested_fixture();
    document.quantity = 3;
    let cost = document.calculate().unwrap();
    assert_eq!(cost.materials[0].quantity, 9);
    let child = cost.materials[0].nested.as_ref().unwrap();
    assert_eq!((child.batches, child.output_units), (3, 12));
    assert_eq!(cost.total, Some(87));

    let mut root = simple(100, 200, None);
    root.materials.push(leaf(201, None));
    let source = simple(200, 300, Some(2));
    root.set_nested(&[0], Some(source.clone())).unwrap();
    root.set_nested(&[1], Some(simple(201, 300, Some(2))))
        .unwrap();
    root.ingredient_at_mut(&[0, 0]).unwrap().unit_price = Some(9);
    assert_eq!(source.materials[0].unit_price, Some(2));
    assert_eq!(root.ingredient_at(&[1, 0]).unwrap().unit_price, Some(2));
    assert_eq!(root.calculate().unwrap().total, Some(11));
    assert!(root.ingredient_at(&[]).is_none());
    assert!(root.ingredient_at(&[9]).is_none());
    assert!(root.ingredient_at(&[0, 0, 0]).is_none());
}

#[test]
fn invalid_nested_selection_preserves_the_entire_previous_document() {
    let mut root = simple(100, 200, Some(4));
    let before = root.clone();
    assert!(root
        .set_nested(&[0], Some(simple(999, 300, Some(1))))
        .is_err());
    assert_eq!(root, before);
    assert!(root
        .set_nested(&[0, 0], Some(simple(200, 300, Some(1))))
        .is_err());
    assert_eq!(root, before);
    assert!(root.set_nested(&[], None).is_err());
    assert_eq!(root, before);

    let mut cycle = simple(200, 100, None);
    cycle
        .set_nested(&[0], Some(simple(100, 400, Some(1))))
        .unwrap();
    assert!(root.set_nested(&[0], Some(cycle)).is_err());
    assert_eq!(root, before);

    root.materials[0].enhance = 1;
    let before = root.clone();
    assert!(root
        .set_nested(&[0], Some(simple(200, 300, Some(1))))
        .is_err());
    assert_eq!(root, before);
}

fn chain(levels: usize) -> CalcDocument {
    let mut doc = simple(100 + levels as i64 - 1, 100 + levels as i64, Some(1));
    for index in (0..levels - 1).rev() {
        let mut parent = simple(100 + index as i64, doc.item_code, None);
        parent.version = NESTED_VERSION;
        parent.materials[0].nested = Some(Box::new(doc));
        doc = parent;
    }
    doc
}

#[test]
fn depth_and_total_nodes_are_bounded_across_siblings_not_only_per_recipe() {
    let document = chain(MAX_NESTED_DEPTH + 1);
    assert_eq!(document.calculate().unwrap().total, Some(1));
    assert!(chain(MAX_NESTED_DEPTH + 2).calculate().is_err());
    assert_eq!(
        decode_calculator(&encode_calculator(&document).unwrap()).unwrap(),
        document
    );

    let mut root = simple(1000, 2000, Some(1));
    root.materials = (2000..2128).map(|code| leaf(code, Some(1))).collect();
    for index in 0..3 {
        let mut child = simple(2000 + index, 3000, Some(1));
        child.materials = (3000..3128).map(|code| leaf(code, Some(1))).collect();
        root.set_nested(&[index as usize], Some(child)).unwrap();
    }
    assert_eq!(
        root.calculate().unwrap().leaf_count,
        MAX_TOTAL_MATERIALS - 3
    );
    let before = root.clone();
    assert!(root
        .set_nested(&[3], Some(simple(2003, 3000, Some(1))))
        .is_err());
    assert_eq!(root, before);
    // Legacy flat documents retain their original 128-material capacity.
    let mut flat = simple(1000, 2000, None);
    flat.materials = (2000..2128).map(|code| leaf(code, Some(1))).collect();
    assert_eq!(flat.calculate().unwrap().total, Some(128));
    flat.materials.push(leaf(9999, Some(1)));
    assert!(flat.calculate().is_err());
}

#[test]
fn nested_arithmetic_overflow_and_version_smuggling_are_rejected() {
    let mut doc = nested_fixture();
    doc.materials[0].nested.as_mut().unwrap().fee_per_batch = i64::MAX;
    assert!(doc.calculate().is_err());
    doc = nested_fixture();
    doc.ingredient_at_mut(&[0, 0]).unwrap().unit_price = Some(i64::MAX);
    assert!(doc.calculate().is_err());
    doc = nested_fixture();
    doc.fee_per_batch = i64::MAX / 2;
    assert!(doc.calculate().is_err());
    doc = nested_fixture();
    doc.materials[0].nested.as_mut().unwrap().output = i64::MAX;
    assert!(doc.calculate().is_ok()); // Only the required quantity is batched.
    doc.materials[0].per_batch = i64::MAX;
    assert!(doc.calculate().is_err());
    doc = nested_fixture();
    doc.version = VERSION;
    assert!(doc.calculate().is_err());
    let code = compressed_code(&serde_json::to_vec(&doc).unwrap());
    assert!(decode_calculator(&code).is_err());
    doc.version = NESTED_VERSION + 1;
    assert!(doc.calculate().is_err());
}

#[test]
fn legacy_and_nested_codes_roundtrip_without_database_dependencies() {
    let old = simple(100, 200, Some(3));
    let old_json = serde_json::to_vec(&old).unwrap();
    assert!(!String::from_utf8_lossy(&old_json).contains("nested"));
    assert_eq!(decode_calculator(&compressed_code(&old_json)).unwrap(), old);
    let doc = nested_fixture();
    let code = encode_calculator(&doc).unwrap();
    assert!(code.starts_with("GS-C2."));
    assert_eq!(decode_calculator(&code).unwrap(), doc);
    assert!(decode_calculator(&code.replacen("GS-C2", "GS-C1", 1)).is_err());
    assert!(decode_calculator(
        &encode_calculator(&old)
            .unwrap()
            .replacen("GS-C1", "GS-C2", 1)
    )
    .is_err());
    let path = preferences_dir().join("nested-calculator-test.json");
    save_to(&path, &old).unwrap();
    save_to(&path, &doc).unwrap();
    assert_eq!(load_from(&path).unwrap(), Some(doc.clone()));
    let mut malicious = serde_json::to_value(&doc).unwrap();
    malicious["materials"][0]["nested"]["total"] = 1.into();
    let code =
        compressed_code(&serde_json::to_vec(&malicious).unwrap()).replacen("GS-C1", "GS-C2", 1);
    assert!(decode_calculator(&code).is_err());
}

#[test]
fn real_doll_nested_cost_includes_beads_stone_and_powder_once() {
    let catalog = Catalog::open().expect("db");
    let book = catalog.calculator_catalog().unwrap();
    let mut root = catalog.calculator_document(&book, 1020, "en-zh").unwrap();
    assert_eq!(root.item_code, 12438);
    for material in &mut root.materials {
        material.unit_price = Some(1);
    }
    for (code, recipe) in [(6944, 696), (6946, 653)] {
        let position = root
            .materials
            .iter()
            .position(|row| row.code == code)
            .unwrap();
        let mut child = catalog.calculator_document(&book, recipe, "en-zh").unwrap();
        for material in &mut child.materials {
            material.unit_price = Some(1);
        }
        if code == 6946 {
            let position = child
                .materials
                .iter()
                .position(|row| row.code == 5241)
                .unwrap();
            let mut powder = catalog.calculator_document(&book, 2613, "en-zh").unwrap();
            for material in &mut powder.materials {
                material.unit_price = Some(1);
            }
            child.set_nested(&[position], Some(powder)).unwrap();
        }
        root.set_nested(&[position], Some(child)).unwrap();
    }
    let cost = root.calculate().unwrap();
    assert_eq!(cost.total, Some(141_000_402));
    assert_eq!(cost.fee, 100_000_000);
    assert_eq!(
        cost.materials
            .iter()
            .find(|row| row.key.code == 6944)
            .unwrap()
            .line_total,
        Some(20_000_100)
    );
    assert_eq!(
        cost.materials
            .iter()
            .find(|row| row.key.code == 6946)
            .unwrap()
            .line_total,
        Some(21_000_270)
    );
    assert_eq!(
        decode_calculator(&encode_calculator(&root).unwrap()).unwrap(),
        root
    );
}
