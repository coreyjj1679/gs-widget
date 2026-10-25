use super::*;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};

struct TestPlatform(Rc<MinimalSoftwareWindow>);
impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn capture(app: &App, window: &MinimalSoftwareWindow, name: &str) {
    let Ok(directory) = std::env::var("GS_BONUS_REVIEW") else {
        return;
    };
    std::fs::create_dir_all(&directory).unwrap();
    let sizes: &[(u32, u32)] = match name {
        "zh" => &[(1280, 800), (960, 640), (1585, 900)],
        "en" | "ko" | "india" | "minimum-zh" | "minimum-en" | "minimum-ko" | "source-zh"
        | "source-en" | "source-ko" => &[(1280, 800), (960, 640)],
        "error" => &[(960, 640)],
        "razia-perfect" => &[(1585, 900)],
        _ => &[(1280, 800)],
    };
    for &(width, height) in sizes {
        window.set_size(slint::LogicalSize::new(width as f32, height as f32));
        slint::platform::update_timers_and_animations();
        app.window().request_redraw();
        let mut pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(pixels.make_mut_slice(), width as usize);
        }));
        image::save_buffer_with_format(
            std::path::Path::new(&directory).join(format!("bonus-{name}-{width}x{height}.jpg")),
            pixels.as_bytes(),
            width,
            height,
            image::ColorType::Rgb8,
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    }
}

fn optimize_and_wait(app: &App) {
    app.invoke_bonus_optimize();
    let started = std::time::Instant::now();
    while app.get_bonus_optimizing() {
        assert!(
            started.elapsed() < Duration::from_secs(45),
            "planner did not finish"
        );
        std::thread::sleep(Duration::from_millis(10));
        slint::platform::update_timers_and_animations();
    }
}

#[test]
fn bonus_controller_keeps_drafts_validates_and_relocalizes() {
    // Each capture allocates fresh pixels, so every frame must paint the full buffer.
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let app = App::new().unwrap();
    app.global::<crate::UiLanguage>()
        .on_translate(|language, en, zh| db::ui_text(&language, &zh, &en).into());
    app.set_full_mode(true);
    app.set_collapsed(false);
    app.set_calculator_open(true);
    app.set_calculator_tab(2);
    app.set_lang("zh-en".into());
    app.global::<crate::UiLanguage>().set_primary("zh".into());
    app.set_ui_font("PingFang TC".into());
    install(
        &app,
        Rc::new(db::Catalog::open().unwrap()),
        Rc::new(RefCell::new(HashMap::new())),
    );
    assert!(app.get_bonus_candidates().row_count() >= 15);
    app.invoke_bonus_select_character("T80".into());
    assert_eq!(app.get_bonus_error(), "");
    let catalog = db::Catalog::open().unwrap();
    assert_eq!(app.get_bonus_selected_alternate_name().as_str(), catalog.record_alternate_name("hero:T80", "zh-en"));
    assert!(app.get_bonus_candidates().iter().any(|row| !row.alternate_name.is_empty()));
    let full_bonus: i64 = app.get_bonus_total().parse().unwrap();
    // Independent all-250 Jumong fixture: 719, or 1019 with four 300-point white robes.
    assert_eq!(full_bonus, 719);
    assert_eq!(app.get_bonus_maximum(), "1019");
    assert!(!app.get_bonus_perfect());
    assert!(app.get_bonus_has_white());
    assert!(app.get_bonus_source_text().contains("ExpForLevelUp.dat"));
    assert_eq!(app.get_bonus_tree_columns(), 4);
    let rows = app.get_bonus_rows();
    assert_eq!(app.get_bonus_edges().row_count(), rows.row_count() - 1);
    for row in rows.iter().filter(|row| row.depth > 0) {
        assert!(row.x >= 0.5 && row.x <= 3.5);
        assert_eq!(row.y, row.depth as f32 - 1.0);
    }
    app.show().unwrap();
    capture(&app, &window, "zh");

    let row = app
        .get_bonus_rows()
        .iter()
        .find(|row| row.editable)
        .unwrap();
    let model = app.get_bonus_rows();
    app.invoke_bonus_input_changed(row.key.clone(), "level".into(), "invalid".into());
    assert!(!app.get_bonus_error().is_empty());
    assert!(app.get_bonus_total().is_empty());
    assert_eq!(
        model.iter().find(|r| r.key == row.key).unwrap().level,
        "invalid"
    );
    capture(&app, &window, "error");

    app.invoke_bonus_input_changed(row.key.clone(), "level".into(), "250".into());
    assert!(app.get_bonus_error().is_empty());
    assert_eq!(app.get_bonus_total().parse::<i64>().unwrap(), full_bonus);
    app.invoke_bonus_white_preset();
    assert!(app
        .get_bonus_rows()
        .iter()
        .filter(|row| row.white_enabled)
        .all(|row| row.white == "300"));
    assert_eq!(
        app.get_bonus_total().parse::<i64>().unwrap(),
        full_bonus + 300
    );
    assert!(
        app.get_bonus_perfect(),
        "Equality after all stage floors is perfect"
    );
    let material = app
        .get_bonus_rows()
        .iter()
        .find(|row| row.inherited_editable)
        .unwrap();
    app.invoke_bonus_input_changed(material.key.clone(), "inherited".into(), material.bonus);
    assert_eq!(
        app.get_bonus_total().parse::<i64>().unwrap(),
        full_bonus + 300
    );
    assert!(
        !app.get_bonus_perfect(),
        "Manual history cannot certify a perfect training path"
    );
    app.invoke_bonus_input_changed(material.key, "inherited".into(), "".into());
    assert!(app.get_bonus_perfect());
    app.invoke_bonus_preset(200);
    assert!(app.get_bonus_total().parse::<i64>().unwrap() < full_bonus + 300);
    let expected = app.get_bonus_total();
    for lang in ["en-zh", "ko-en", "zh", "en", "ko"] {
        app.set_lang(lang.into());
        app.global::<crate::UiLanguage>().set_primary(db::primary_lang(lang).into());
        app.invoke_bonus_refresh();
        assert_eq!(app.get_bonus_total(), expected);
        assert_eq!(app.get_bonus_selected_alternate_name().as_str(), catalog.record_alternate_name("hero:T80", lang));
        let tree = catalog.bonus_tree("T80", lang).unwrap();
        for row in app.get_bonus_rows().iter() {
            let node = find_node(&tree, &row.key).unwrap();
            if !node.code.starts_with("statue:") {
                assert_eq!(row.alternate_name.as_str(), catalog.record_alternate_name(&format!("hero:{}", node.code), lang));
            }
        }
        capture(&app, &window, lang);
    }
    app.invoke_bonus_preset(0);
    assert_eq!(app.get_bonus_total().parse::<i64>().unwrap(), full_bonus);
    app.invoke_bonus_select_character("T49".into());
    assert_ne!(app.get_bonus_selected_name(), "");
    app.invoke_bonus_preset(250);
    let statue = app
        .get_bonus_rows()
        .iter()
        .find(|row| row.key.contains("/statue:"))
        .unwrap();
    assert_eq!(statue.level, "200", "statue levels have their own 200 cap");
    assert!(!statue.inherited_editable);
    app.invoke_bonus_input_changed(statue.key, "inherited".into(), "invalid".into());
    assert!(app.get_bonus_error().is_empty());
    let old = app.get_bonus_total();
    app.invoke_bonus_input_changed(row.key, "level".into(), "-1".into());
    assert_eq!(app.get_bonus_total(), old); // callback from old tree cannot edit current target.
    app.invoke_bonus_select_character("D117".into());
    assert_eq!(app.get_bonus_total(), "250");
    let source = app
        .get_bonus_rows()
        .iter()
        .find(|row| row.source_options.row_count() == 4)
        .unwrap();
    assert_eq!(source.min_level, 100);
    assert_eq!(source.max_level, 250);
    assert_eq!(
        app.get_bonus_rows().row_count(),
        2,
        "Only the chosen alternative is an ingredient"
    );
    let original_code = source.source_codes.row_data(0).unwrap();
    let alternate_code = source.source_codes.row_data(1).unwrap();
    app.invoke_bonus_input_changed(source.key.clone(), "level".into(), "174".into());
    assert_eq!(app.get_bonus_total(), "174");
    app.invoke_bonus_input_changed(
        source.source_owner.clone(),
        "predecessor".into(),
        alternate_code,
    );
    assert_eq!(app.get_bonus_total(), "250");
    let alternate = app
        .get_bonus_rows()
        .iter()
        .find(|row| row.depth == 1)
        .unwrap();
    assert_ne!(source.key, alternate.key);
    app.invoke_bonus_input_changed(alternate.key, "level".into(), "119".into());
    assert_eq!(app.get_bonus_total(), "119");
    app.invoke_bonus_input_changed(
        source.source_owner.clone(),
        "predecessor".into(),
        original_code,
    );
    assert_eq!(
        app.get_bonus_total(),
        "174",
        "Each candidate retains its own level draft"
    );
    app.invoke_bonus_select_node(source.key.clone());
    assert_eq!(app.get_bonus_selected_row().key, source.key);
    app.invoke_bonus_input_changed(source.key, "level".into(), "99".into());
    assert!(!app.get_bonus_error().is_empty());
    app.invoke_bonus_preset(260);
    assert_eq!(
        app.get_bonus_total(),
        "250",
        "Max levels respects ordinary mercenary caps"
    );
    for lang in ["zh", "en", "ko"] {
        app.set_lang(lang.into());
        app.global::<crate::UiLanguage>().set_primary(lang.into());
        app.invoke_bonus_refresh();
        capture(&app, &window, &format!("source-{lang}"));
    }
    app.set_lang("zh-en".into());
    app.global::<crate::UiLanguage>().set_primary("zh".into());
    app.invoke_bonus_refresh();
    app.invoke_bonus_select_character("T112".into());
    assert!(app.get_bonus_error().is_empty());
    assert_eq!(app.get_bonus_tree_columns(), 4);
    assert_eq!(
        app.get_bonus_rows()
            .iter()
            .filter(|row| row.source_options.row_count() == 4)
            .count(),
        4
    );
    capture(&app, &window, "india");
    app.invoke_bonus_select_character("V29".into());
    app.invoke_bonus_preset(260);
    assert_eq!(app.get_bonus_total(), "976");
    assert_eq!(app.get_bonus_maximum(), "976");
    assert!(app.get_bonus_perfect());
    assert!(app.get_bonus_verification().contains("976"));
    let king = app
        .get_bonus_rows()
        .iter()
        .find(|row| row.depth == 1)
        .unwrap();
    assert_eq!(king.max_level, 260);
    assert_eq!(king.level, "260");
    app.invoke_bonus_input_changed(king.key.clone(), "level".into(), "261".into());
    assert!(!app.get_bonus_error().is_empty());
    assert!(!app.get_bonus_perfect());
    app.invoke_bonus_input_changed(king.key, "level".into(), "260".into());
    capture(&app, &window, "wisdom-perfect");
    app.invoke_bonus_select_character("V16".into());
    for row in app
        .get_bonus_rows()
        .iter()
        .filter(|row| row.white_enabled)
        .collect::<Vec<_>>()
    {
        assert!(row.indian_white_history);
        app.invoke_bonus_input_changed(row.key.clone(), "white".into(), "300".into());
        assert!(
            !app.get_bonus_error().is_empty(),
            "White chronology is an explicit choice"
        );
        app.invoke_bonus_input_changed(row.key, "white-before-india".into(), "true".into());
        assert!(app.get_bonus_error().is_empty());
    }
    assert_eq!(app.get_bonus_total(), "1080");
    assert_eq!(app.get_bonus_maximum(), "1080");
    assert!(app.get_bonus_perfect());
    assert!(app.get_bonus_verification().contains("1080"));
    capture(&app, &window, "razia-perfect");
    app.invoke_bonus_preset(0);
    app.invoke_bonus_white_preset();
    assert_eq!(app.get_bonus_total(), "1080");
    assert!(app
        .get_bonus_rows()
        .iter()
        .filter(|row| row.white_enabled)
        .all(|row| row.white_before_india));
    assert!(app.get_bonus_plan_status().contains("印度"));
    optimize_and_wait(&app);
    assert_eq!(app.get_bonus_error(), "");
    assert_eq!(app.get_bonus_total(), "1080");
    assert!(app.get_bonus_perfect());
    assert!(app.get_bonus_plan_status().contains("XP"));
    assert!(app
        .get_bonus_rows()
        .iter()
        .any(|row| row.source_owner != "" && row.level != "250"));
    capture(&app, &window, "minimum-zh");
    for lang in ["en", "ko"] {
        app.set_lang(lang.into());
        app.global::<crate::UiLanguage>().set_primary(lang.into());
        app.invoke_bonus_refresh();
        optimize_and_wait(&app);
        assert_eq!(app.get_bonus_total(), "1080");
        assert!(app.get_bonus_perfect());
        capture(&app, &window, &format!("minimum-{lang}"));
    }
    app.invoke_bonus_select_character("V27".into());
    assert!(!app.get_bonus_has_white());
    optimize_and_wait(&app);
    assert_eq!(app.get_bonus_total(), "955");
    assert!(app.get_bonus_perfect());
    let lowered = app
        .get_bonus_rows()
        .iter()
        .find(|row| row.editable && row.level != row.max_level.to_string())
        .unwrap();
    app.invoke_bonus_input_changed(lowered.key, "level".into(), "250".into());
    assert!(
        app.get_bonus_plan_status().is_empty(),
        "Editing invalidates the applied-plan label"
    );
    app.hide().unwrap();
}
