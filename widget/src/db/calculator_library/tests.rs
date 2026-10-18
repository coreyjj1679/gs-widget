use super::*;
use crate::db::calculator::CalcIngredient;

fn document(number: i64) -> CalcDocument {
    CalcDocument {
        version: 1,
        catalog_version: "offline-source-db".into(),
        language: "zh-en".into(),
        recipe_id: number + 1,
        item_code: number + 100,
        title: format!("寶石 {number}"),
        source: "煉金術".into(),
        output: 2,
        fee_per_batch: 30,
        learn_fee: Some(500),
        success: Some(90),
        quantity: 3,
        materials: vec![
            CalcIngredient {
                code: 1,
                enhance: 0,
                title: "水晶".into(),
                per_batch: 3,
                unit_price: Some(10),
                nested: None,
            },
            CalcIngredient {
                code: 2,
                enhance: 0,
                title: "免費材料".into(),
                per_batch: 1,
                unit_price: Some(0),
                nested: None,
            },
            CalcIngredient {
                code: 3,
                enhance: 2,
                title: "尚未定價".into(),
                per_batch: 1,
                unit_price: None,
                nested: None,
            },
        ],
    }
}

fn gems() -> CalcFolderShare {
    CalcFolderShare {
        version: 1,
        name: "寶石".into(),
        entries: (0..6)
            .map(|number| SharedCalculation {
                name: format!("寶石 {number} 成本"),
                document: document(number),
            })
            .collect(),
    }
}

fn code_for(body: &[u8]) -> String {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(body).unwrap();
    packed_code(&encoder.finish().unwrap())
}

fn packed_code(packed: &[u8]) -> String {
    format!("{PREFIX}.{}.{}", base64_encode(packed), checksum(packed))
}

#[test]
fn six_sheet_folder_roundtrips_without_database_or_imported_ids() {
    let mut sender = CalcLibrary::default();
    let id = sender.import_folder(gems()).unwrap();
    let source = sender
        .folders
        .iter()
        .find(|folder| folder.id == id)
        .unwrap();
    let code = encode_calculator_folder(source).unwrap();
    assert!(code.starts_with("GS-F1."));
    let share = decode_calculator_folder(&code).unwrap();
    assert_eq!(share, gems());

    let mut receiver = CalcLibrary::default();
    receiver.next_id = 1_000;
    let id = receiver.import_folder(share).unwrap();
    assert_eq!(id, 1_000);
    let folder = receiver.folders.last().unwrap();
    assert_eq!(folder.name, "寶石");
    assert_eq!(folder.entries.len(), 6);
    for (source, received) in source.entries.iter().zip(&folder.entries) {
        assert_ne!(source.id, received.id);
        assert_eq!(source.name, received.name);
        assert_eq!(source.document, received.document);
        assert_eq!(source.document.calculate(), received.document.calculate());
        assert_eq!(received.document.materials[1].unit_price, Some(0));
        assert_eq!(received.document.materials[2].unit_price, None);
    }
    let path = preferences_dir().join("folder-roundtrip.json");
    save_to(&path, &receiver).unwrap();
    assert_eq!(load_from(&path).unwrap(), receiver);
}

#[test]
fn save_update_move_rename_and_remove_use_stable_non_reused_ids() {
    let mut library = CalcLibrary::default();
    let folder = library.new_folder("  寶石  ").unwrap();
    assert_eq!(library.folders[1].name, "寶石");
    let entry = library.save_entry(1, None, "初稿", document(1)).unwrap();
    let mut revised = document(1);
    revised.quantity = 20;
    assert_eq!(
        library
            .save_entry(folder, Some(entry), "正式價格", revised.clone())
            .unwrap(),
        entry
    );
    assert!(library.folders[0].entries.is_empty());
    assert_eq!(library.folders[1].entries[0].document, revised);
    library.rename_folder(folder, "高級寶石").unwrap();
    assert!(library.remove_folder(folder).is_err());
    library.move_entry(entry, 1).unwrap();
    library.remove_folder(folder).unwrap();
    library.remove_entry(entry).unwrap();
    assert!(library.remove_folder(1).is_err());
    let new_id = library.save_entry(1, None, "新計算", document(1)).unwrap();
    assert!(new_id > entry);
    let before = library.clone();
    assert!(library
        .save_entry(1, Some(entry), "已刪除 ID", document(1))
        .is_err());
    assert!(library.move_entry(new_id, 99_999).is_err());
    assert!(library.remove_entry(entry).is_err());
    assert_eq!(library, before);
}

#[test]
fn imports_append_new_folders_without_overwriting_name_collisions() {
    let mut library = CalcLibrary::default();
    let original_id = library.import_folder(gems()).unwrap();
    let original = library.folders[1].clone();
    let second_id = library.import_folder(gems()).unwrap();
    let third_id = library.import_folder(gems()).unwrap();
    assert_eq!(library.folders[1], original);
    assert_eq!(library.folders[2].name, "寶石 (2)");
    assert_eq!(library.folders[3].name, "寶石 (3)");
    assert!(original_id < second_id && second_id < third_id);
    let ids: Vec<_> = library
        .folders
        .iter()
        .flat_map(|folder| {
            std::iter::once(folder.id).chain(folder.entries.iter().map(|entry| entry.id))
        })
        .collect();
    assert_eq!(
        ids.iter().copied().collect::<BTreeSet<_>>().len(),
        ids.len()
    );
    assert!(library.new_folder("寶石").is_err());
    assert!(library.rename_folder(second_id, "寶石").is_err());

    let mut long = gems();
    long.name = "寶".repeat(53); // 159 UTF-8 bytes, close to the limit.
    library.import_folder(long.clone()).unwrap();
    library.import_folder(long).unwrap();
    let name = &library.folders.last().unwrap().name;
    assert!(name.ends_with(" (2)") && name.len() <= MAX_NAME);
    assert!(valid_name(name));
}

#[test]
fn failed_imports_are_atomic_including_id_counter_and_capacity() {
    let mut library = CalcLibrary::default();
    let before = library.clone();
    let mut share = gems();
    share.entries[5].document.materials[0].unit_price = Some(-1);
    assert!(library.import_folder(share).is_err());
    assert_eq!(library, before);
    let mut share = gems();
    share.entries = vec![share.entries[0].clone(); MAX_FOLDER_ENTRIES + 1];
    assert!(library.import_folder(share).is_err());
    assert_eq!(library, before);
    library.next_id = u64::MAX - 2;
    let before = library.clone();
    assert!(library.import_folder(gems()).is_err());
    assert_eq!(library, before);
}

#[test]
fn count_limits_cannot_be_bypassed_by_import_or_move() {
    let mut library = CalcLibrary::default();
    for number in 1..MAX_FOLDERS {
        library.new_folder(&format!("folder {number}")).unwrap();
    }
    let before = library.clone();
    assert!(library.new_folder("one too many").is_err());
    assert!(library.import_folder(gems()).is_err());
    assert_eq!(library, before);

    let mut library = CalcLibrary::default();
    let other = library.new_folder("other").unwrap();
    let moved = library
        .save_entry(other, None, "move me", document(1))
        .unwrap();
    for number in 0..MAX_FOLDER_ENTRIES {
        library
            .save_entry(
                1,
                None,
                "same names are valid independent scenarios",
                document(number as i64),
            )
            .unwrap();
    }
    let before = library.clone();
    assert!(library.move_entry(moved, 1).is_err());
    assert!(library
        .save_entry(1, Some(moved), "move while saving", document(1))
        .is_err());
    assert!(library
        .save_entry(1, None, "one too many", document(1))
        .is_err());
    assert_eq!(library, before);

    let mut library = CalcLibrary::default();
    let mut share = gems();
    share.entries = vec![share.entries[0].clone(); MAX_FOLDER_ENTRIES];
    for _ in 0..4 {
        library.import_folder(share.clone()).unwrap();
    }
    let before = library.clone();
    assert!(library.import_folder(gems()).is_err());
    assert!(library.save_entry(1, None, "257", document(1)).is_err());
    assert_eq!(library, before);
}

#[test]
fn unknown_fields_ids_duplicate_fields_and_deep_json_are_rejected() {
    let base = serde_json::to_value(gems()).unwrap();
    for path in ["script", "id", "path", "url", "next_id", "total"] {
        let mut injected = base.clone();
        injected[path] = "malicious extra data".into();
        assert!(
            decode_calculator_folder(&code_for(&serde_json::to_vec(&injected).unwrap())).is_err()
        );
    }
    let mut injected = base.clone();
    injected["entries"][0]["id"] = 1.into();
    assert!(decode_calculator_folder(&code_for(&serde_json::to_vec(&injected).unwrap())).is_err());
    let mut injected = base.clone();
    injected["entries"][0]["document"]["total"] = 1.into();
    assert!(decode_calculator_folder(&code_for(&serde_json::to_vec(&injected).unwrap())).is_err());
    let json = String::from_utf8(serde_json::to_vec(&gems()).unwrap()).unwrap();
    let duplicate = json.replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
    assert!(decode_calculator_folder(&code_for(duplicate.as_bytes())).is_err());
    let nested = format!("{}0{}", "[".repeat(200), "]".repeat(200));
    assert!(decode_calculator_folder(&code_for(nested.as_bytes())).is_err());
}

#[test]
fn version_tamper_invalid_base64_and_checksum_are_rejected() {
    let share = gems();
    let valid = code_for(&serialize_share(&share).unwrap());
    assert!(decode_calculator_folder(&format!("{valid}.extra")).is_err());
    assert!(decode_calculator_folder(&valid.replace("GS-F1", "GS-F2")).is_err());
    assert!(decode_calculator_folder(&valid.replace("GS-F1", "GS-C1")).is_err());
    let mut tampered = valid.clone().into_bytes();
    let end = tampered.len() - 1;
    tampered[end] = if tampered[end] == b'0' { b'1' } else { b'0' };
    assert!(decode_calculator_folder(std::str::from_utf8(&tampered).unwrap()).is_err());
    for body in ["A", "Zh", "Zg==", "+_", "/_", "a\nb"] {
        assert!(decode_calculator_folder(&format!("{PREFIX}.{body}.{}", "0".repeat(32))).is_err());
    }
    let mut future = share;
    future.version = 2;
    assert!(decode_calculator_folder(&code_for(&serde_json::to_vec(&future).unwrap())).is_err());
}

#[test]
fn compressed_bombs_trailing_streams_and_oversize_input_are_rejected() {
    let bomb = code_for(&vec![b' '; MAX_JSON + 1]);
    assert!(bomb.len() < MAX_CODE);
    assert!(decode_calculator_folder(&bomb).is_err());
    assert!(decode_calculator_folder(&"x".repeat(MAX_CODE + 1)).is_err());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&serialize_share(&gems()).unwrap())
        .unwrap();
    let packed = encoder.finish().unwrap();
    let mut trailing = packed.clone();
    trailing.extend_from_slice(b"extra bytes");
    assert!(decode_calculator_folder(&packed_code(&trailing)).is_err());
    let mut concatenated = packed.clone();
    concatenated.extend_from_slice(&packed);
    assert!(decode_calculator_folder(&packed_code(&concatenated)).is_err());
    assert!(decode_calculator_folder(&packed_code(&vec![1; MAX_PACKED + 1])).is_err());
}

#[test]
fn invalid_documents_are_rejected_even_with_recomputed_checksum() {
    let base = gems();
    for invalid in 0..6 {
        let mut share = base.clone();
        let doc = &mut share.entries[5].document;
        match invalid {
            0 => doc.quantity = -1,
            1 => doc.materials[0].unit_price = Some(-1),
            2 => doc.materials[0].unit_price = Some(i64::MAX),
            3 => doc.quantity = i64::MAX,
            4 => doc.materials.push(doc.materials[0].clone()),
            _ => doc.version = 3,
        }
        let code = code_for(&serde_json::to_vec(&share).unwrap());
        assert!(decode_calculator_folder(&code).is_err());
    }
}

#[test]
fn labels_remain_inert_text_and_never_become_paths_or_commands() {
    let mut share = gems();
    share.name = "../宝石;$(touch INJECTED)".into();
    share.entries[0].name = "<script>alert('test')</script>".into();
    share.entries[0].document.title = "</text><image href=\"file:///secret\"/>".into();
    share.entries[0].document.materials[0].title =
        "${process.exit()} `echo test` javascript:test".into();
    let decoded = decode_calculator_folder(&code_for(&serialize_share(&share).unwrap())).unwrap();
    assert_eq!(decoded, share);
    let mut library = CalcLibrary::default();
    library.import_folder(decoded).unwrap();
    save_calculator_library(&library).unwrap();
    assert_eq!(load_calculator_library().unwrap(), library);
    let files: Vec<_> = std::fs::read_dir(preferences_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files, [FILE_NAME]);
}

#[test]
fn names_reject_controls_bidi_empty_and_byte_overflow() {
    let mut library = CalcLibrary::default();
    for name in [
        "",
        "  ",
        "寶石\n",
        "\t寶石",
        "a\0b",
        "a\u{202e}b",
        "a\u{2066}b",
    ] {
        assert!(library.new_folder(name).is_err(), "accepted {name:?}");
        let mut share = gems();
        share.name = name.into();
        assert!(library.import_folder(share).is_err());
    }
    assert!(library.new_folder(&"寶".repeat(54)).is_err());
    assert!(library
        .save_entry(1, None, "bad\rname", document(1))
        .is_err());
    assert_eq!(library, CalcLibrary::default());
}

#[test]
fn local_duplicate_ids_next_id_reuse_and_duplicate_folders_are_rejected() {
    let mut library = CalcLibrary::default();
    library.import_folder(gems()).unwrap();
    let valid = library.clone();
    library.folders[1].entries[0].id = 1;
    assert!(serialize_library(&library).is_err());
    library = valid.clone();
    library.next_id = library.folders[1].entries[0].id;
    assert!(serialize_library(&library).is_err());
    library = valid.clone();
    library.folders[1].name = library.folders[0].name.clone();
    assert!(serialize_library(&library).is_err());
    library = valid;
    library.next_id = u64::MAX;
    let before = library.clone();
    assert!(library.new_folder("exhausted").is_err());
    assert_eq!(library, before);
}

#[test]
fn corrupt_future_and_oversize_storage_are_preserved() {
    let path = preferences_dir().join("protected-library.json");
    let mut library = CalcLibrary::default();
    assert_eq!(load_from(&path).unwrap(), library);
    library.import_folder(gems()).unwrap();
    save_to(&path, &library).unwrap();
    assert_eq!(load_from(&path).unwrap(), library);
    for body in [
        b"broken".to_vec(),
        b"{\"version\":2,\"future\":true}".to_vec(),
        vec![b' '; MAX_FILE + 1],
    ] {
        std::fs::write(&path, &body).unwrap();
        assert!(load_from(&path).is_err());
        assert!(save_to(&path, &library).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), body);
    }
    std::fs::write(&path, serialize_library(&library).unwrap()).unwrap();
    let before = std::fs::read(&path).unwrap();
    library.folders[1].entries[0].document.quantity = 0;
    assert!(save_to(&path, &library).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn valid_but_oversized_snapshots_cannot_exceed_bundle_or_storage_caps() {
    let mut large = document(1);
    large.materials = (1..=128)
        .map(|code| CalcIngredient {
            code,
            enhance: 0,
            title: "x".repeat(1_024),
            per_batch: 1,
            unit_price: Some(1),
            nested: None,
        })
        .collect();
    assert!(large.calculate().is_ok());
    let share = CalcFolderShare {
        version: 1,
        name: "large".into(),
        entries: vec![
            SharedCalculation {
                name: "large sheet".into(),
                document: large.clone()
            };
            32
        ],
    };
    assert!(serialize_share(&share).is_err());
    let mut library = CalcLibrary::default();
    library.folders[0].entries = (2..34)
        .map(|id| SavedCalculation {
            id,
            name: "large sheet".into(),
            document: large.clone(),
        })
        .collect();
    library.next_id = 34;
    assert!(serialize_library(&library).is_err());
}

#[test]
fn nested_and_legacy_sheets_share_one_folder_without_live_saved_references() {
    let mut parent = document(1);
    parent.materials[2].unit_price = Some(0);
    let mut saved_source = document(99);
    saved_source.item_code = 1;
    saved_source.output = 4;
    saved_source.quantity = 999;
    saved_source.fee_per_batch = 10;
    saved_source.materials = vec![CalcIngredient {
        code: 900,
        enhance: 0,
        title: "Reusable material".into(),
        per_batch: 3,
        unit_price: Some(7),
        nested: None,
    }];
    parent.set_nested(&[0], Some(saved_source.clone())).unwrap();
    assert_eq!(parent.calculate().unwrap().total, Some(122));

    let mut sender = CalcLibrary::default();
    let source_id = sender
        .save_entry(1, None, "Original saved source", saved_source.clone())
        .unwrap();
    sender
        .save_entry(1, None, "Nested sheet", parent.clone())
        .unwrap();
    sender
        .save_entry(1, None, "Legacy sheet", document(2))
        .unwrap();
    saved_source.materials[0].unit_price = Some(99);
    sender
        .save_entry(1, Some(source_id), "Changed source", saved_source)
        .unwrap();
    sender.remove_entry(source_id).unwrap();
    assert_eq!(sender.folders[0].entries[0].document, parent);

    let code = encode_calculator_folder(&sender.folders[0]).unwrap();
    let share = decode_calculator_folder(&code).unwrap();
    assert_eq!(share.entries[0].document, parent);
    assert_eq!(share.entries[1].document.version, 1);
    let mut receiver = CalcLibrary::default();
    let id = receiver.import_folder(share).unwrap();
    let imported = receiver
        .folders
        .iter()
        .find(|folder| folder.id == id)
        .unwrap();
    assert_eq!(
        imported.entries[0].document.calculate().unwrap().total,
        Some(122)
    );
    assert_eq!(imported.entries[1].document, document(2));
    let path = preferences_dir().join("nested-library-test.json");
    save_to(&path, &receiver).unwrap();
    assert_eq!(load_from(&path).unwrap(), receiver);
}

#[test]
fn invalid_nested_folder_snapshots_are_rejected_before_import() {
    let mut parent = document(1);
    let mut child = document(99);
    child.item_code = 1;
    child.materials = vec![CalcIngredient {
        code: 900,
        enhance: 0,
        title: "Leaf".into(),
        per_batch: 1,
        unit_price: Some(1),
        nested: None,
    }];
    parent.set_nested(&[0], Some(child)).unwrap();
    let share = CalcFolderShare {
        version: 1,
        name: "Nested".into(),
        entries: vec![SharedCalculation {
            name: "Nested sheet".into(),
            document: parent,
        }],
    };
    let mut tampered = serde_json::to_value(&share).unwrap();
    tampered["entries"][0]["document"]["materials"][0]["nested"]["item_code"] = 999.into();
    assert!(decode_calculator_folder(&code_for(&serde_json::to_vec(&tampered).unwrap())).is_err());
    let mut tampered = serde_json::to_value(&share).unwrap();
    tampered["entries"][0]["document"]["version"] = 1.into();
    assert!(decode_calculator_folder(&code_for(&serde_json::to_vec(&tampered).unwrap())).is_err());
}
