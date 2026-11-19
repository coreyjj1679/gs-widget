use super::*;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use std::time::Instant;

struct Display(Rc<MinimalSoftwareWindow>);

impl Platform for Display {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn back(app: &App) {
    for event in [
        slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Escape.into(),
        },
        slint::platform::WindowEvent::KeyReleased {
            text: slint::platform::Key::Escape.into(),
        },
    ] {
        app.window().dispatch_event(event);
    }
}

#[test]
fn browse_real_archive_in_one_window() {
    let display = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Display(display.clone()))).unwrap();
    let app = App::new().unwrap();
    app.set_full_mode(true);
    app.set_collapsed(false);
    app.set_lang("zh-en".into());
    app.global::<crate::UiLanguage>().set_primary("zh".into());
    app.global::<crate::UiLanguage>()
        .on_translate(|lang, en, zh| db::ui_text(&lang, &zh, &en).into());
    #[cfg(target_os = "macos")]
    app.set_ui_font("PingFang TC".into());
    let catalog = Rc::new(db::Catalog::open().unwrap());
    let (jobs, _jobs_rx) = std::sync::mpsc::channel();
    let session = crate::Session {
        jobs,
        generation: Default::default(),
        link_generation: Default::default(),
        cache: Default::default(),
    };
    install(&app, catalog.clone(), session.cache.clone());
    app.show().unwrap();
    let width = 960;
    let height = 640;
    display.set_size(slint::LogicalSize::new(width as f32, height as f32));
    let mut pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
    let mut frame = || {
        slint::platform::update_timers_and_animations();
        app.window().request_redraw();
        assert!(display.draw_if_needed(|r| {
            r.render(pixels.make_mut_slice(), width as usize);
        }));
    };
    frame();
    let mut sample = |label: &str, change: &dyn Fn()| {
        let start = Instant::now();
        change();
        let prepared = start.elapsed();
        frame();
        eprintln!(
            "data changes {label}: model {prepared:.2?}, with 960x640 software frame {:.2?}",
            start.elapsed()
        );
    };
    sample("first open", &|| {
        app.set_category("patch".into());
        app.set_game_update_open(true);
        app.invoke_refresh_game_update();
    });
    let archive = bundled_history().unwrap();
    let latest = archive.versions.len() as i32 - 1;
    assert_eq!(
        (app.get_game_update_from(), app.get_game_update_to()),
        (latest - 1, latest)
    );
    assert_eq!(
        app.get_game_update_from_versions().row_count(),
        latest as usize
    );
    assert_eq!(app.get_game_update_to_versions().row_count(), 1);
    let version = |label: &str| archive.versions.iter().position(|v| v == label).unwrap() as i32;
    let previous = version("V3.4220");
    let current = version("V3.4411");
    app.invoke_select_game_update_versions(previous, current);
    let first = app.get_game_update_feed().row_data(0).unwrap();
    assert!(first.section && first.expanded);
    assert_eq!(first.id.as_str(), "section:new-monsters");
    let region = app.get_game_update_feed().row_data(1).unwrap();
    assert!(region.region && region.title.contains("Corrupted Pond"));
    assert_eq!((region.count, first.count), (4, 4));
    let newcomer = app
        .get_game_update_feed()
        .iter()
        .find(|r| r.record.route_key == "monster:P125")
        .unwrap()
        .record;
    assert_eq!(newcomer.drops.row_count(), 9);
    for (route, rate) in [("item:13324", "3%"), ("item:9464", "0.001%")] {
        assert!(newcomer
            .drops
            .iter()
            .any(|drop| drop.route == route && drop.rate == rate && drop.kind == "loot"));
    }
    let items = app
        .get_game_update_feed()
        .iter()
        .filter(|r| r.section)
        .nth(1)
        .unwrap();
    assert_eq!(items.id.as_str(), "section:new-items");
    assert_eq!(items.count, 108);
    let default_rows = app.get_game_update_feed().row_count();
    sample("collapse monsters", &|| {
        app.invoke_toggle_game_update_section(first.id.clone())
    });
    assert!(app.get_game_update_feed().row_count() < default_rows);
    sample("expand monsters", &|| {
        app.invoke_toggle_game_update_section(first.id.clone())
    });
    assert_eq!(app.get_game_update_feed().row_count(), default_rows);
    sample("first expand 108 items", &|| {
        app.invoke_toggle_game_update_section(items.id.clone())
    });
    let material = app
        .get_game_update_feed()
        .iter()
        .find(|row| row.id == "new-items:E")
        .unwrap();
    assert!(!material.expanded && material.count > 0);
    sample("expand material rows", &|| {
        app.invoke_toggle_game_update_section(material.id.clone())
    });
    assert!(app
        .get_game_update_feed()
        .iter()
        .any(|row| row.record.route_key.starts_with("item:")));
    sample("collapse items", &|| {
        app.invoke_toggle_game_update_section(items.id.clone())
    });
    sample("include unclassified", &|| {
        app.invoke_set_game_update_extras(true)
    });
    assert!(app.get_game_update_feed().row_count() > default_rows);
    app.invoke_set_game_update_extras(false);
    app.invoke_toggle_game_update_section("section:renamed".into());
    let english = app
        .get_game_update_feed()
        .iter()
        .find(|r| r.id == "renamed:en")
        .unwrap();
    assert_eq!(english.count, 23);
    assert!(app
        .get_game_update_feed()
        .iter()
        .any(|r| r.id == "renamed:en+ko" && r.count == 2));
    for name in ["Sliver Key", "Silver Key"] {
        sample(name, &|| app.invoke_search_game_updates(name.into()));
        let renamed = app
            .get_game_update_feed()
            .iter()
            .find(|row| row.id == "record:items:4518")
            .unwrap();
        assert_eq!(renamed.record.kind.as_str(), "renamed");
        assert_eq!(renamed.record.rename_label.as_str(), "僅英文改名");
        assert_eq!(renamed.record.name_changes.row_count(), 1);
        assert!(renamed.record.name_note.contains("繁中、韓文名稱未變"));
        assert!(renamed
            .record
            .fields
            .iter()
            .any(|f| f.before == "Sliver Key" && f.after == "Silver Key"));
    }
    for gourd in ["Iron-Crutch Li's Gourd", "Li Tieguai’s Gourd"] {
        app.invoke_search_game_updates(gourd.into());
        assert!(app.get_game_update_feed().iter().any(|r| r.record.route_key == "item:6340"));
    }
    app.invoke_search_game_updates("Master_Jongmu".into());
    let npc = app
        .get_game_update_feed()
        .iter()
        .find(|r| r.id == "record:npcs:1332")
        .unwrap()
        .record;
    assert_eq!(npc.name_changes.row_count(), 2);
    let changed: std::collections::HashSet<_> = npc
        .name_changes
        .iter()
        .map(|c| c.label.to_string())
        .collect();
    assert_eq!(
        changed,
        ["英文".to_string(), "韓文".to_string()]
            .into_iter()
            .collect()
    );
    assert!(npc.name_note.contains("新版未提供繁中譯名"));
    assert!(npc.name_note.contains("沉睡的靈魂"));
    app.invoke_search_game_updates("".into());
    let valid_pair = (previous, current);
    for (from, to) in [
        (current, previous),
        (current, current),
        (-1, current),
        (previous, latest + 1),
    ] {
        app.invoke_select_game_update_versions(from, to);
        assert_eq!(
            (app.get_game_update_from(), app.get_game_update_to()),
            valid_pair
        );
        assert!(app.get_game_update_lines().row_count() > 0);
    }
    app.invoke_search_game_updates("冰封之星".into());
    let drop = app
        .get_game_update_feed()
        .iter()
        .find(|r| r.id == "record:monster_drops:K188 / 4")
        .unwrap();
    let item = drop.record.drops.row_data(0).unwrap();
    assert_eq!(
        (
            item.route.as_str(),
            item.previous_rate.as_str(),
            item.rate.as_str()
        ),
        ("item:9605", "15%", "12%")
    );
    let monster = app
        .get_game_update_feed()
        .iter()
        .find(|r| r.record.route_key == "monster:K188")
        .unwrap();
    assert!(monster.record.location.contains("星之祠堂"));
    let handle = app.as_weak();
    app.on_open_hit(move |key| {
        let app = handle.unwrap();
        app.set_viewing(key.clone());
        crate::sheet::show_key(&app, &catalog, &session, &key, false);
        crate::list::sync_view_star(&app, &Default::default());
    });
    for (route, category, name) in [
        (item.route, "item", "冰封之星"),
        (monster.record.route_key.clone(), "monster", "道士"),
    ] {
        app.invoke_open_game_update_record(route.clone());
        assert_eq!(app.get_viewing(), route);
        assert_eq!(app.get_category().as_str(), category);
        assert!(!app.get_game_update_open());
        if category == "item" {
            assert!(app.get_viewing_item());
        } else {
            assert!(app.get_viewing_monster());
        }
        assert!(app.get_detail().contains(name) || app.get_sheet_title().contains(name));
        frame();
        back(&app);
        assert!(app.get_game_update_open());
        assert_eq!(app.get_category().as_str(), "patch");
    }
    for (key, language, before, after) in [
        ("item:4518", "英文", "Sliver Key", "Silver Key"),
        (
            "monster:H00",
            "英文",
            "Evil Taoist_Iron-Crutch Li",
            "Evil Taoist_Li Tieguai",
        ),
        ("hero:U96", "英文", "Sorcerer", "Exorcist"),
        ("item:13245", "韓文", "암흑정수", "심연의 암흑정수"),
    ] {
        app.invoke_open_game_update_record(key.into());
        assert_eq!(app.get_viewing(), key);
        let rows = app.get_catalog_name_history();
        assert_eq!(rows.row_count(), 1, "{key}");
        let row = rows.row_data(0).unwrap();
        assert_eq!(row.versions, "V3.4220 → V3.4411");
        assert_eq!(
            (
                row.language.as_str(),
                row.before.as_str(),
                row.after.as_str()
            ),
            (language, before, after)
        );
        back(&app);
        assert!(app.get_game_update_open());
    }
    app.invoke_open_game_update_record("monster:P125".into());
    assert_eq!(app.get_catalog_name_history().row_count(), 0);
    back(&app);
    assert_eq!(app.get_game_update_query().as_str(), "冰封之星");
    assert!(app
        .get_game_update_feed()
        .iter()
        .any(|r| r.id == monster.id));
}
