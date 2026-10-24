use super::*;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Key, Platform, WindowAdapter, WindowEvent};
use std::cell::Cell;

struct TestPlatform(Rc<MinimalSoftwareWindow>);

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
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

fn press(app: &App, text: slint::SharedString) {
    app.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    app.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
    slint::platform::update_timers_and_animations();
}

fn set_language(app: &App, language: &str) {
    app.set_lang(language.into());
    app.set_primary_language(language.into());
    app.global::<crate::UiLanguage>()
        .set_primary(language.into());
}

fn review_captures(app: &App, window: &MinimalSoftwareWindow, state: &str) {
    app.set_update_prompt_open(true);
    let Ok(directory) = std::env::var("GS_UPDATE_REVIEW") else {
        return;
    };
    std::fs::create_dir_all(&directory).unwrap();
    for language in ["zh", "en", "ko"] {
        set_language(app, language);
        if state == "restart-blocked" {
            app.set_update_error(alias_error(language).into());
        }
        for (full, width, height) in [(true, 960, 640), (false, 420, 600), (false, 280, 360)] {
            app.set_full_mode(full);
            let pixels = draw(app, window, width, height);
            image::save_buffer_with_format(
                std::path::Path::new(&directory)
                    .join(format!("update-{state}-{language}-{width}x{height}.jpg")),
                pixels.as_bytes(),
                width,
                height,
                image::ColorType::Rgb8,
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        }
    }
    set_language(app, "en");
    if state == "restart-blocked" {
        app.set_update_error(alias_error("en").into());
    }
    app.set_full_mode(true);
    draw(app, window, 960, 640);
}

fn alias_error(language: &str) -> String {
    crate::db::ui_text(
        language,
        "別名有未儲存的修改。請先到「別名」儲存或取消，再重新啟動更新。",
        "You have unsaved aliases. Save or cancel them in the Aliases tab before restarting.",
    )
}

#[test]
fn launch_prompt_preserves_manual_checks_defers_in_dock_and_owns_keyboard_focus() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let app = App::new().unwrap();
    app.global::<crate::UiLanguage>()
        .on_translate(|language, en, zh| crate::db::ui_text(&language, &zh, &en).into());
    app.set_app_version(env!("CARGO_PKG_VERSION").into());
    // Test presentation on every development platform without invoking the
    // Windows-only network, download, or installer paths.
    app.set_updates_supported(true);
    app.set_full_mode(true);
    app.set_ui_font("PingFang TC".into());
    set_language(&app, "en");
    app.show().unwrap();
    draw(&app, &window, 960, 640);
    let session = RefCell::new(Session::default());

    for background in [true, false] {
        app.set_update_state(CHECKING);
        apply_checked(&app, &session, Ok(None), background);
        assert_eq!(app.get_update_state(), CURRENT);
        assert!(!app.get_update_prompt_open());
        assert!(!app.get_update_prompt_visible());
    }

    app.set_update_state(CHECKING);
    apply_checked(&app, &session, Err("offline fixture".into()), true);
    assert_eq!(app.get_update_state(), IDLE);
    assert!(app.get_update_error().is_empty());
    assert!(!app.get_update_prompt_open());

    app.set_update_state(CHECKING);
    apply_checked(&app, &session, Err("manual fixture".into()), false);
    assert_eq!(app.get_update_state(), ERROR);
    assert!(app.get_update_error().contains("manual fixture"));
    assert!(!app.get_update_prompt_open());

    app.set_update_error("".into());
    app.set_update_state(CHECKING);
    apply_checked(
        &app,
        &session,
        Ok(Some(update::release_for_test("9.8.7"))),
        false,
    );
    assert_eq!(app.get_update_state(), AVAILABLE);
    assert_eq!(app.get_update_version(), "9.8.7");
    assert!(!app.get_update_prompt_open());
    assert_eq!(session.borrow().release.as_ref().unwrap().version, "9.8.7");

    // The same checked release prompts on launch, while a manual check stays
    // in the user's existing update surface.
    app.set_update_state(CHECKING);
    apply_checked(
        &app,
        &session,
        Ok(Some(update::release_for_test("9.8.7"))),
        true,
    );
    assert_eq!(app.get_update_state(), AVAILABLE);
    assert!(app.get_update_prompt_open());
    assert!(app.get_update_prompt_visible());
    review_captures(&app, &window, "available");

    app.invoke_dismiss_update_prompt();
    assert!(!app.get_update_prompt_open());
    assert_eq!(app.get_update_state(), AVAILABLE);
    assert_eq!(session.borrow().release.as_ref().unwrap().version, "9.8.7");
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
    }
    assert!(
        !app.get_update_prompt_open(),
        "dismissal must survive later UI ticks"
    );

    app.set_collapsed(true);
    apply_checked(
        &app,
        &session,
        Ok(Some(update::release_for_test("9.8.7"))),
        true,
    );
    assert!(app.get_update_prompt_open());
    assert!(!app.get_update_prompt_visible());
    assert!(
        app.get_collapsed(),
        "discovery must not expand the user's dock"
    );
    app.set_collapsed(false);
    draw(&app, &window, 960, 640);
    assert!(app.get_update_prompt_visible());

    let requests = Rc::new(Cell::new(0));
    let requested = requests.clone();
    let weak = app.as_weak();
    app.on_request_update(move || {
        requested.set(requested.get() + 1);
        weak.upgrade().unwrap().set_update_state(DOWNLOADING);
    });

    // Dispatch actual keys through Slint focus, rather than invoking action
    // callbacks. Escape must leave the underlying settings page untouched.
    app.set_settings_open(true);
    app.set_settings_tab(4);
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    press(&app, "k".into());
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
    assert!(app.get_update_prompt_open());
    assert!(
        !app.get_quick_search_open(),
        "Ctrl+K must not open quick search beneath the prompt"
    );
    press(&app, Key::Escape.into());
    assert!(!app.get_update_prompt_open());
    assert!(app.get_settings_open());
    assert_eq!(requests.get(), 0);

    app.set_update_prompt_open(true);
    draw(&app, &window, 960, 640);
    press(&app, Key::Return.into());
    assert!(
        !app.get_update_prompt_open(),
        "Later should receive initial focus"
    );
    assert_eq!(requests.get(), 0, "initial Enter must not start a download");

    app.set_update_prompt_open(true);
    draw(&app, &window, 960, 640);
    press(&app, Key::Tab.into());
    press(&app, Key::Return.into());
    assert_eq!(requests.get(), 1);
    assert_eq!(app.get_update_state(), DOWNLOADING);
    assert!(
        app.get_update_prompt_open(),
        "the prompt should show download progress"
    );
    press(&app, Key::Return.into());
    assert!(
        !app.get_update_prompt_open(),
        "the primary action should return focus to Keep browsing"
    );
    assert_eq!(
        app.get_update_state(),
        DOWNLOADING,
        "Keep browsing must not cancel an active download"
    );
    assert_eq!(
        requests.get(),
        1,
        "busy actions must not be submitted twice"
    );
    app.set_update_progress(42);
    review_captures(&app, &window, "downloading");

    app.set_update_state(READY);
    app.set_update_progress(100);
    review_captures(&app, &window, "ready");
    app.set_update_error(alias_error("en").into());
    review_captures(&app, &window, "restart-blocked");
    app.set_update_error("".into());
    app.set_update_state(AVAILABLE);
    app.invoke_dismiss_update_prompt();
    // Reopening is a separate UI turn in the app. Let Slint remove the closed
    // conditional component before testing a newly opened prompt's focus.
    draw(&app, &window, 960, 640);

    app.set_update_prompt_open(true);
    draw(&app, &window, 960, 640);
    press(&app, Key::Tab.into());
    press(&app, Key::Tab.into());
    press(&app, Key::Return.into());
    assert!(
        !app.get_update_prompt_open(),
        "Tab should cycle back to Later"
    );
    assert_eq!(
        requests.get(),
        1,
        "Tab must not escape to the settings action"
    );

    app.set_update_prompt_open(true);
    draw(&app, &window, 960, 640);
    press(&app, Key::Backtab.into());
    press(&app, Key::Backtab.into());
    press(&app, Key::Return.into());
    assert!(
        !app.get_update_prompt_open(),
        "backward Tab navigation should also wrap to Later"
    );
    assert_eq!(
        requests.get(),
        1,
        "backward Tab must remain within the prompt"
    );
    assert_eq!(session.borrow().release.as_ref().unwrap().version, "9.8.7");
}
