use super::*;

fn relation_keys(rows: &[Hit], kind: &str) -> HashSet<String> {
    rows.iter().filter(|row| row.chip == kind).map(|row| row.key.clone()).collect()
}

fn expect_relation(rows: &[Hit], kind: &str, key: &str) {
    assert!(
        rows.iter().any(|row| row.chip == kind && row.key == key),
        "missing {kind} relation to {key}"
    );
}

#[test]
fn enchant_scroll_info_returns_raw_boundaries_and_only_requested_scrolls() {
    let catalog = Catalog::open().expect("db");
    let info = catalog.enchant_scroll_info(&[8165, 8247, 1127, 1175, 1175, 0, -1, 1480, 2287, 11642]);
    assert_eq!(info.keys().copied().collect::<HashSet<_>>(), HashSet::from([8165, 8247, 1127, 1175]));
    for (code, level, rate) in [(8165, 1, 100), (8247, 150, 10), (1127, 2, 50), (1175, 90, 25)] {
        assert_eq!((info[&code].level, info[&code].rate), (level, rate), "raw client values for {code}");
    }
    assert!(catalog.enchant_scroll_info(&[]).is_empty());
    assert!(catalog.enchant_scroll_info(&[0, -1, 1480, 2287, 11642]).is_empty());
    for lang in ["zh-en", "en-zh", "ko-zh"] {
        let sheet = catalog.item_sheet(1175, lang).expect("scroll sheet");
        let label = catalog.enchant_success_label(lang);
        assert!(sheet.3.iter().any(|(key, value)| key == &label && value == "25%"),
            "detail label and raw data agree in {lang}");
    }
}

#[test]
fn enchant_directory_exposes_every_scroll_and_source_equipment_record() {
    let catalog = Catalog::open().expect("db");
    let sections = catalog.search_scoped("enchant", "", "zh-en", "", "", "all", "");
    assert_eq!(
        sections.iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
        ["ecat:scrolls", "ecat:equipment"]
    );
    for (key, table, expected) in [
        ("ecat:scrolls", "enchant_recipes", 135),
        ("ecat:equipment", "enchant_equipment", 1317),
    ] {
        let count: usize = catalog.conn.query_row(
            &format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0)
        ).expect("source count");
        assert_eq!(count, expected, "bundled {table}");
        let rows = catalog.section_records(key, "zh-en");
        assert_eq!(rows.len(), count, "{key} must not inherit ordinary search limits");
        let unique: HashSet<_> = rows.iter().map(|row| row.key.as_str()).collect();
        assert_eq!(unique.len(), count);
        assert!(rows.iter().all(|row| row.key.starts_with("item:") && !row.title.is_empty()));
    }
    let equipment = catalog.section_records("ecat:equipment", "zh-en");
    assert!(equipment.iter().any(|row| row.key == "item:1254"), "source-only result stays browsable");
    assert!(equipment.iter().any(|row| row.key == "item:1236"), "zero-scroll result stays browsable");
    assert!(equipment.iter().any(|row| row.key == "item:12021"), "missing-scroll result stays browsable");
    assert!(catalog.section_records("ecat:unknown", "zh-en").is_empty());
}

#[test]
fn enchant_color_directory_partitions_all_scrolls_with_real_icons() {
    let catalog = Catalog::open().expect("db");
    let expected = [
        ("ecat:blue", 1127, 25),
        ("ecat:green", 1161, 28),
        ("ecat:red", 1222, 54),
        ("ecat:yellow", 1128, 12),
        ("ecat:brown", 6914, 5),
        ("ecat:other", 8165, 8),
        ("ecat:event", 3096, 3),
    ];
    let sections = catalog.search_scoped("enchant", "", "zh-en", "", "", "", "");
    assert_eq!(
        sections.iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
        expected.iter().map(|(key, _, _)| *key).collect::<Vec<_>>()
    );
    assert_eq!(
        sections.iter().map(|hit| hit.title.as_str()).collect::<Vec<_>>(),
        ["藍色", "綠色", "赤色", "黃色", "褐色", "其他", "活動"]
    );
    let mut all_keys = HashSet::new();
    for (section, (key, representative, count)) in sections.iter().zip(expected) {
        assert_eq!(section.subtitle, count.to_string(), "{key} count");
        assert!(section.icon.is_some(), "{key} has a real client icon");
        assert_eq!(section.icon, catalog.item_icon(representative), "{key} icon");
        let rows = catalog.section_records(key, "zh-en");
        assert_eq!(rows.len(), count, "{key}");
        assert!(rows.iter().any(|row| row.key == format!("item:{representative}")));
        for row in rows {
            assert!(all_keys.insert(row.key.clone()), "{} occurs in multiple colors", row.key);
        }
    }
    let all_scrolls: HashSet<_> = catalog.enchant_records("scrolls", "", "zh-en")
        .into_iter().map(|row| row.key).collect();
    assert_eq!(all_keys.len(), 135);
    assert_eq!(all_keys, all_scrolls, "color browsing retains every scroll exactly once");
}

#[test]
fn enchant_colors_follow_inventory_frames_and_event_source_ids_in_every_language() {
    let catalog = Catalog::open().expect("db");
    let groups = [
        ("blue", vec![1127]),
        ("green", vec![1161]),
        ("red", vec![1222]),
        ("yellow", vec![1128, 1168, 11636, 11645, 11648, 11786]),
        ("brown", vec![6914, 6915, 6916, 6917, 6918]),
        ("other", vec![8165, 8242, 8243, 8244, 8245, 8246, 8247, 8248]),
        ("event", vec![3096, 3097, 3098]),
    ];
    for (group, representatives) in groups {
        let expected: Vec<_> = catalog.enchant_records(group, "", "zh-en")
            .into_iter().map(|row| row.key).collect();
        for lang in ["zh-en", "en-zh", "ko-zh", "ja"] {
            let actual: Vec<_> = catalog.enchant_records(&format!("ecat:{group}"), "", lang)
                .into_iter().map(|row| row.key).collect();
            assert_eq!(actual, expected, "{group} classification must not depend on {lang} names");
            for code in &representatives {
                assert!(actual.contains(&format!("item:{code}")), "{code} in {group} for {lang}");
            }
        }
    }
    // These anniversary scrolls have the same frame as red scrolls, but the source
    // identifies them as event items. Neither required level nor 4Cls/6Cls keys
    // are a substitute for the inventory frame.
    let events: HashSet<_> = catalog.enchant_records("event", "", "zh-en")
        .into_iter().map(|row| row.key).collect();
    assert_eq!(events, HashSet::from(["item:3096".to_string(), "item:3097".to_string(), "item:3098".to_string()]));
    assert!(catalog.enchant_records("red", "3096", "zh-en").is_empty());
    assert_eq!(catalog.enchant_records("event", "#3096", "zh-en").len(), 1);
    assert_eq!(catalog.enchant_records("yellow", "11636", "zh-en").len(), 1);
}

#[test]
fn enchant_scroll_search_keeps_events_and_sorts_them_with_other_scrolls() {
    let catalog = Catalog::open().expect("db");
    let event_keys = HashSet::from(["item:3096", "item:3097", "item:3098"]);
    for lang in ["zh-en", "en-zh", "ko-zh"] {
        for query in ["", "주술"] {
            let hits = catalog.enchant_hits(query, lang);
            assert_eq!(hits.iter().filter(|hit| event_keys.contains(hit.key.as_str())).count(), 3,
                "all anniversary scrolls still match {lang} {query}");
            let level_two = hits.iter().position(|hit| hit.key == "item:1127").expect("level 2 known scroll");
            for event in event_keys.iter() {
                let position = hits.iter().position(|hit| hit.key == *event).expect("anniversary scroll");
                assert!(position < level_two, "level 1 event with known equipment follows the same sorting rule");
            }
        }
    }
}

#[test]
fn enchant_scroll_sort_places_all_known_equipment_first_then_sorts_each_group_by_level() {
    let catalog = Catalog::open().expect("db");
    let levels: HashMap<i64, i64> = catalog.conn.prepare("SELECT item_code, level FROM enchant_recipes")
        .expect("levels").query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("level rows").map(Result::unwrap).collect();
    for lang in ["zh-en", "en-zh", "ko-zh"] {
        let codes: Vec<i64> = catalog.enchant_hits("", lang).into_iter()
            .map(|hit| hit.key.strip_prefix("item:").expect("item key").parse().expect("code")).collect();
        let targets = catalog.enchant_applicable_equipment(&codes, lang);
        let expect_order = |hits: &[Hit], context: &str| {
            let ordered: Vec<i64> = hits.iter()
                .map(|hit| hit.key.strip_prefix("item:").expect("item key").parse().expect("code")).collect();
            for pair in ordered.windows(2) {
                let (before, after) = (pair[0], pair[1]);
                let before_missing = targets[&before].is_empty();
                let after_missing = targets[&after].is_empty();
                assert!(!before_missing || after_missing,
                    "{context}: every known-equipment scroll precedes every unknown: {before}/{after}");
                if before_missing == after_missing {
                    assert!(levels[&before] <= levels[&after],
                        "{context}: levels rise within each equipment-availability group: {before}/{after}");
                }
            }
        };
        for section in ["scrolls", "blue", "green", "red", "yellow", "brown", "other", "event"] {
            let hits = catalog.enchant_records(section, "", lang);
            assert!(!hits.is_empty(), "{section} fixture");
            expect_order(&hits, section);
        }
        let searched = catalog.enchant_hits("3631", lang);
        assert!(!searched.is_empty() && searched.len() < codes.len(), "material search is a real subset");
        expect_order(&searched, "material search");
        expect_order(&catalog.enchant_hits("주술", lang), "scroll name search");
        let position = |code| codes.iter().position(|candidate| *candidate == code).expect("scroll");
        assert!(position(1127) < position(1126), "level 2 known precedes level 1 unknown");
        assert!(position(8247) < position(1126), "even level 150 known precedes level 1 unknown");
        assert!(position(1187) < position(1165), "level 40 known scroll wins even with a larger code");
        assert!(position(3133) < position(1225), "level 100 known scroll wins even with a larger code");
    }
}

#[test]
fn enchant_applicable_equipment_batches_verified_bases_without_outputs_or_duplicates() {
    let catalog = Catalog::open().expect("db");
    for lang in ["zh-en", "en-zh", "ko-zh"] {
        let equipment = catalog.enchant_applicable_equipment(&[1175, 1175, 8242, 8247], lang);
        assert_eq!(equipment.len(), 3, "duplicate requested scrolls are collapsed");
        let warrior = equipment.get(&1175).expect("known scroll");
        assert_eq!(
            warrior.iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
            ["item:1103", "item:2271"]
        );
        for (hit, code) in warrior.iter().zip([1103, 2271]) {
            assert_eq!(hit.title, catalog.item_name(code, lang), "{code} {lang}");
            assert_eq!(hit.icon, catalog.item_icon(code), "{code} {lang}");
            assert!(hit.icon.is_some(), "equipment preview has its actual icon");
        }
        // Both the result table and the verified target token point to 4160.
        // The preview must show that base once, rather than the enchanted result.
        assert_eq!(
            equipment[&8242].iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
            ["item:4160"]
        );
        // This target's number is a text index, so its actual item relationship
        // comes from the explicit result table rather than a label-ID guess.
        assert_eq!(
            equipment[&8247].iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
            ["item:8585"]
        );
    }
}

#[test]
fn enchant_applicable_equipment_marks_empty_scrolls_without_guessing_category_targets() {
    let catalog = Catalog::open().expect("db");
    let equipment = catalog.enchant_applicable_equipment(
        &[1126, 1127, 8165, 0, 11642, 12021, 1254, 1968, 2287], "zh-en"
    );
    assert_eq!(equipment.keys().copied().collect::<HashSet<_>>(), HashSet::from([1126, 1127, 8165]));
    assert!(equipment[&1126].is_empty(), "ALL is not a list of verified equipment");
    assert!(equipment[&8165].is_empty(), "restoration references are not applicable base links");
    assert_eq!(
        equipment[&1127].iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
        ["item:51"],
        "source rows with scroll 0 must not be assigned by their prefix text"
    );
    assert!(catalog.enchant_applicable_equipment(&[], "zh-en").is_empty());

    let codes: Vec<_> = catalog.enchant_hits("", "zh-en").into_iter()
        .filter_map(|hit| hit.key.strip_prefix("item:")?.parse::<i64>().ok()).collect();
    let all = catalog.enchant_applicable_equipment(&codes, "zh-en");
    assert_eq!(all.len(), 135, "every real scroll remains identifiable, including empty targets");
    for targets in all.values() {
        let unique: HashSet<_> = targets.iter().map(|hit| hit.key.as_str()).collect();
        assert_eq!(unique.len(), targets.len(), "a batch must never duplicate equipment");
    }
}

#[test]
fn enchant_preview_materials_preserve_client_slot_order_quantities_names_and_icons() {
    let catalog = Catalog::open().expect("db");
    let expected = [(642, 100), (205, 100), (206, 1), (150, 500), (148, 500), (147, 6), (154, 9)];
    for lang in ["zh-en", "en-zh", "ko-zh"] {
        let materials = catalog.enchant_preview_materials(&[1126, 1126, 1175, 8247, 1480, 2287, 0], lang);
        assert_eq!(materials.keys().copied().collect::<HashSet<_>>(), HashSet::from([1126, 1175, 8247]));
        let basic = &materials[&1126];
        assert_eq!(basic.len(), expected.len());
        for (hit, (code, qty)) in basic.iter().zip(expected) {
            assert_eq!(hit.key, format!("item:{code}"), "client slot order is retained");
            assert_eq!(hit.subtitle, format!("× {qty}"));
            assert_eq!(hit.title, catalog.item_name(code, lang), "quantity stays outside the item name");
            assert_eq!(hit.icon, catalog.item_icon(code));
            assert!(hit.icon.is_some(), "{code} preview has its actual icon");
        }
        assert_eq!(
            materials[&1175].iter().map(|hit| (hit.key.as_str(), hit.subtitle.as_str())).collect::<Vec<_>>(),
            [("item:2204", "× 5"), ("item:2205", "× 5"), ("item:2206", "× 5"), ("item:3631", "× 5")]
        );
        assert_eq!(materials[&8247].last().map(|hit| (hit.key.as_str(), hit.subtitle.as_str())),
            Some(("item:3631", "× 150")), "a shared material keeps each recipe's own quantity");
    }
    let codes: Vec<_> = catalog.enchant_hits("", "zh-en").into_iter()
        .filter_map(|hit| hit.key.strip_prefix("item:")?.parse::<i64>().ok()).collect();
    let all = catalog.enchant_preview_materials(&codes, "zh-en");
    assert_eq!(all.len(), 135);
    assert_eq!(all.values().map(Vec::len).sum::<usize>(), 754, "every source material slot remains visible");
    assert!(catalog.enchant_preview_materials(&[], "zh-en").is_empty());
    assert!(catalog.enchant_preview_materials(&[1480, 2287, 0], "zh-en").is_empty());
}

#[test]
fn enchant_preview_materials_keep_empty_recipes_and_do_not_reuse_another_recipes_quantity() {
    let mut catalog = Catalog::open().expect("db");
    // Only an in-memory fixture is written. Its item names and icons still come
    // from the catalog index, and the bundled database remains read-only.
    catalog.conn = Connection::open_in_memory().expect("fixture");
    catalog.conn.execute_batch(
        "CREATE TABLE enchant_recipes (item_code INTEGER PRIMARY KEY, recipe_key TEXT);
         CREATE TABLE enchant_materials (recipe_key TEXT, slot INTEGER, item_code INTEGER, qty INTEGER);
         INSERT INTO enchant_recipes VALUES (1127, 'first'), (1175, 'second'), (8165, 'empty');
         INSERT INTO enchant_materials VALUES ('first', 2, 2204, 5), ('first', 1, 2206, 20), ('second', 1, 2204, 9);"
    ).expect("material fixture");
    let materials = catalog.enchant_preview_materials(&[1127, 1175, 8165, 1480], "zh-en");
    assert_eq!(materials.len(), 3);
    assert!(materials[&8165].is_empty(), "known scroll with no source rows retains an empty preview");
    assert_eq!(
        materials[&1127].iter().map(|hit| (hit.key.as_str(), hit.subtitle.as_str())).collect::<Vec<_>>(),
        [("item:2206", "× 20"), ("item:2204", "× 5")],
        "slot order controls preview order, not insertion order or item code"
    );
    assert_eq!(
        materials[&1175].iter().map(|hit| (hit.key.as_str(), hit.subtitle.as_str())).collect::<Vec<_>>(),
        [("item:2204", "× 9")]
    );
}

#[test]
fn enchant_ability_previews_keep_bonuses_penalties_and_known_zero_distinct_from_unknown() {
    let catalog = Catalog::open().expect("db");
    for (lang, warrior, element, penalty, missing, zero) in [
        ("zh-en", "敏捷 +10 · 生命力 +20", "屬性值 +1", "敏捷 -5", "未記錄", "來源未列數值加成"),
        ("en-zh", "Agility +10 · Vitality +20", "Element +1", "Agility -5", "Not recorded", "No numeric bonuses listed"),
        ("ko-zh", "민첩 +10 · 생명력 +20", "속성값 +1", "민첩 -5", "기록 없음", "원본에 수치 보너스 없음"),
    ] {
        let previews = catalog.enchant_preview_abilities(
            &[1175, 1175, 8247, 3127, 3096, 3097, 3098, 1126, 8165, 1480, 0, 11642], lang
        );
        assert_eq!(previews.keys().copied().collect::<HashSet<_>>(),
            HashSet::from([1175, 8247, 3127, 3096, 3097, 3098, 1126, 8165]));
        assert_eq!(previews[&1175].summary, warrior);
        assert_eq!(previews[&8247].summary, element);
        assert!(previews[&3127].summary.contains(penalty), "negative bonuses remain visible");
        for code in [1175, 8247, 3127] {
            assert!(previews[&code].recorded);
            assert!(!previews[&code].summary.contains("140") && !previews[&code].summary.contains("245"),
                "equipment requirements do not become bonus text");
        }
        for code in [3096, 3097, 3098] {
            assert!(previews[&code].recorded, "a linked all-zero profile is known");
            assert_eq!(previews[&code].summary, zero, "inherited flags and zero values are not bonuses");
        }
        for code in [1126, 8165] {
            assert!(!previews[&code].recorded, "category text and reverse restoration links prove no bonus");
            assert_eq!(previews[&code].summary, missing);
        }
    }
    let codes: Vec<_> = catalog.enchant_hits("", "zh-en").into_iter()
        .filter_map(|hit| hit.key.strip_prefix("item:")?.parse::<i64>().ok()).collect();
    let all = catalog.enchant_preview_abilities(&codes, "zh-en");
    assert_eq!(all.len(), 135);
    assert_eq!(all.values().filter(|preview| preview.recorded).count(), 63);
    assert_eq!(all.values().filter(|preview| !preview.recorded).count(), 72);
    assert!(all.values().all(|preview| !preview.summary.is_empty()));
    assert!(catalog.enchant_preview_abilities(&[], "zh-en").is_empty());
}

#[test]
fn enchant_ability_previews_group_different_effects_by_equipment_and_preserve_missing_profiles() {
    let mut catalog = Catalog::open().expect("db");
    let schema: String = catalog.conn.query_row(
        "SELECT sql FROM sqlite_master WHERE name='enchant_effects'", [], |row| row.get(0)
    ).expect("effect schema");
    let effects: Vec<Vec<rusqlite::types::Value>> = {
        let mut stmt = catalog.conn.prepare(
            "SELECT * FROM enchant_effects WHERE effect_key IN ('3Cls_[Warrior]', '2Cls_[Cow]')"
        ).expect("source fixture profiles");
        let columns = stmt.column_count();
        let rows = stmt.query_map([], |row| (0..columns).map(|column| row.get(column))
            .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()).expect("profile rows");
        rows.map(Result::unwrap).collect()
    };
    // Deliberately equal names with distinct base codes exercise display-name
    // deduplication. All changes are restricted to this in-memory fixture.
    for code in [1103, 1968] {
        catalog.index.items.iter_mut().find(|item| item.code == code).expect("fixture base").en = "Shared gloves".into();
    }
    let second_name = catalog.item_name(2271, "en");
    let missing_name = catalog.item_name(215, "en");
    catalog.conn = Connection::open_in_memory().expect("fixture");
    catalog.conn.execute_batch(&schema).expect("effect fixture schema");
    catalog.conn.execute_batch(
        "CREATE TABLE enchant_recipes (item_code INTEGER, level INTEGER, success_rate INTEGER);
         CREATE TABLE enchant_equipment (scroll_item INTEGER, effect_key TEXT, base_item INTEGER, scroll_status TEXT);
         INSERT INTO enchant_recipes VALUES (1175, 90, 25), (1127, 2, 50);
         INSERT INTO enchant_equipment VALUES
           (1175, '3Cls_[Warrior]', 1103, 'linked'),
           (1175, '3Cls_[Warrior]', 1103, 'linked'),
           (1175, '3Cls_[Warrior]', 1968, 'linked'),
           (1175, '2Cls_[Cow]', 2271, 'linked'),
           (1127, 'missing-profile', 215, 'linked'),
           (1127, '3Cls_[Warrior]', 1103, 'no_scroll');"
    ).expect("ability fixture");
    let insert = format!("INSERT INTO enchant_effects VALUES ({})", vec!["?"; effects[0].len()].join(", "));
    for row in effects {
        catalog.conn.execute(&insert, rusqlite::params_from_iter(row)).expect("fixture profile");
    }
    let previews = catalog.enchant_preview_abilities(&[1175, 1127], "en");
    let warrior = &previews[&1175];
    assert!(warrior.recorded);
    assert_eq!(warrior.summary.matches("Shared gloves").count(), 1, "repeated base names appear once");
    assert!(warrior.summary.contains("Shared gloves:\nAgility +10 · Vitality +20"));
    assert!(warrior.summary.contains(&format!("{second_name}:\nAgility -5 · Intelligence +10")));
    assert_eq!(warrior.summary.split("\n\n").count(), 2, "different effects stay separate");
    assert!(!warrior.summary.contains("140") && !warrior.summary.contains("80"), "requirements stay separate");
    assert!(!previews[&1127].recorded);
    assert_eq!(previews[&1127].summary, "Not recorded", "unlinked text cannot fill a missing profile");

    catalog.conn.execute(
        "INSERT INTO enchant_equipment VALUES (1175, 'missing-profile', 215, 'linked')", []
    ).expect("partially missing effect");
    let previews = catalog.enchant_preview_abilities(&[1175], "en");
    assert!(!previews[&1175].recorded, "partially missing profiles are not presented as complete");
    assert!(previews[&1175].summary.contains(&format!("{missing_name}:\nNot recorded")));
    assert!(previews[&1175].summary.contains("Agility -5"));
    assert!(previews[&1175].summary.contains("Agility +10 · Vitality +20"));
    assert_eq!(previews[&1175].summary.split("\n\n").count(), 3, "missing groups are never silently dropped");
}

#[test]
fn enchant_search_resolves_names_codes_and_explicit_material_links() {
    let catalog = Catalog::open().expect("db");
    for query in ["開陽", "  8247  "] {
        let rows = catalog.search_scoped("enchant", query, "en-zh", "", "", "", "");
        assert!(rows.iter().any(|row| row.key == "item:8247"), "{query}");
        assert!(rows.iter().any(|row| row.key == "item:8254"), "{query}");
        assert!(rows.iter().all(|row| row.key.starts_with("item:")));
    }
    let material_matches = catalog.enchant_records("scrolls", "2287", "zh-en");
    assert!(material_matches.iter().any(|row| row.key == "item:1129"), "black iron links to its actual scroll");
    for query in ["1254", "#1254"] {
        let source_only_matches = catalog.enchant_records("equipment", query, "zh-en");
        assert!(source_only_matches.iter().any(|row| row.key == "item:1254"), "{query}");
    }
    let jackpot_matches = catalog.search_scoped("enchant", "1968", "zh-en", "", "", "", "");
    assert!(jackpot_matches.iter().any(|row| row.key == "item:1175"));
    assert!(jackpot_matches.iter().any(|row| row.key == "item:1480"));
    let restoration_matches = catalog.search_scoped("enchant", "8165", "zh-en", "", "", "", "");
    assert!(restoration_matches.iter().any(|row| row.key == "item:1480"));
    assert!(catalog.search_scoped("enchant", "zz-no-such-enchantment", "zh-en", "", "", "", "").is_empty());
}

#[test]
fn enchant_item_routes_link_scroll_base_result_jackpot_and_restoration() {
    let catalog = Catalog::open().expect("db");
    let (_, scroll_links, _) = catalog.open_key("item:1175", "zh-en");
    assert_eq!(
        relation_keys(&scroll_links, "enchant-result"),
        HashSet::from(["item:1480".to_string(), "item:3286".to_string()])
    );
    assert_eq!(
        relation_keys(&scroll_links, "enchant-target"),
        HashSet::from(["item:1103".to_string(), "item:2271".to_string()])
    );
    let (_, result_links, _) = catalog.open_key("item:1480", "zh-en");
    expect_relation(&result_links, "enchant-for", "item:1175");
    expect_relation(&result_links, "enchant-base", "item:1103");
    expect_relation(&result_links, "enchant-jackpot", "item:1968");
    expect_relation(&result_links, "enchant-revert", "item:8165");
    let (_, base_links, _) = catalog.open_key("item:1103", "zh-en");
    expect_relation(&base_links, "enchant-result", "item:1480");
    expect_relation(&base_links, "enchant-for", "item:1175");
    let (_, jackpot_links, _) = catalog.open_key("item:1968", "zh-en");
    expect_relation(&jackpot_links, "enchant-result", "item:1480");
    let (_, restoration_links, _) = catalog.open_key("item:8165", "zh-en");
    expect_relation(&restoration_links, "enchant-result", "item:1480");
    assert_eq!(relation_keys(&restoration_links, "enchant-result").len(), 1317,
        "restoration relationships must retain every explicit source result");
    for (key, links) in [
        ("1175", &scroll_links), ("1480", &result_links), ("1103", &base_links),
        ("1968", &jackpot_links), ("8165", &restoration_links),
    ] {
        let sections: Vec<_> = links.iter().filter(|row| row.chip == "section")
            .map(|row| row.key.as_str()).collect();
        assert_eq!(sections.len(), sections.iter().copied().collect::<HashSet<_>>().len(),
            "item:{key} must not publish duplicate accordion section keys");
    }

    // Every step uses the ordinary item route consumed by the UI history stack.
    // Reopening the prior key must reconstruct the same detail without a new key kind.
    for code in [1175, 1480, 1103, 1480, 1175, 3286, 1968, 8165] {
        let key = format!("item:{code}");
        let (text, _, _) = catalog.open_key(&key, "zh-en");
        assert_ne!(text, phrase("missing-item", "zh-en"), "{key}");
        assert!(catalog.item_sheet(code, "zh-en").is_some(), "{key}");
    }
}

#[test]
fn source_only_enchant_results_open_without_inventing_scroll_links() {
    let catalog = Catalog::open().expect("db");
    let indexed: i64 = catalog.conn.query_row(
        "SELECT COUNT(*) FROM items WHERE code=1254", [], |row| row.get(0)
    ).expect("catalog availability");
    assert_eq!(indexed, 0, "fixture deliberately exercises the source-only fallback");
    let (text, links, icon) = catalog.open_key("item:1254", "zh-en");
    assert_ne!(text, phrase("missing-item", "zh-en"));
    assert!(icon.is_some(), "source-only result inherits a usable base icon");
    let sheet = catalog.item_sheet(1254, "zh-en").expect("source-only detail");
    assert!(!sheet.0.is_empty());
    assert_eq!(sheet.2, "#1254");
    assert_eq!(catalog.copyable_name("item:1254", "zh-en", false).as_deref(), Some(sheet.0.as_str()));
    assert_eq!(catalog.item_alternate_name("item:1254", "zh-en"), sheet.1);
    expect_relation(&links, "enchant-base", "item:215");
    expect_relation(&links, "enchant-for", "item:1151");

    for (code, marker) in [(1236, "未指定"), (12021, "#11642")] {
        let (_, links, _) = catalog.open_key(&format!("item:{code}"), "zh-en");
        assert!(!links.iter().any(|row| row.key == "item:0" || row.key == "item:11642"));
        assert!(relation_keys(&links, "enchant-for").is_empty(), "{code} has no verified scroll");
        let sheet = catalog.item_sheet(code, "zh-en").expect("unlinked result detail");
        assert!(sheet.3.iter().any(|(_, value)| value.contains(marker)), "{code} must explain its source status");
    }
}

#[test]
fn enchant_details_distinguish_scroll_level_from_effect_requirements() {
    let catalog = Catalog::open().expect("db");
    let warrior = catalog.item_sheet(1175, "zh-en").expect("warrior scroll");
    assert!(warrior.3.iter().any(|(label, value)| label == "成功率" && value == "25%"));
    assert!(warrior.3.iter().any(|(label, value)| label == "最低等級" && value == "90"));
    assert!(warrior.3.iter().any(|(label, value)| label == "鬼道加成"
        && value.contains("敏捷 +10") && value.contains("生命力 +20")));
    assert!(warrior.3.iter().any(|(label, value)| label == "裝備需求" && value.contains("140")));

    let kaiyang = catalog.item_sheet(8247, "zh-en").expect("kaiyang scroll");
    assert!(kaiyang.3.iter().any(|(label, value)| label == "成功率" && value == "10%"));
    assert!(kaiyang.3.iter().any(|(label, value)| label == "最低等級" && value == "150"));
    assert!(kaiyang.3.iter().any(|(label, value)| label == "鬼道加成" && value.contains("屬性值 +1")));
    assert!(kaiyang.3.iter().any(|(label, value)| label == "裝備需求" && value.contains("245")));

    let penalty = catalog.item_sheet(3177, "zh-en").expect("negative agility enchant");
    assert!(penalty.3.iter().any(|(label, value)| label == "鬼道加成" && value.contains("敏捷 -5")));
    let source_value = catalog.item_sheet(6919, "zh-en").expect("client effect overrides old prose");
    assert!(source_value.3.iter().any(|(label, value)| label == "鬼道加成" && value.contains("生命力 +300")));
}
