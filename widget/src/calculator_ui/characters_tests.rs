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
    assert_eq!(amount(&app.get_character_calculator_total()), old_total + 6_000);
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
    app.hide().unwrap();
}
