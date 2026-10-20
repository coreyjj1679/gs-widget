//! Integration checks share the parent's single Slint platform and window.
use super::*;

const ORB: &str = "item:6330";
const POWDER: &str = "item:6330/3";
const LEAF: &str = "item:6330/3/0";

fn saved() -> db::CharacterCalcDocument {
    db::load_character_calculator().unwrap().unwrap()
}

fn row(app: &App, key: &str) -> CharacterCalcMaterial {
    character_rows(app)
        .into_iter()
        .find(|row| row.key == key)
        .unwrap_or_else(|| panic!("Missing material row: {key}"))
}

fn select_group(app: &App, catalog: &db::Catalog, id: &str) -> i32 {
    let index = catalog
        .character_calculator_groups(&app.get_lang())
        .iter()
        .position(|group| group.id == id)
        .unwrap() as i32;
    app.invoke_character_calculator_select_group(index);
    assert_eq!(app.get_character_calculator_group_index(), index);
    index
}

fn price(app: &App, key: &str, value: &str) {
    assert!(
        app.invoke_character_calculator_price_changed(key.into(), value.into()),
        "Cannot set {key} to {value}: {}",
        app.get_character_calculator_error()
    );
}

fn assert_options_include(app: &App, key: &str, reuse: bool, option: &str) {
    app.invoke_character_calculator_request_nested_options(key.into(), reuse);
    assert!(
        app.get_character_calculator_nested_options()
            .iter()
            .any(|row| row.id == option),
        "Missing source {option} for {key}"
    );
}

pub(super) fn check_character_groups_and_nested_materials(
    app: &App,
    window: &MinimalSoftwareWindow,
    catalog: &Rc<db::Catalog>,
) {
    super::super::super::install(app, catalog.clone(), Rc::new(RefCell::new(HashMap::new())));
    app.invoke_show_record_calculator(1);
    set_language(app, "zh-en");
    let item_before = db::load_calculator().unwrap().unwrap();
    app.invoke_character_calculator_select_character("V06".into());
    assert_eq!(saved().route_id, "upgrade:T49");
    assert!(app.invoke_character_calculator_quantity_changed("2".into()));
    for input in saved().inputs {
        price(app, &input.key, "0");
    }
    price(app, ORB, "12345");
    assert_eq!(amount(&row(app, ORB).quantity), 60);

    // Browsing and filtering must not select a different character or discard
    // partially typed values in the current calculation.
    let other_key = saved()
        .inputs
        .iter()
        .find(|input| input.key != ORB)
        .unwrap()
        .key
        .clone();
    assert!(!app.invoke_character_calculator_quantity_changed("group-draft".into()));
    assert!(!app.invoke_character_calculator_price_changed(other_key.clone().into(), "9x".into()));
    let before_groups = saved();
    app.invoke_character_calculator_search("".into());
    let kings = select_group(app, catalog, "9");
    assert!(app
        .get_character_calculator_candidates()
        .iter()
        .any(|candidate| candidate.id == "T51"));
    assert!(!app
        .get_character_calculator_candidates()
        .iter()
        .any(|candidate| candidate.id == "E74"));
    app.invoke_character_calculator_search("T51".into());
    assert_eq!(app.get_character_calculator_candidates().row_count(), 1);
    assert_eq!(
        app.get_character_calculator_candidates()
            .row_data(0)
            .unwrap()
            .id,
        "T51"
    );
    select_group(app, catalog, "8");
    assert_eq!(app.get_character_calculator_candidates().row_count(), 0);
    select_group(app, catalog, "9");
    for language in ["en-zh", "ko-zh", "zh-en"] {
        set_language(app, language);
        assert_eq!(app.get_character_calculator_group_index(), kings);
        assert_eq!(app.get_character_calculator_candidates().row_count(), 1);
        assert_eq!(saved(), before_groups);
        assert!(app.get_character_calculator_quantity_has_draft());
        assert_eq!(app.get_character_calculator_quantity_draft(), "group-draft");
        assert_eq!(row(app, &other_key).draft, "9x");
    }
    assert!(app.invoke_character_calculator_quantity_changed("2".into()));
    price(app, &other_key, "0");
    app.invoke_character_calculator_search("".into());

    // V06 needs 30 fire orbs. Each orb's recipe needs two five-colour powders;
    // the powder recipe produces ten units, so this exercises two nested levels.
    assert!(row(app, ORB).can_craft);
    assert_options_include(app, ORB, false, "recipe:652");
    app.invoke_character_calculator_set_source(ORB.into(), "recipe:652".into());
    assert!(row(app, ORB).nested);
    assert_eq!(saved().nested[ORB].item_code, 6330);
    assert_eq!(
        saved()
            .inputs
            .iter()
            .find(|input| input.key == ORB)
            .unwrap()
            .unit_price,
        Some(12345)
    );
    assert!(!app.get_character_calculator_complete());
    assert_eq!(app.get_character_calculator_unit_total(), "—");
    assert_eq!(amount(&app.get_character_calculator_total()), 120_000_000);
    assert_eq!(row(app, POWDER).depth, 1);
    assert!(row(app, POWDER).can_craft);
    assert_options_include(app, POWDER, false, "recipe:2613");
    app.invoke_character_calculator_set_source(POWDER.into(), "recipe:2613".into());
    assert!(row(app, POWDER).nested);
    assert_eq!(row(app, LEAF).depth, 2);
    assert!(!row(app, LEAF).context.is_empty());
    assert_eq!(amount(&row(app, LEAF).quantity), 120);
    assert_eq!(amount(&app.get_character_calculator_total()), 126_000_000);
    for material in character_rows(app)
        .into_iter()
        .filter(|material| !material.nested)
    {
        price(
            app,
            &material.key,
            if material.depth > 0 { "10" } else { "0" },
        );
    }
    assert!(app.get_character_calculator_complete());
    assert_eq!(amount(&app.get_character_calculator_total()), 126_016_200);
    let before_reselect = saved();
    let before_reselect_revision = app.get_character_calculator_input_revision();
    app.invoke_character_calculator_select_character("V06".into());
    assert_eq!(saved(), before_reselect);
    assert_eq!(
        app.get_character_calculator_input_revision(),
        before_reselect_revision
    );
    let costs = saved().calculate().unwrap();
    let orb_cost = costs.inputs.iter().find(|input| input.key == ORB).unwrap();
    assert_eq!(orb_cost.nested.as_ref().unwrap().batches, 60);
    assert_eq!(
        orb_cost.nested.as_ref().unwrap().materials[3]
            .nested
            .as_ref()
            .unwrap()
            .batches,
        12
    );

    // Leaf drafts survive edits elsewhere, quantity changes and relabelling.
    assert!(!app.invoke_character_calculator_price_changed(LEAF.into(), "123x".into()));
    app.invoke_character_calculator_select_character("V06".into());
    assert_eq!(saved(), before_reselect);
    assert!(row(app, LEAF).has_draft);
    assert_eq!(row(app, LEAF).draft, "123x");
    price(app, "item:6330/0", "20");
    assert!(app.invoke_character_calculator_quantity_changed("3".into()));
    set_language(app, "en-zh");
    assert!(row(app, LEAF).has_draft);
    assert_eq!(row(app, LEAF).draft, "123x");
    assert_eq!(amount(&row(app, LEAF).quantity), 180);
    assert!(!app.get_character_calculator_complete());
    assert_options_blocked_by_draft(app);
    price(app, LEAF, "10");
    assert!(app.get_character_calculator_complete());
    assert_eq!(amount(&app.get_character_calculator_total()), 189_033_300);
    let costs = saved().calculate().unwrap();
    let orb_cost = costs.inputs.iter().find(|input| input.key == ORB).unwrap();
    assert_eq!(orb_cost.nested.as_ref().unwrap().fee, 180_000_000);
    assert_eq!(
        orb_cost.nested.as_ref().unwrap().materials[3]
            .nested
            .as_ref()
            .unwrap()
            .fee,
        9_000_000
    );
    price(app, LEAF, "");
    assert!(!app.get_character_calculator_complete());
    assert!(row(app, LEAF).missing_price);
    assert!(row(app, ORB).missing_price);
    assert_eq!(amount(&app.get_character_calculator_total()), 189_031_500);
    price(app, LEAF, "10");

    // A saved item calculation becomes an independent snapshot in the character
    // draft; switching back to manual restores the retained direct price.
    let nested_before = saved().nested[ORB].clone();
    let mut library = db::load_calculator_library().unwrap();
    let folder = library.folders[0].id;
    let entry = library
        .save_entry(
            folder,
            None,
            "Fire orb nested fixture",
            nested_before.clone(),
        )
        .unwrap();
    db::save_calculator_library(&library).unwrap();
    let option = format!("saved:{entry}");
    app.invoke_character_calculator_set_source(ORB.into(), "manual".into());
    assert!(saved().nested.is_empty());
    assert_eq!(row(app, ORB).price, "12,345");
    assert_eq!(amount(&app.get_character_calculator_total()), 1_111_050);
    assert_options_include(app, ORB, true, &option);
    app.invoke_character_calculator_set_source(ORB.into(), option.into());
    assert_eq!(saved().nested[ORB].materials, nested_before.materials);
    assert!(app.get_character_calculator_complete());
    library.remove_entry(entry).unwrap();
    db::save_calculator_library(&library).unwrap();
    app.invoke_character_calculator_refresh();
    assert_eq!(saved().nested[ORB].materials, nested_before.materials);

    // Invalid direct editing of a calculated parent cannot replace its source
    // or retained manual price. Reload clears this intentionally invalid call.
    let before_parent_edit = saved();
    assert!(!app.invoke_character_calculator_price_changed(ORB.into(), "999".into()));
    assert_eq!(saved(), before_parent_edit);
    super::super::install(app, catalog.clone(), Rc::new(RefCell::new(HashMap::new())));

    // Re-selecting a current route for the same hero keeps compatible subtrees.
    let mut historical = saved();
    historical.route_id = "historical-nested-route".into();
    db::save_character_calculator(&historical).unwrap();
    super::super::install(app, catalog.clone(), Rc::new(RefCell::new(HashMap::new())));
    assert_eq!(app.get_character_calculator_routes().row_count(), 2);
    app.invoke_character_calculator_select_route(1);
    assert_eq!(saved().route_id, "upgrade:T49");
    assert_eq!(saved().quantity, 3);
    assert_eq!(saved().nested, historical.nested);

    // Failed persistence leaves accepted inputs and both active recipes intact.
    let before_failure = saved();
    let saved_file = SavedEntryFile::corrupt("character-calculator.json");
    assert!(!app.invoke_character_calculator_price_changed(LEAF.into(), "500".into()));
    assert_eq!(row(app, LEAF).price, "10");
    assert!(row(app, LEAF).has_draft);
    drop(saved_file);
    assert_eq!(saved(), before_failure);
    price(app, LEAF, "10");
    let saved_file = SavedEntryFile::corrupt("character-calculator.json");
    app.invoke_character_calculator_set_source(ORB.into(), "manual".into());
    assert!(row(app, ORB).nested);
    drop(saved_file);
    assert_eq!(saved(), before_failure);
    app.invoke_character_calculator_refresh();

    if std::env::var_os("GS_WIDGET_CHARACTER_NESTED_REVIEW").is_some() {
        save_nested_review_images(app, window, catalog);
    }
    draw(app, window, 960, 640);

    // A different hero starts without the old subtree but keeps compatible
    // direct prices, while all item-calculator state remains independent.
    let before_target_change = saved();
    let (next_code, common_key) = catalog
        .character_calculator_browse("", "", "zh")
        .into_iter()
        .filter(|candidate| candidate.code != "V06")
        .find_map(|candidate| {
            let route = catalog.character_calculator_entry_route(&candidate.code, "zh")?;
            let key = route
                .inputs
                .iter()
                .find(|input| {
                    matches!(input.kind.as_str(), "item" | "hero")
                        && before_target_change
                            .inputs
                            .iter()
                            .any(|old| old.key == input.key)
                })?
                .key
                .clone();
            Some((candidate.code, key))
        })
        .unwrap();
    let previous_price = before_target_change
        .inputs
        .iter()
        .find(|input| input.key == common_key)
        .unwrap()
        .unit_price;
    app.invoke_character_calculator_select_character(next_code.clone().into());
    assert_eq!(saved().hero_code, next_code);
    assert!(saved().nested.is_empty());
    assert_eq!(
        saved()
            .inputs
            .iter()
            .find(|input| input.key == common_key)
            .unwrap()
            .unit_price,
        previous_price
    );
    assert_eq!(db::load_calculator().unwrap().unwrap(), item_before);
    assert_eq!(
        app.get_calculator_selected_name(),
        catalog
            .localized_calculator_document(&item_before, &app.get_lang())
            .title
    );
}

fn assert_options_blocked_by_draft(app: &App) {
    app.invoke_character_calculator_request_nested_options(ORB.into(), false);
    assert_eq!(app.get_character_calculator_nested_options().row_count(), 0);
}

fn save_nested_review_images(app: &App, window: &MinimalSoftwareWindow, catalog: &db::Catalog) {
    use slint::platform::{Key, PointerEventButton, WindowEvent};
    let directory = std::path::Path::new("/tmp/gs-widget-character-nested-review");
    std::fs::create_dir_all(directory).unwrap();
    let click = |x, y| {
        let position = slint::LogicalPosition::new(x, y);
        app.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        app.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        app.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
    };
    let escape = || {
        app.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        app.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Escape.into(),
        });
    };
    let save = |name: &str, width, height| {
        let pixels = draw(app, window, width, height);
        image::save_buffer_with_format(
            directory.join(format!("{name}.jpg")),
            pixels.as_bytes(),
            width,
            height,
            image::ColorType::Rgb8,
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    };
    app.invoke_character_calculator_search("".into());
    select_group(app, catalog, "9");
    for language in ["zh-en", "en-zh"] {
        set_language(app, language);
        for (width, height) in [(1280, 800), (960, 640)] {
            draw(app, window, width, height);
            // Actual pointer events open the production category and picker.
            let category_x = if width == 1280 { 280.0 } else { 250.0 };
            click(category_x, 215.0);
            save(
                &format!("categories-{language}-{width}x{height}"),
                width,
                height,
            );
            escape();
            click(if width == 1280 { 650.0 } else { 550.0 }, 215.0);
            save(
                &format!("characters-{language}-{width}x{height}"),
                width,
                height,
            );
            app.window().dispatch_event(WindowEvent::KeyPressed {
                text: Key::End.into(),
            });
            app.window().dispatch_event(WindowEvent::KeyReleased {
                text: Key::End.into(),
            });
            save(
                &format!("characters-end-{language}-{width}x{height}"),
                width,
                height,
            );
            escape();
            app.window().dispatch_event(WindowEvent::PointerScrolled {
                position: slint::LogicalPosition::new(600.0, 400.0),
                delta_x: 0.0,
                delta_y: -560.0,
            });
            save(
                &format!("nested-{language}-{width}x{height}"),
                width,
                height,
            );
            app.window().dispatch_event(WindowEvent::PointerScrolled {
                position: slint::LogicalPosition::new(600.0, 400.0),
                delta_x: 0.0,
                delta_y: -300.0,
            });
            save(
                &format!("nested-deep-{language}-{width}x{height}"),
                width,
                height,
            );
            app.window().dispatch_event(WindowEvent::PointerScrolled {
                position: slint::LogicalPosition::new(600.0, 400.0),
                delta_x: 0.0,
                delta_y: 2000.0,
            });
        }
    }
}
