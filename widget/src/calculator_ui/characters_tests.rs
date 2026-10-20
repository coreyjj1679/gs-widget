use super::*;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};

struct CalculatorTestPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for CalculatorTestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
}

fn set_language(app: &App, language: &str) {
    let primary = db::primary_lang(language);
    app.set_lang(language.into());
    app.set_primary_language(primary.into());
    app.global::<crate::UiLanguage>()
        .set_primary(primary.into());
    app.invoke_calculator_refresh();
}

fn character_rows(app: &App) -> Vec<CharacterCalcMaterial> {
    app.get_character_calculator_materials().iter().collect()
}

fn amount(value: &str) -> i64 {
    value.replace(',', "").parse().unwrap()
}

fn fill_character_prices(app: &App) -> i64 {
    character_rows(app)
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let price = (index as i64 + 1) * 1_000;
            assert!(app.invoke_character_calculator_price_changed(
                row.key.clone(),
                price.to_string().into()
            ));
            amount(&row.quantity) * price
        })
        .sum()
}

fn draw(
    app: &App,
    window: &MinimalSoftwareWindow,
    width: u32,
    height: u32,
) -> slint::SharedPixelBuffer<slint::Rgb8Pixel> {
    window.set_size(slint::LogicalSize::new(width as f32, height as f32));
    slint::platform::update_timers_and_animations();
    app.window().request_redraw();
    let mut pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(pixels.make_mut_slice(), width as usize);
    }));
    pixels
}

fn save_review_images(app: &App, window: &MinimalSoftwareWindow) {
    let directory = std::path::Path::new("/tmp/gs-widget-character-calculator-review");
    std::fs::create_dir_all(directory).unwrap();
    for language in ["zh-en", "en-zh"] {
        set_language(app, language);
        for (width, height) in [(1280, 800), (960, 640)] {
            for (tab, page) in [(0, "items"), (1, "characters")] {
                app.set_calculator_tab(tab);
                let pixels = draw(app, window, width, height);
                let path = directory.join(format!("{page}-{language}-{width}x{height}.jpg"));
                image::save_buffer_with_format(
                    &path,
                    pixels.as_bytes(),
                    width,
                    height,
                    image::ColorType::Rgb8,
                    image::ImageFormat::Jpeg,
                )
                .unwrap();
            }
        }
    }
}

fn entry_session(cache: IconCache) -> crate::Session {
    let (jobs, _receiver) = std::sync::mpsc::channel();
    crate::Session {
        cache,
        jobs,
        generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        link_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
    }
}

fn show_record_detail(
    app: &App,
    catalog: &db::Catalog,
    session: &crate::Session,
    key: &str,
    full: bool,
    preview: bool,
) {
    app.set_full_mode(full);
    app.set_collapsed(false);
    app.set_calculator_open(false);
    app.set_settings_open(false);
    app.set_mcp_open(false);
    let (category, code) = key.split_once(':').unwrap_or(("item", key));
    app.set_category(category.into());
    app.set_place("".into());
    app.set_area("".into());
    app.set_place_name("".into());
    app.set_area_name("".into());
    app.set_nation_name("".into());
    app.set_query("".into());
    app.set_list_offset(0.0);
    app.set_viewing(if preview { "" } else { key }.into());
    app.set_detail_key(if preview { key } else { "" }.into());
    app.set_detail_index(if preview { 0 } else { -1 });
    app.set_viewing_monster(false);
    if preview {
        app.set_catalog_sections(ModelRc::default());
        let records = catalog.search(category, code, &app.get_lang(), "", "");
        app.set_hits(ModelRc::new(VecModel::from(crate::list::plain_hits(
            records,
        ))));
    }
    crate::sheet::show_key(app, catalog, session, key, false);
}

struct SavedEntryFile {
    path: std::path::PathBuf,
    original: Vec<u8>,
}

impl SavedEntryFile {
    fn corrupt(name: &str) -> Self {
        // calculator_export_dir uses the same isolated thread-local preference
        // directory as both calculators; installed user state is never touched.
        let path = db::calculator_export_dir().parent().unwrap().join(name);
        let original = std::fs::read(&path).unwrap();
        std::fs::write(&path, b"not valid JSON").unwrap();
        Self { path, original }
    }
}

impl Drop for SavedEntryFile {
    fn drop(&mut self) {
        std::fs::write(&self.path, &self.original).unwrap();
    }
}

fn check_record_calculator_entries(app: &App, catalog: &Rc<db::Catalog>) -> String {
    let cache = Rc::new(RefCell::new(HashMap::new()));
    let session = entry_session(cache.clone());
    // Earlier assertions reinstall just the character controller. Reinstall the
    // complete integration so every callback shares the same retained drafts.
    super::super::install(app, catalog.clone(), cache);
    set_language(app, "zh-en");

    let item_id = app.get_calculator_candidates().row_data(0).unwrap().id;
    app.invoke_calculator_select_item(item_id.clone());
    assert!(app.invoke_calculator_quantity_changed("9".into()));
    let item_row_key = app.get_calculator_materials().row_data(0).unwrap().key;
    assert!(app.invoke_calculator_price_changed(item_row_key.clone(), "700".into()));
    assert!(!app.invoke_calculator_quantity_changed("item-entry-draft".into()));
    assert!(!app.invoke_calculator_price_changed(item_row_key.clone(), "7x".into()));
    let item_before = db::load_calculator().unwrap().unwrap();
    let item_revision = app.get_calculator_input_revision();
    let item_key = format!("item:{item_id}");

    app.invoke_character_calculator_select_character("T49".into());
    assert!(app.invoke_character_calculator_quantity_changed("7".into()));
    fill_character_prices(app);
    let hero_row_key = character_rows(app)[0].key.clone();
    assert!(!app.invoke_character_calculator_quantity_changed("hero-entry-draft".into()));
    assert!(!app.invoke_character_calculator_price_changed(hero_row_key.clone(), "9x".into()));
    let hero_before = db::load_character_calculator().unwrap().unwrap();
    let hero_revision = app.get_character_calculator_input_revision();

    let full_mode_requests = Rc::new(std::cell::Cell::new(0));
    let requests = full_mode_requests.clone();
    app.on_mode_changed(move |full| {
        assert!(full);
        requests.set(requests.get() + 1);
    });

    // Opening the same target from a compact detail only changes navigation.
    // Its accepted values and even invalid, partially typed fields survive.
    app.set_calculator_library_open(true);
    show_record_detail(app, catalog, &session, &item_key, false, false);
    assert!(app.get_sheet_can_calculate());
    app.invoke_calculate_record(item_key.clone().into());
    assert!(app.get_full_mode());
    assert!(app.get_calculator_open());
    assert!(!app.get_calculator_library_open());
    assert_eq!(app.get_calculator_tab(), 0);
    assert_eq!(full_mode_requests.get(), 1);
    assert_eq!(db::load_calculator().unwrap().unwrap(), item_before);
    assert_eq!(app.get_calculator_input_revision(), item_revision);
    assert_eq!(app.get_calculator_quantity(), "9");
    assert_eq!(app.get_calculator_quantity_draft(), "item-entry-draft");
    assert!(app.get_calculator_quantity_has_draft());
    let item_row = app
        .get_calculator_materials()
        .iter()
        .find(|row| row.key == item_row_key)
        .unwrap();
    assert!(item_row.has_draft);
    assert_eq!(item_row.draft, "7x");

    show_record_detail(app, catalog, &session, "hero:T49", true, true);
    assert!(app.get_sheet_can_calculate());
    app.invoke_calculate_record("hero:T49".into());
    assert!(app.get_calculator_open());
    assert_eq!(app.get_calculator_tab(), 1);
    assert_eq!(full_mode_requests.get(), 1);
    assert_eq!(
        db::load_character_calculator().unwrap().unwrap(),
        hero_before
    );
    assert_eq!(app.get_character_calculator_input_revision(), hero_revision);
    assert_eq!(app.get_character_calculator_quantity(), "7");
    assert_eq!(
        app.get_character_calculator_quantity_draft(),
        "hero-entry-draft"
    );
    assert!(app.get_character_calculator_quantity_has_draft());
    let hero_row = character_rows(app)
        .into_iter()
        .find(|row| row.key == hero_row_key)
        .unwrap();
    assert!(hero_row.has_draft);
    assert_eq!(hero_row.draft, "9x");

    // Basic recruitment is available in the calculator's general search, but
    // does not pretend to have a synthesis recipe on its detail page.
    let recruit = catalog.character_calculator_routes("U00", "zh-en");
    assert_eq!(recruit.len(), 1);
    assert_eq!(recruit[0].id, "recruitment");
    for key in [
        "hero:U00",
        "hero:missing",
        "item:1",
        "item:bad",
        "recipe:1",
        "",
    ] {
        show_record_detail(app, catalog, &session, key, false, false);
        assert!(
            !app.get_sheet_can_calculate(),
            "unexpected calculator entry for {key}"
        );
        let viewing = app.get_viewing();
        app.invoke_calculate_record(key.into());
        assert!(
            !app.get_calculator_open(),
            "invalid entry navigated for {key}"
        );
        assert!(!app.get_full_mode());
        assert_eq!(app.get_viewing(), viewing);
        assert_eq!(app.get_calculator_tab(), 1);
        assert_eq!(db::load_calculator().unwrap().unwrap(), item_before);
        assert_eq!(
            db::load_character_calculator().unwrap().unwrap(),
            hero_before
        );
    }
    assert_eq!(full_mode_requests.get(), 1);

    // A different target uses the normal selection rules: starts at one output,
    // clears the prior invalid drafts, and retains compatible material prices.
    show_record_detail(app, catalog, &session, "hero:T51", false, false);
    assert!(app.get_sheet_can_calculate());
    app.invoke_calculate_record("hero:T51".into());
    let next_hero = db::load_character_calculator().unwrap().unwrap();
    assert_eq!(next_hero.hero_code, "T51");
    assert_eq!(next_hero.quantity, 1);
    assert!(app.get_full_mode());
    assert!(app.get_calculator_open());
    assert_eq!(app.get_calculator_tab(), 1);
    assert_eq!(full_mode_requests.get(), 2);
    assert!(!app.get_character_calculator_quantity_has_draft());
    assert!(character_rows(app).iter().all(|row| !row.has_draft));
    assert!(app.get_character_calculator_input_revision() > hero_revision);
    for row in &next_hero.inputs {
        if row.kind == "item" {
            if let Some(prior) = hero_before.inputs.iter().find(|prior| prior.key == row.key) {
                assert_eq!(row.unit_price, prior.unit_price);
            }
        }
    }
    assert_eq!(db::load_calculator().unwrap().unwrap(), item_before);

    let other_item = app
        .get_calculator_candidates()
        .iter()
        .find(|candidate| candidate.id != item_id)
        .unwrap()
        .id;
    let other_key = format!("item:{other_item}");
    show_record_detail(app, catalog, &session, &other_key, true, true);
    assert!(app.get_sheet_can_calculate());
    app.invoke_calculate_record(other_key.into());
    let next_item = db::load_calculator().unwrap().unwrap();
    assert_eq!(next_item.item_code.to_string(), other_item.as_str());
    assert_eq!(next_item.quantity, 1);
    assert_eq!(app.get_calculator_tab(), 0);
    assert!(app.get_calculator_open());
    assert!(!app.get_calculator_quantity_has_draft());
    assert!(app
        .get_calculator_materials()
        .iter()
        .all(|row| !row.has_draft));
    assert!(app.get_calculator_input_revision() > item_revision);
    assert_eq!(db::load_character_calculator().unwrap().unwrap(), next_hero);
    assert_eq!(full_mode_requests.get(), 2);

    // A failed save must not open the calculator displaying the previous target
    // as if it were the requested one. Restore each fixture even on assertion
    // failure, and confirm a subsequent valid action can still succeed.
    for (key, file) in [
        (item_key.as_str(), "calculator.json"),
        ("hero:T49", "character-calculator.json"),
    ] {
        show_record_detail(app, catalog, &session, key, false, false);
        assert!(app.get_sheet_can_calculate());
        let previous_item_name = app.get_calculator_selected_name();
        let previous_hero_name = app.get_character_calculator_selected_name();
        let saved_file = SavedEntryFile::corrupt(file);
        app.invoke_calculate_record(key.into());
        assert!(!app.get_full_mode());
        assert!(!app.get_calculator_open());
        assert_eq!(app.get_viewing(), key);
        assert_eq!(app.get_calculator_selected_name(), previous_item_name);
        assert_eq!(
            app.get_character_calculator_selected_name(),
            previous_hero_name
        );
        assert_eq!(full_mode_requests.get(), 2);
        drop(saved_file);
    }
    assert_eq!(db::load_calculator().unwrap().unwrap(), next_item);
    assert_eq!(db::load_character_calculator().unwrap().unwrap(), next_hero);
    app.invoke_calculate_record("hero:T49".into());
    assert!(app.get_calculator_open());
    assert_eq!(app.get_calculator_tab(), 1);
    assert_eq!(
        db::load_character_calculator().unwrap().unwrap().hero_code,
        "T49"
    );
    item_key
}

fn check_record_calculator_keyboard(
    app: &App,
    window: &MinimalSoftwareWindow,
    catalog: &db::Catalog,
    item_key: &str,
) {
    use slint::platform::{Key, WindowEvent};
    let session = entry_session(Rc::new(RefCell::new(HashMap::new())));
    let press = |text: slint::SharedString| {
        app.window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        app.window()
            .dispatch_event(WindowEvent::KeyReleased { text });
    };

    // Exercise actual focus and key delivery, rather than invoking the button's
    // callback. The platform emits either Backtab or Shift + Tab; both must
    // reach the action without changing the existing plain-Tab navigation.
    for (key, full, preview, shifted_tab, space, tab) in [
        (item_key, true, false, false, false, 0),
        (item_key, false, false, true, true, 0),
        (item_key, true, true, false, false, 0),
        ("hero:T51", true, false, true, true, 1),
        ("hero:T51", false, false, false, false, 1),
        ("hero:T51", true, true, true, true, 1),
    ] {
        // Normal calculator navigation focuses the root keyboard scope. Keep
        // that focus while setting up the detail view, as real open-hit does.
        app.invoke_show_record_calculator(0);
        show_record_detail(app, catalog, &session, key, full, preview);
        assert!(app.get_sheet_can_calculate());
        let (width, height) = if full { (960, 640) } else { (420, 700) };
        draw(app, window, width, height);
        if shifted_tab {
            app.window().dispatch_event(WindowEvent::KeyPressed {
                text: Key::Shift.into(),
            });
            press(Key::Tab.into());
            app.window().dispatch_event(WindowEvent::KeyReleased {
                text: Key::Shift.into(),
            });
        } else {
            press(Key::Backtab.into());
        }
        assert!(!app.get_calculator_open());
        slint::platform::update_timers_and_animations();
        press(if space {
            " ".into()
        } else {
            Key::Return.into()
        });
        assert!(
            app.get_calculator_open(),
            "keyboard action did not open {key}, full={full}, preview={preview}"
        );
        assert!(app.get_full_mode());
        assert_eq!(app.get_calculator_tab(), tab);
        if tab == 0 {
            assert_eq!(
                db::load_calculator()
                    .unwrap()
                    .unwrap()
                    .item_code
                    .to_string(),
                key.strip_prefix("item:").unwrap()
            );
        } else {
            assert_eq!(
                db::load_character_calculator().unwrap().unwrap().hero_code,
                "T51"
            );
        }
    }
}

fn save_entry_review_images(
    app: &App,
    window: &MinimalSoftwareWindow,
    catalog: &db::Catalog,
    item_key: &str,
) {
    let directory = std::path::Path::new("/tmp/gs-widget-calculator-entry-review");
    std::fs::create_dir_all(directory).unwrap();
    let session = entry_session(Rc::new(RefCell::new(HashMap::new())));
    let save = |name: &str, width: u32, height: u32| {
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
    for (name, key, language, full, preview, width, height) in [
        (
            "item-full-zh-1280x800",
            item_key,
            "zh-en",
            true,
            false,
            1280,
            800,
        ),
        (
            "hero-full-en-960x640",
            "hero:T51",
            "en-zh",
            true,
            false,
            960,
            640,
        ),
        (
            "item-compact-en-420x700",
            item_key,
            "en-zh",
            false,
            false,
            420,
            700,
        ),
        (
            "hero-compact-zh-420x700",
            "hero:T51",
            "zh-en",
            false,
            false,
            420,
            700,
        ),
        (
            "item-preview-en-960x640",
            item_key,
            "en-zh",
            true,
            true,
            960,
            640,
        ),
        (
            "hero-preview-zh-1280x800",
            "hero:T51",
            "zh-en",
            true,
            true,
            1280,
            800,
        ),
    ] {
        set_language(app, language);
        show_record_detail(app, catalog, &session, key, full, preview);
        assert!(app.get_sheet_can_calculate());
        save(name, width, height);
    }
    for (name, key, language, tab) in [
        ("item-calculator-en-960x640", item_key, "en-zh", 0),
        ("hero-calculator-zh-960x640", "hero:T51", "zh-en", 1),
    ] {
        set_language(app, language);
        show_record_detail(app, catalog, &session, key, false, false);
        app.invoke_calculate_record(key.into());
        assert!(app.get_calculator_open());
        assert!(app.get_full_mode());
        assert_eq!(app.get_calculator_tab(), tab);
        save(name, 960, 640);
    }
}

#[test]
fn callbacks_keep_character_and_item_drafts_independent_across_tabs_routes_and_languages() {
    // This test uses the real bundled catalog and the existing thread-local
    // preferences directory. No installed user preferences or native app window
    // are touched, and calling the real callbacks catches RefCell reentrancy.
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(CalculatorTestPlatform {
        window: window.clone(),
    }))
    .unwrap();
    let app = App::new().unwrap();
    app.global::<crate::UiLanguage>()
        .on_translate(|language, en, zh| db::ui_text(&language, &zh, &en).into());
    #[cfg(target_os = "macos")]
    app.set_ui_font("PingFang TC".into());
    #[cfg(target_os = "windows")]
    app.set_ui_font("Microsoft JhengHei UI".into());
    app.set_full_mode(true);
    app.set_collapsed(false);
    app.set_calculator_open(true);
    let catalog = Rc::new(db::Catalog::open().unwrap());
    super::super::install(&app, catalog.clone(), Rc::new(RefCell::new(HashMap::new())));
    set_language(&app, "zh-en");
    app.show().unwrap();

    // Keep an actual item calculation and an invalid item draft while using the
    // new page. Both calculators have a quantity field, but own separate state.
    let item = app.get_calculator_candidates().row_data(0).unwrap();
    app.invoke_calculator_select_item(item.id);
    assert!(app.invoke_calculator_quantity_changed("3".into()));
    let item_row = app.get_calculator_materials().row_data(0).unwrap();
    assert!(app.invoke_calculator_price_changed(item_row.key.clone(), "250".into()));
    assert!(!app.invoke_calculator_quantity_changed("item-draft".into()));
    let item_name = app.get_calculator_selected_name();
    let item_total = app.get_calculator_total();
    let item_document = db::load_calculator().unwrap().unwrap();

    app.set_calculator_tab(1);
    app.invoke_character_calculator_search("持國天王".into());
    assert!(app
        .get_character_calculator_candidates()
        .iter()
        .any(|candidate| candidate.id == "T49"));
    app.invoke_character_calculator_select_character("T49".into());
    let chinese_name = app.get_character_calculator_selected_name();
    assert!(!chinese_name.is_empty());
    assert!(app.get_character_calculator_error().is_empty());
    assert!(character_rows(&app).len() >= 3);
    assert!(!app.get_character_calculator_complete());
    assert_eq!(app.get_character_calculator_unit_total(), "—");
    let expected_total = fill_character_prices(&app);
    assert!(app.get_character_calculator_complete());
    assert_eq!(
        amount(&app.get_character_calculator_total()),
        expected_total
    );
    assert!(app.invoke_character_calculator_quantity_changed("2".into()));
    assert_eq!(
        amount(&app.get_character_calculator_total()),
        expected_total * 2
    );
    assert_eq!(
        amount(&app.get_character_calculator_unit_total()),
        expected_total
    );

    // Accepting an edit must clear only that field's invalid draft. Empty
    // quantity is itself an invalid draft, distinct from no pending draft.
    let rows = character_rows(&app);
    let first_key = rows[0].key.clone();
    let second_key = rows[1].key.clone();
    assert!(!app.invoke_character_calculator_quantity_changed("".into()));
    assert!(!app.invoke_character_calculator_price_changed(first_key.clone(), "12x".into()));
    assert!(app.invoke_character_calculator_price_changed(second_key.clone(), "4,500".into()));
    assert!(app.get_character_calculator_quantity_has_draft());
    assert_eq!(app.get_character_calculator_quantity_draft(), "");
    assert_eq!(app.get_character_calculator_quantity(), "2");
    let first_row = character_rows(&app)
        .into_iter()
        .find(|row| row.key == first_key)
        .unwrap();
    assert!(first_row.has_draft);
    assert_eq!(first_row.draft, "12x");
    assert!(!app.get_character_calculator_error().is_empty());
    let persisted = db::load_character_calculator().unwrap().unwrap();
    assert_eq!(persisted.quantity, 2);
    assert_eq!(
        persisted
            .inputs
            .iter()
            .find(|input| input.key == first_key.as_str())
            .unwrap()
            .unit_price,
        Some(1_000)
    );
    draw(&app, &window, 960, 640);

    for tab in [0, 1, 0, 1] {
        app.set_calculator_tab(tab);
        app.invoke_calculator_refresh();
        assert_eq!(app.get_calculator_selected_name(), item_name);
        assert_eq!(app.get_calculator_total(), item_total);
        assert_eq!(app.get_calculator_quantity(), "3");
        assert!(app.get_calculator_quantity_has_draft());
        assert_eq!(app.get_calculator_quantity_draft(), "item-draft");
        assert!(app.get_character_calculator_quantity_has_draft());
        assert_eq!(app.get_character_calculator_quantity_draft(), "");
        let row = character_rows(&app)
            .into_iter()
            .find(|row| row.key == first_key)
            .unwrap();
        assert_eq!(row.draft, "12x");
        assert!(row.has_draft);
    }
    assert_eq!(db::load_calculator().unwrap().unwrap(), item_document);

    assert!(app.invoke_character_calculator_quantity_changed("3".into()));
    assert!(!app.get_character_calculator_quantity_has_draft());
    assert!(!app.get_character_calculator_error().is_empty());
    assert!(app.invoke_character_calculator_price_changed(first_key, "5,000".into()));
    assert!(app.get_character_calculator_error().is_empty());
    assert!(character_rows(&app).iter().all(|row| !row.has_draft));
    assert!(app.get_calculator_quantity_has_draft());

    // Language refresh relabels live data without discarding estimates or
    // interpreting a partially typed value as an accepted price.
    let before_language = db::load_character_calculator().unwrap().unwrap();
    assert!(!app.invoke_character_calculator_quantity_changed("not-a-number".into()));
    for language in ["en-zh", "ko-zh", "zh-en"] {
        set_language(&app, language);
        let expected = catalog.localized_character_calculator_document(&before_language, language);
        assert_eq!(app.get_character_calculator_selected_name(), expected.title);
        assert_eq!(app.get_character_calculator_quantity(), "3");
        assert_eq!(
            app.get_character_calculator_quantity_draft(),
            "not-a-number"
        );
        assert!(app.get_character_calculator_quantity_has_draft());
        assert_eq!(
            db::load_character_calculator().unwrap().unwrap(),
            before_language
        );
        if language == "en-zh" {
            assert_ne!(app.get_character_calculator_selected_name(), chinese_name);
        }
    }

    // Changing routes resets drafts and keeps the accepted output quantity.
    // The bundled catalog currently has one route per character. A previously
    // saved route can still differ from today's catalog: retain the actual
    // material snapshot and change only its route ID to exercise that case.
    let revision_before_select = app.get_character_calculator_input_revision();
    app.invoke_character_calculator_select_character("T49".into());
    assert!(app.get_character_calculator_input_revision() > revision_before_select);
    assert!(!app.get_character_calculator_quantity_has_draft());
    assert!(app.invoke_character_calculator_quantity_changed("4".into()));
    let next_route = catalog
        .character_calculator_routes("T49", "zh-en")
        .into_iter()
        .next()
        .unwrap();
    let mut historical = db::load_character_calculator().unwrap().unwrap();
    historical.route_id = "historical-route".into();
    db::save_character_calculator(&historical).unwrap();
    super::install(&app, catalog.clone(), Rc::new(RefCell::new(HashMap::new())));
    assert_eq!(app.get_character_calculator_routes().row_count(), 2);
    assert_eq!(
        db::load_character_calculator().unwrap().unwrap().route_id,
        "historical-route"
    );
    assert!(!app.invoke_character_calculator_quantity_changed("0".into()));
    let route_input = character_rows(&app).remove(0);
    assert!(!app.invoke_character_calculator_price_changed(route_input.key, "1,,000".into()));
    let revision_before_route = app.get_character_calculator_input_revision();
    app.invoke_character_calculator_select_route(1);
    assert_eq!(
        db::load_character_calculator().unwrap().unwrap().route_id,
        next_route.id
    );
    assert_eq!(app.get_character_calculator_route_index(), 0);
    assert_eq!(app.get_character_calculator_quantity(), "4");
    assert!(app.get_character_calculator_input_revision() > revision_before_route);
    assert!(!app.get_character_calculator_quantity_has_draft());
    assert!(character_rows(&app).iter().all(|row| !row.has_draft));
    assert!(app.get_character_calculator_error().is_empty());
    assert_eq!(app.get_calculator_quantity_draft(), "item-draft");

    // Reopen an older, fully priced T51 draft that omitted its tablet. The
    // repaired row must be persisted and shown as unknown, never as free.
    app.invoke_character_calculator_select_character("T51".into());
    fill_character_prices(&app);
    assert!(app.invoke_character_calculator_quantity_changed("3".into()));
    let mut incomplete = db::load_character_calculator().unwrap().unwrap();
    incomplete.inputs.retain(|row| row.key != "item:5748");
    let old_total = incomplete.calculate().unwrap().total.unwrap();
    db::save_character_calculator(&incomplete).unwrap();
    super::install(&app, catalog.clone(), Rc::new(RefCell::new(HashMap::new())));
    assert_eq!(app.get_character_calculator_quantity(), "3");
    assert!(app.get_character_calculator_error().is_empty());
    assert!(!app.get_character_calculator_complete());
    assert_eq!(app.get_character_calculator_unit_total(), "—");
    assert_eq!(amount(&app.get_character_calculator_total()), old_total);
    let tablet = character_rows(&app)
        .into_iter()
        .find(|row| row.key == "item:5748")
        .unwrap();
    assert_eq!(amount(&tablet.quantity), 60);
    assert!(tablet.price.is_empty());
    assert!(tablet.missing_price);
    let repaired = db::load_character_calculator().unwrap().unwrap();
    assert_eq!(repaired.inputs.len(), incomplete.inputs.len() + 1);
    for previous in &incomplete.inputs {
        assert!(repaired.inputs.contains(previous));
    }
    assert!(app.invoke_character_calculator_price_changed("item:5748".into(), "100".into()));
    assert!(app.get_character_calculator_complete());
    assert_eq!(
        amount(&app.get_character_calculator_total()),
        old_total + 6_000
    );
    assert_eq!(db::load_calculator().unwrap().unwrap(), item_document);

    // Render real controls in the narrow supported window even in normal test
    // runs. Opt-in captures provide review evidence without regular test output.
    app.invoke_character_calculator_select_character("T49".into());
    fill_character_prices(&app);
    assert!(app.invoke_calculator_quantity_changed("3".into()));
    draw(&app, &window, 960, 640);
    if std::env::var_os("GS_WIDGET_CHARACTER_CALCULATOR_REVIEW").is_some() {
        save_review_images(&app, &window);
    }
    let item_key = check_record_calculator_entries(&app, &catalog);
    check_record_calculator_keyboard(&app, &window, &catalog, &item_key);
    if std::env::var_os("GS_WIDGET_CALCULATOR_ENTRY_REVIEW").is_some() {
        save_entry_review_images(&app, &window, &catalog, &item_key);
    }
    app.hide().unwrap();
}
