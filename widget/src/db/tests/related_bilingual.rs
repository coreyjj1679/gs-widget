use super::*;
use crate::db::npc::npc_sheet_parts;
use crate::db::quest::load_quests;
use crate::db::rows::{FormulaPart, ItemFormula, NpcRow};

#[test]
fn mercenary_quest_titles_use_recorded_character_translations_and_remain_searchable() {
    let catalog = Catalog::open().expect("catalog");
    let quests: Vec<_> = catalog.index.quests.iter().filter(|quest| quest.source == "mercenary").collect();
    assert!(!quests.is_empty());
    for quest in quests {
        let (en, zh, ko): (String, String, String) = catalog.conn.query_row(
            "SELECT se.text, sz.text, c.name_ko FROM quests q
             JOIN characters c ON c.code = q.title
             JOIN strings se ON se.object='OnlineText.dat' AND se.lang='en' AND se.id=printf('%08d',c.name_gts)
             JOIN strings sz ON sz.object='OnlineText.dat' AND sz.lang='zh' AND sz.id=printf('%08d',c.name_gts)
             WHERE q.id=?1", [quest.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).expect("recorded names");
        for (lang, title, alternate) in [
            ("zh-en", zh.trim(), en.trim()), ("en-zh", en.trim(), zh.trim()),
            ("zh", zh.trim(), ""), ("ko-en", ko.trim(), en.trim()),
        ] {
            let sheet = catalog.quest_sheet(quest.id, lang).expect("quest sheet");
            assert_eq!(sheet.0, title, "{} {lang}", quest.id);
            assert_eq!(sheet.1, alternate, "{} {lang}", quest.id);
            assert_eq!(catalog.copyable_name(&format!("quest:{}", quest.id), lang, false).as_deref(), Some(title));
        }
        assert!(quest.names.contains(&en.trim().to_lowercase()));
        assert!(quest.names.contains(zh.trim()));
    }
}

#[test]
fn mercenary_title_fallback_preserves_explicit_names_and_selected_secondary_semantics() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE quests (id INTEGER, source TEXT, code TEXT, title TEXT, detail TEXT, kind TEXT);
         CREATE TABLE quest_lines (quest_id INTEGER, role TEXT, slot INTEGER, gts_id INTEGER, text_object TEXT);
         CREATE TABLE characters (code TEXT, name_ko TEXT, name_gts INTEGER);
         CREATE TABLE strings (object TEXT, lang TEXT, id TEXT, text TEXT);
         INSERT INTO characters VALUES ('U1','원본',1),('U2','같음',2),('U3','원본',3),('U4','한국어',4);
         INSERT INTO quests VALUES (1,'mercenary','1','U1','',''),(2,'mercenary','2','U2','',''),
             (3,'mercenary','3','U3','',''),(4,'mercenary','4','U4','',''),(5,'mercenary','5','missing','','');
         INSERT INTO strings VALUES ('OnlineText.dat','en','00000001','Character'),('OnlineText.dat','zh','00000001','角色'),
             ('OnlineText.dat','en','00000002',' Same '),('OnlineText.dat','zh','00000002','same'),
             ('OnlineText.dat','en','00000003','Only English'),('QuestText.dat','en','00000010','Recorded quest title');
         INSERT INTO quest_lines VALUES (1,'title',0,10,'QuestText.dat');"
    ).unwrap();
    let quests = load_quests(&conn).unwrap();
    let first = &quests[0];
    assert_eq!(first.title_en, "Recorded quest title");
    assert_eq!(first.title_zh, "角色");
    assert!(first.names.contains("recorded quest title"));
    for (index, lang, title, alternate) in [
        (0, "en-zh", "Recorded quest title", "角色"),
        (0, "zh", "角色", ""),
        (1, "zh-en", "same", ""),
        (1, "en-zh", "Same", ""),
        (2, "zh-en", "Only English", ""),
        (2, "zh-ko", "원본", ""),
        (3, "zh-en", "한국어", ""),
        (4, "en-zh", "5", ""),
    ] {
        let quest = &quests[index];
        assert_eq!(lead3(lang, &quest.title_en, &quest.title_zh, &quest.title_ko), title);
        assert_eq!(trail3(lang, &quest.title_en, &quest.title_zh, &quest.title_ko), alternate);
    }
}

fn related_fixture() -> Catalog {
    let mut catalog = Catalog::open().unwrap();
    // All writes stay in this in-memory fixture; no catalog or preference file changes.
    catalog.conn = Connection::open_in_memory().unwrap();
    catalog.conn.execute_batch(
        "CREATE TABLE items (code INTEGER, name_ko TEXT, name_gts INTEGER);
         CREATE TABLE strings (object TEXT, lang TEXT, id TEXT, text TEXT);
         CREATE TABLE recipes (id INTEGER, source TEXT, result_item INTEGER, result_qty INTEGER, fee INTEGER, learn_fee INTEGER, npc TEXT);
         CREATE TABLE recipe_materials (recipe_id INTEGER, slot INTEGER, item_code INTEGER, qty INTEGER, enhance INTEGER);
         CREATE TABLE item_bases (item_code INTEGER, base_code INTEGER, prefix_gts INTEGER);
         CREATE TABLE recipe_facility (recipe_id INTEGER, book_item INTEGER, duration_days INTEGER, influence_type INTEGER, influence_value INTEGER);
         CREATE TABLE recipe_facility_locations (recipe_id INTEGER, village_code TEXT, village_name_ko TEXT, town_code TEXT);
         INSERT INTO items VALUES (900001,'제품',1),(900002,'재료',2),(900003,'',3),(900004,'같음',4),(900005,'한국어',5),(900006,'<crafted> 재료',6);
         INSERT INTO item_bases VALUES (900006,900002,0);
         INSERT INTO strings VALUES ('OnlineText.dat','en','00000001','Product'),('OnlineText.dat','zh','00000001','成品'),
             ('OnlineText.dat','en','00000002','Material'),('OnlineText.dat','zh','00000002','素材'),
             ('OnlineText.dat','en','00000003','Only English'),
             ('OnlineText.dat','en','00000004',' Same '),('OnlineText.dat','zh','00000004','same');
         INSERT INTO recipes VALUES (1,'alchemy',900001,1,0,NULL,'');
         INSERT INTO recipe_materials VALUES (1,0,900002,4,0),(1,1,900003,2,0),(1,2,900004,3,0),(1,3,900005,1,0);
         INSERT INTO recipe_facility VALUES (1,900002,5,0,0);
         INSERT INTO recipe_facility_locations VALUES (1,'627','카트만두','');"
    ).unwrap();
    catalog.index.formulas.insert(900001, ItemFormula {
        country: 0, labor: 0, yield_n: 1, yield_max: 1, plant: String::new(),
        parts: vec![FormulaPart { kind: "mat".into(), item: 900002, qty: 4 }],
    });
    catalog
}

#[test]
fn recipe_and_production_rows_keep_alternates_quantities_and_links_separate() {
    let catalog = related_fixture();
    for (lang, title, alternate, product) in [
        ("zh-en", "素材", "Material", "成品"),
        ("en-zh", "Material", "素材", "Product"),
        ("zh", "素材", "", "成品"),
        ("ko-en", "재료", "Material", "제품"),
    ] {
        let subtitle = caption(alternate, "× 4".into());
        let recipe = catalog.recipe_material_hits(1, lang);
        let material = recipe.iter().find(|row| row.key == "item:900002").unwrap();
        assert_eq!((&*material.title, &*material.subtitle), (title, subtitle.as_str()));
        let production = catalog.production_for(900001, lang);
        let material = production.iter().find(|row| row.key == "item:900002").unwrap();
        assert_eq!((&*material.title, &*material.subtitle), (title, subtitle.as_str()));
        let uses = catalog.recipes_using(900002, lang);
        assert_eq!(uses[0].key, "recipe:1");
        assert_eq!(uses[0].title, product);
        assert!(uses[0].subtitle.ends_with("× 4"));
        let formula = catalog.formulas_using(900002, lang);
        assert_eq!(formula[0].key, "item:900001");
        assert_eq!(formula[0].title, product);
        let (text, links, _) = catalog.open_recipe(1, lang);
        assert_eq!(links[0].title, product);
        assert!(text.contains(title));
        if !alternate.is_empty() { assert!(text.contains(alternate)); }
        let facts = catalog.facility_recipe_facts(1, lang);
        let book = facts.iter().find(|(label, _)| label == &chrome_text(lang, "所需製造書", "Required recipe book")).unwrap();
        let shown = if alternate.is_empty() { title.into() } else { format!("{title} · {alternate}") };
        assert_eq!(book.1, format!("{shown} × 1"));
        assert!(facts.iter().any(|(_, value)| value == "카트만두"));
    }
    assert_eq!(catalog.item_name(900002, "zh-en"), "素材", "the shared primary-name helper stays single-language");
    let linked = catalog.item_name(900006, "zh-en");
    assert!(linked.contains("素材"));
    assert_eq!(catalog.related_item_names(900006, "zh-en"), (linked, String::new()));
    assert_eq!(catalog.related_item_names(900006, "ko-en"), ("<crafted> 재료".into(), String::new()));
    for (code, lang, title) in [(900003, "zh-en", "Only English"), (900004, "zh-en", "same"), (900004, "en-zh", "Same"), (900005, "zh-en", "한국어")] {
        assert_eq!(catalog.related_item_names(code, lang), (title.into(), String::new()));
        let rows = catalog.recipe_material_hits(1, lang);
        let row = rows.iter().find(|row| row.key == format!("item:{code}")).unwrap();
        assert!(!row.subtitle.contains('·'));
    }
}

#[test]
fn production_facility_rows_keep_main_name_and_localized_building_metadata() {
    let catalog = Catalog::open().unwrap();
    let building = catalog.index.works.iter().find(|building| building.code == "WEA").unwrap();
    let yard = &catalog.index.yards["SWD"];
    let code = yard.products.first().unwrap().item;
    for lang in ["zh-en", "en-zh", "zh", "ko-en"] {
        let hits = catalog.production_for(code, lang);
        let row = hits.iter().find(|row| row.key == "bay:SWD").unwrap();
        assert_eq!(row.title, lead3(lang, &yard.en, &yard.zh, &yard.ko));
        assert_eq!(row.subtitle, caption(
            &trail3(lang, &yard.en, &yard.zh, &yard.ko),
            show3(lang, &building.en, &building.zh, &building.ko),
        ));
    }
}

#[test]
fn npc_locations_obey_selected_secondary_and_do_not_duplicate_fallback_names() {
    let mut npc = NpcRow {
        code: 1, ko: "사람".into(), en: "Person".into(), zh: "人物".into(),
        nation_en: "China".into(), nation_zh: "中國".into(),
        map_ko: "지도".into(), map_en: "Map".into(), map_zh: "地圖".into(), map_index: 0,
        x: 1, y: 2, place_en: "Recorded place".into(), place_zh: "記錄地點".into(),
        names: String::new(), icon: None,
    };
    for (lang, name, alternate, map, place) in [
        ("zh-en", "人物", "Person", "地圖 · Map", "記錄地點\nRecorded place"),
        ("en-zh", "Person", "人物", "Map · 地圖", "Recorded place\n記錄地點"),
        ("zh", "人物", "", "地圖", "記錄地點"),
        ("ko-en", "사람", "Person", "지도 · Map", "Recorded place"),
    ] {
        let sheet = npc_sheet_parts(&npc, lang);
        assert_eq!((sheet.0.as_str(), sheet.1.as_str(), sheet.2.as_str(), sheet.4.as_str()), (name, alternate, map, place));
        assert!(sheet.3.iter().any(|(_, value)| value == map));
    }
    npc.map_zh.clear();
    npc.place_zh = " Recorded place ".into();
    let sheet = npc_sheet_parts(&npc, "zh-en");
    assert_eq!(sheet.2, "Map");
    assert_eq!(sheet.4, "Recorded place");
}

#[test]
fn unstructured_quest_objectives_keep_full_selected_languages_outside_the_compact_summary() {
    let mut catalog = Catalog::open().unwrap();
    let id = catalog.index.quests[0].id;
    catalog.conn = Connection::open_in_memory().unwrap();
    catalog.conn.execute_batch(
        "CREATE TABLE quests (id INTEGER, detail TEXT);
         CREATE TABLE quest_lines (quest_id INTEGER, role TEXT, slot INTEGER, gts_id INTEGER, text_object TEXT);
         CREATE TABLE strings (object TEXT, lang TEXT, id TEXT, text TEXT);"
    ).unwrap();
    catalog.conn.execute("INSERT INTO quests VALUES (?1,'')", [id]).unwrap();
    catalog.conn.execute("INSERT INTO quest_lines VALUES (?1,'mission',0,99,'QuestText.dat')", [id]).unwrap();
    let english = "Return to the recorded quest giver after completing the requested objective. ".repeat(3);
    let chinese = "完成已記錄目標後，返回尋找任務委託人。".repeat(5);
    for (en, zh, lang, expected) in [
        (english.as_str(), chinese.as_str(), "zh-en", format!("{}\n{}", chinese.trim(), english.trim())),
        (english.as_str(), chinese.as_str(), "en-zh", format!("{}\n{}", english.trim(), chinese.trim())),
        (english.as_str(), chinese.as_str(), "zh", chinese.trim().to_string()),
        (english.as_str(), "", "zh-en", english.trim().to_string()),
        (" Same ", "same", "zh-en", "same".to_string()),
        (english.as_str(), chinese.as_str(), "ko-en", english.trim().to_string()),
    ] {
        catalog.conn.execute("DELETE FROM strings", []).unwrap();
        for (lang, value) in [("en", en), ("zh", zh)] {
            catalog.conn.execute("INSERT INTO strings VALUES ('QuestText.dat',?1,'00000099',?2)", (lang, value)).unwrap();
        }
        let summaries = catalog.quest_list_summaries(&[id], lang);
        let row = &summaries[&id];
        assert_eq!(row.objective_details, expected);
        assert!(row.summary.chars().count() <= 60);
        let preview = catalog.quest_map_preview(id, lang);
        assert_eq!(row.summary, preview.summary);
        assert!(preview.objectives.ends_with(&expected));
    }
}
