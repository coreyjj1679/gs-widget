use super::*;

fn fixture() -> CalcDocument {
    CalcDocument {
        version: 1,
        catalog_version: "23".into(),
        language: "zh-en".into(),
        recipe_id: 17,
        item_code: 300,
        title: "測試合成道具".into(),
        source: "煉金術".into(),
        output: 3,
        fee_per_batch: 25,
        learn_fee: Some(500),
        success: None,
        quantity: 4,
        materials: vec![
            CalcIngredient {
                code: 100,
                enhance: 0,
                title: "普通材料".into(),
                per_batch: 2,
                unit_price: Some(10),
                nested: None,
            },
            CalcIngredient {
                code: 100,
                enhance: 5,
                title: "強化材料 +5".into(),
                per_batch: 1,
                unit_price: None,
                nested: None,
            },
        ],
    }
}

fn compressed_code(body: &[u8]) -> String {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(body).unwrap();
    let packed = encoder.finish().unwrap();
    format!("{PREFIX}.{}.{}", base64_encode(&packed), checksum(&packed))
}

#[test]
fn round_batches_preserve_unknown_prices_and_zero_is_explicit() {
    let mut document = fixture();
    let result = document.calculate().unwrap();
    assert_eq!(
        (result.batches, result.output_units, result.fee),
        (2, 6, 50)
    );
    assert_eq!(result.materials[0].quantity, 4);
    assert_eq!(result.materials[0].line_total, Some(40));
    assert_eq!(result.materials[1].line_total, None);
    assert_eq!(
        (
            result.material_total,
            result.known_total,
            result.total,
            result.priced
        ),
        (40, 90, None, 1)
    );
    document.materials[1].unit_price = Some(0);
    let result = document.calculate().unwrap();
    assert_eq!((result.total, result.priced), (Some(90), 2));
    assert_eq!(result.materials[1].line_total, Some(0));
    // Learning cost is recorded as separate metadata, never silently recurring.
    assert_eq!(document.learn_fee, Some(500));
}

#[test]
fn offline_roundtrip_preserves_snapshot_and_remains_editable() {
    let document = fixture();
    let code = encode_calculator(&document).unwrap();
    assert!(code.starts_with("GS-C1."));
    let mut decoded = decode_calculator(&code).unwrap();
    assert_eq!(decoded, document);
    assert_eq!(decoded.calculate(), document.calculate());
    decoded.recipe_id = 999_999; // Need no locally matching recipe/database.
    decoded.catalog_version = "older-catalogue".into();
    decoded.quantity = 7;
    decoded.materials[1].unit_price = Some(20);
    let result = decoded.calculate().unwrap();
    assert_eq!(
        (result.batches, result.output_units, result.fee),
        (3, 9, 75)
    );
    assert_eq!(result.total, Some(195));
    assert_eq!(
        decode_calculator(&encode_calculator(&decoded).unwrap()).unwrap(),
        decoded
    );
}

#[test]
fn base64url_vectors_and_canonical_padding() {
    for (plain, encoded) in [
        ("", ""),
        ("f", "Zg"),
        ("fo", "Zm8"),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg"),
        ("fooba", "Zm9vYmE"),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64_encode(plain.as_bytes()), encoded);
        if !encoded.is_empty() {
            assert_eq!(base64_decode(encoded).unwrap(), plain.as_bytes());
        }
    }
    for invalid in ["", "Z", "Zh", "Zm9", "Zg==", "Zm+v", "Zm/v", "Zm9v\n"] {
        assert!(base64_decode(invalid).is_err(), "accepted {invalid:?}");
    }
    let bytes: Vec<u8> = (0..=255).collect();
    assert_eq!(base64_decode(&base64_encode(&bytes)).unwrap(), bytes);
}

#[test]
fn share_code_rejects_tampering_future_versions_and_unknown_fields() {
    let code = encode_calculator(&fixture()).unwrap();
    let mut damaged = code.clone().into_bytes();
    let at = damaged.len() - 1;
    damaged[at] = if damaged[at] == b'0' { b'1' } else { b'0' };
    assert!(decode_calculator(&String::from_utf8(damaged).unwrap()).is_err());
    assert!(decode_calculator(&code.replace("GS-C1", "GS-C2")).is_err());
    assert!(decode_calculator(&(code + ".extra")).is_err());
    let mut value = serde_json::to_value(fixture()).unwrap();
    value["version"] = 3.into();
    assert!(decode_calculator(&compressed_code(&serde_json::to_vec(&value).unwrap())).is_err());
    value["version"] = 1.into();
    value["total"] = 1.into(); // Derived totals must not be injected.
    assert!(decode_calculator(&compressed_code(&serde_json::to_vec(&value).unwrap())).is_err());
}

#[test]
fn decoder_bounds_compression_bombs_and_rejects_trailing_streams() {
    let bomb = compressed_code(&vec![b' '; MAX_JSON + 1]);
    assert!(bomb.len() < MAX_CODE);
    assert!(decode_calculator(&bomb).is_err());
    assert!(decode_calculator(&"x".repeat(MAX_CODE + 1)).is_err());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&serialize(&fixture()).unwrap()).unwrap();
    let mut packed = encoder.finish().unwrap();
    packed.extend_from_slice(b"trailing bytes");
    let trailing = format!("{PREFIX}.{}.{}", base64_encode(&packed), checksum(&packed));
    assert!(decode_calculator(&trailing).is_err());
}

#[test]
fn invalid_materials_prices_and_checked_arithmetic_are_rejected() {
    let mut document = fixture();
    document.materials[1].enhance = 0;
    assert!(document.calculate().is_err());
    document = fixture();
    document.materials[0].unit_price = Some(-1);
    assert!(document.calculate().is_err());
    document = fixture();
    document.quantity = 0;
    assert!(document.calculate().is_err());
    document = fixture();
    document.quantity = i64::MAX;
    assert!(document.calculate().is_err());
    document = fixture();
    document.materials[0].unit_price = Some(i64::MAX);
    assert!(document.calculate().is_err());
    document = fixture();
    document.fee_per_batch = i64::MAX;
    assert!(document.calculate().is_err());
    document = fixture();
    document.materials[0].title.push('\n');
    assert!(document.calculate().is_err());
    document = fixture();
    document.materials[0].per_batch = 0;
    assert!(document.calculate().is_err());
}

#[test]
fn persistence_roundtrips_and_preserves_invalid_or_future_data() {
    let path = preferences_dir().join("calculator-test.json");
    let document = fixture();
    assert_eq!(load_from(&path).unwrap(), None);
    save_to(&path, &document).unwrap();
    assert_eq!(load_from(&path).unwrap(), Some(document.clone()));
    for body in [
        b"broken".as_slice(),
        b"{\"version\":3,\"new_format\":true}".as_slice(),
    ] {
        std::fs::write(&path, body).unwrap();
        assert!(load_from(&path).is_err());
        assert!(save_to(&path, &document).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), body);
    }
}

#[test]
fn real_recipe_1040_uses_exact_direct_materials_and_fees() {
    let catalog = Catalog::open().expect("db");
    let book = catalog.calculator_catalog().unwrap();
    let mut document = catalog.calculator_document(&book, 1040, "zh-en").unwrap();
    assert_eq!(
        (document.item_code, document.output, document.fee_per_batch),
        (6893, 1, 60_680_000)
    );
    assert!(document.title.contains("白水晶"));
    let quantities: BTreeMap<_, _> = document
        .materials
        .iter()
        .map(|row| ((row.code, row.enhance), row.per_batch))
        .collect();
    assert_eq!(
        quantities,
        [
            ((4157, 0), 1),
            ((6847, 0), 119),
            ((6851, 0), 80),
            ((6843, 0), 52)
        ]
        .into_iter()
        .collect()
    );
    document.quantity = 3;
    for row in &mut document.materials {
        row.unit_price = Some(10);
    }
    let breakdown = document.calculate().unwrap();
    assert_eq!(breakdown.fee, 182_040_000);
    assert_eq!(breakdown.total, Some(182_047_560));
    assert_eq!(
        decode_calculator(&encode_calculator(&document).unwrap()).unwrap(),
        document
    );
}

#[test]
fn doll_catalog_uses_only_the_unified_single_unit_recipe() {
    let catalog = Catalog::open().expect("db");
    let book = catalog.calculator_catalog().unwrap();
    let recipes = catalog.calculator_recipes(&book, 12438, "en-zh");
    assert_eq!(
        recipes
            .iter()
            .map(|recipe| recipe.recipe_id)
            .collect::<Vec<_>>(),
        vec![1020]
    );
    assert_eq!(recipes[0].output, 1);
    assert!(catalog
        .index
        .recipes
        .iter()
        .all(|recipe| recipe.source != "formation"));

    let mut document = catalog.calculator_document(&book, 1020, "en-zh").unwrap();
    document.quantity = 21;
    let cost = document.calculate().unwrap();
    assert_eq!(
        (cost.batches, cost.output_units, cost.fee),
        (21, 21, 2_100_000_000)
    );
}

#[test]
fn item_dropdown_deduplicates_and_only_contains_complete_non_reinforcement_recipes() {
    let catalog = Catalog::open().expect("db");
    let book = catalog.calculator_catalog().unwrap();
    let items = catalog.calculator_items(&book, "白水晶", "zh-en");
    assert!(items.iter().any(|item| item.code == 6893));
    let unique: BTreeSet<_> = items.iter().map(|item| item.code).collect();
    assert_eq!(items.len(), unique.len());
    for item in &items {
        let recipes = catalog.calculator_recipes(&book, item.code, "zh-en");
        assert_eq!(recipes.len(), item.recipe_count);
        assert!(!recipes.is_empty());
        for recipe in recipes {
            assert!(catalog
                .calculator_document(&book, recipe.recipe_id, "zh-en")
                .is_ok());
            let source: String = catalog
                .conn
                .query_row(
                    "SELECT source FROM recipes WHERE id = ?1",
                    [recipe.recipe_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_ne!(source, "reinforce");
        }
    }
    let reinforce: i64 = catalog
        .conn
        .query_row(
            "SELECT id FROM recipes WHERE source = 'reinforce' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(catalog
        .calculator_document(&book, reinforce, "zh-en")
        .is_err());
}

#[test]
fn every_exposed_real_recipe_builds_a_valid_portable_document() {
    let catalog = Catalog::open().expect("db");
    let book = catalog.calculator_catalog().unwrap();
    assert!(!book.by_item.is_empty());
    for (&code, ids) in &book.by_item {
        assert!(code > 0 && !ids.is_empty());
        for &id in ids {
            for language in ["zh-en", "en-zh"] {
                let document = catalog
                    .calculator_document(&book, id, language)
                    .unwrap_or_else(|error| {
                        panic!("Recipe {id}, item {code}, language {language}: {error}")
                    });
                assert_eq!(document.item_code, code);
                assert!(document.calculate().unwrap().total.is_none());
                assert_eq!(
                    decode_calculator(&encode_calculator(&document).unwrap()).unwrap(),
                    document
                );
            }
        }
    }
}

#[path = "nested_tests.rs"]
mod nested_tests;
