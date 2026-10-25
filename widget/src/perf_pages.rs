//! Opt-in timings for real page models and a synchronous software-rendered frame.
//! Run against an isolated data directory: normal calculator initialization may
//! migrate an existing saved document, just as it does in the normal app.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::rc::Rc;
use std::sync::{atomic::AtomicU64, mpsc, Arc};
use std::time::Instant;

use serde_json::{json, Value};
use slint::{ComponentHandle, Image, Model, ModelRc, VecModel};

use crate::{db, list, App, Hit, Session, UiLanguage};

#[derive(Clone, Copy)]
enum Page {
    Home,
    Category(&'static str),
    MonsterAll,
    QuestAll,
    Calculator(i32),
    Settings(i32),
}

fn elapsed(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn distribution(mut samples: Vec<f64>) -> Value {
    samples.sort_by(f64::total_cmp);
    json!({ "min_ms": samples[0], "median_ms": samples[samples.len() / 2], "max_ms": samples[samples.len() - 1] })
}

fn reset(app: &App) {
    app.set_settings_open(false);
    app.set_calculator_open(false);
    app.set_calculator_library_open(false);
    app.set_mcp_open(false);
    app.set_quick_search_open(false);
    app.set_quest_map_open(false);
    app.set_drop_return_key("".into());
    app.set_category("".into());
    app.set_nation("".into());
    app.set_nation_name("".into());
    app.set_area("".into());
    app.set_area_name("".into());
    app.set_place("".into());
    app.set_place_name("".into());
    app.set_viewing("".into());
    app.set_query("".into());
    app.set_search_open(false);
    app.set_favourites_only(false);
    app.set_ship("".into());
    app.set_expanded("".into());
    app.set_list_offset(0.0);
    app.set_cursor(0);
    app.set_links(ModelRc::new(VecModel::<Hit>::default()));
    app.set_board_monsters(ModelRc::new(VecModel::<Hit>::default()));
    list::reset_catalog_context(app);
}

fn prepare(
    app: &App,
    catalog: &db::Catalog,
    session: &Session,
    favourites: &HashSet<String>,
    aliases: &[(String, String)],
    page: Page,
) {
    reset(app);
    match page {
        Page::Home => list::show_home_saved(app, catalog, session, favourites),
        Page::Category(category) => {
            app.set_category(category.into());
            if category == "quest" {
                app.set_quest_summary(true);
                app.set_quest_category("qcat:all".into());
                app.set_quest_grouping("".into());
                app.set_quest_group_query("".into());
            }
            list::publish(app, catalog, session, favourites, category, "");
        }
        Page::MonsterAll => {
            app.set_category("monster".into());
            app.set_expanded("all:".into());
            list::publish(app, catalog, session, favourites, "monster", "");
        }
        Page::QuestAll => {
            app.set_category("quest".into());
            app.set_quest_summary(false);
            app.set_quest_category("qcat:all".into());
            app.set_quest_grouping("".into());
            app.set_quest_group_query("".into());
            list::publish(app, catalog, session, favourites, "quest", "");
        }
        Page::Calculator(tab) => {
            app.set_calculator_tab(tab);
            app.set_calculator_open(true);
            // Opening the calculator invokes this for every tab; bonus data was
            // eagerly prepared during install, and switching tabs only shows it.
            app.invoke_calculator_refresh();
        }
        Page::Settings(tab) => {
            app.set_settings_tab(tab);
            app.set_settings_open(true);
            if tab == 2 {
                crate::alias_ui::show_alias_rows(app, catalog, aliases);
            }
        }
    }
}

fn rows(app: &App, page: Page) -> Value {
    match page {
        Page::Home => json!({ "saved_rows": app.get_hits().row_count() }),
        Page::Calculator(0) => {
            json!({ "candidate_rows": app.get_calculator_candidates().row_count(), "material_rows": app.get_calculator_materials().row_count() })
        }
        Page::Calculator(1) => {
            json!({ "candidate_rows": app.get_character_calculator_candidates().row_count(), "material_rows": app.get_character_calculator_materials().row_count() })
        }
        Page::Calculator(_) => {
            json!({ "candidate_rows": app.get_bonus_candidates().row_count(), "tree_rows": app.get_bonus_rows().row_count() })
        }
        Page::Settings(1) => json!({ "dock_rows": app.get_dock_shown().row_count() }),
        Page::Settings(2) => json!({ "alias_rows": app.get_alias_rows().row_count() }),
        Page::Settings(_) => json!({}),
        _ => {
            json!({ "list_rows": app.get_hits().row_count(), "board_rows": app.get_board_monsters().row_count(), "section_rows": app.get_catalog_sections().row_count(), "region_rows": if matches!(page, Page::Category("monster") | Page::MonsterAll) { app.get_region_rows().row_count() } else { 0 } })
        }
    }
}

pub(crate) fn benchmark(path: &Path) -> Result<(), String> {
    let started = Instant::now();
    let mut startup = Vec::new();
    let start = Instant::now();
    let _running_guard = crate::update::running_guard()?;
    db::initialize_user_data()?;
    startup.push(json!({ "stage": "user_data_initialize", "elapsed_ms": elapsed(start) }));
    let start = Instant::now();
    let catalog = Rc::new(db::Catalog::open()?);
    startup.push(json!({ "stage": "catalog_open", "elapsed_ms": elapsed(start) }));
    let start = Instant::now();
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()
        .map_err(|e| e.to_string())?;
    let app = App::new().map_err(|e| e.to_string())?;
    app.set_full_mode(true);
    app.set_collapsed(false);
    app.set_lang("zh-en".into());
    app.set_primary_language("zh".into());
    app.set_secondary_language("en".into());
    app.global::<UiLanguage>().set_primary("zh".into());
    app.global::<UiLanguage>()
        .on_translate(|language, en, zh| db::ui_text(&language, &zh, &en).into());
    #[cfg(target_os = "macos")]
    app.set_ui_font("PingFang TC".into());
    app.set_monster_gallery(false);
    app.set_app_version(env!("CARGO_PKG_VERSION").into());
    startup.push(json!({ "stage": "backend_and_app_create", "elapsed_ms": elapsed(start) }));

    let cache = Rc::new(RefCell::new(HashMap::<String, Image>::new()));
    let (jobs, icon_rx) = mpsc::channel();
    let session = Session {
        cache: Rc::clone(&cache),
        jobs,
        generation: Arc::new(AtomicU64::new(0)),
        link_generation: Arc::new(AtomicU64::new(0)),
    };
    crate::ICON_CACHE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&cache)));
    // Keep real queue submission, but drain without decoding after each sample.
    // A worker cannot deliver icons without an event loop and would introduce
    // uncontrolled competing CPU work into synchronous snapshot measurements.
    let start = Instant::now();
    crate::calculator_ui::install(&app, Rc::clone(&catalog), Rc::clone(&cache));
    startup.push(
        json!({ "stage": "calculator_install_including_character", "elapsed_ms": elapsed(start) }),
    );
    let start = Instant::now();
    crate::bonus_ui::install(&app, Rc::clone(&catalog), Rc::clone(&cache));
    startup.push(json!({ "stage": "bonus_install", "elapsed_ms": elapsed(start) }));
    let start = Instant::now();
    crate::changelog::install(&app);
    let favourites = db::load_favourites();
    let aliases = db::alias_rows();
    crate::alias_ui::show_alias_rows(&app, &catalog, &aliases);
    let element_icons: Vec<Image> = (-1..=5)
        .map(|element| {
            catalog
                .element_icon_for_value(element)
                .and_then(|(path, frame)| crate::agf::frame_rgba(&path, frame).ok())
                .map(crate::icons::image_from_icon)
                .unwrap_or_default()
        })
        .collect();
    app.set_filter_element_icons(ModelRc::new(VecModel::from(element_icons)));
    if let Some((path, frame)) = catalog.hero_mark() {
        if let Ok(icon) = crate::agf::frame_rgba(&path, frame) {
            app.set_hero_logo(crate::icons::image_from_icon(icon));
            app.set_hero_logo_ready(true);
        }
    }
    if let Some((path, frame)) = catalog.linked_icon("item:1168") {
        if let Ok(icon) = crate::agf::frame_rgba(&path, frame) {
            app.set_enchant_logo(crate::icons::image_from_icon(icon));
        }
    }
    startup.push(
        json!({ "stage": "navigation_assets_aliases_changelog", "elapsed_ms": elapsed(start) }),
    );
    let start = Instant::now();
    app.show().map_err(|e| e.to_string())?;
    app.window()
        .set_size(slint::LogicalSize::new(1280.0, 800.0));
    startup.push(json!({ "stage": "window_show_and_size", "elapsed_ms": elapsed(start) }));
    let startup_ms = elapsed(started);

    let pages = [
        ("home", Page::Home),
        ("monster", Page::Category("monster")),
        ("monster.all", Page::MonsterAll),
        ("item", Page::Category("item")),
        ("recipe", Page::Category("recipe")),
        ("quest", Page::Category("quest")),
        ("quest.all", Page::QuestAll),
        ("enchant", Page::Category("enchant")),
        ("npc", Page::Category("npc")),
        ("hero", Page::Category("hero")),
        ("works", Page::Category("works")),
        ("atlas", Page::Category("atlas")),
        ("patch", Page::Category("patch")),
        ("calculator.equipment", Page::Calculator(0)),
        ("calculator.character", Page::Calculator(1)),
        ("calculator.bonus", Page::Calculator(2)),
        ("settings.look", Page::Settings(0)),
        ("settings.bar", Page::Settings(1)),
        ("settings.aliases", Page::Settings(2)),
        ("settings.app", Page::Settings(4)),
    ];
    let mut measurements = Vec::new();
    let mut screenshots = Vec::new();
    for (index, (name, page)) in pages.iter().enumerate() {
        let mut samples = Vec::new();
        let mut prep_times = Vec::new();
        let mut render_times = Vec::new();
        let mut total_times = Vec::new();
        for visit in 0..6 {
            if index != 0 || visit != 0 {
                prepare(&app, &catalog, &session, &favourites, &aliases, Page::Home);
                // Home itself needs a different intervening page to unmount it.
                if matches!(page, Page::Home) {
                    app.set_settings_open(true);
                    app.set_settings_tab(0);
                }
                std::hint::black_box(app.window().take_snapshot().map_err(|e| e.to_string())?);
                for _ in icon_rx.try_iter() {}
            }
            let start = Instant::now();
            prepare(&app, &catalog, &session, &favourites, &aliases, *page);
            let prep_ms = elapsed(start);
            let render_start = Instant::now();
            let snapshot = app
                .window()
                .take_snapshot()
                .map_err(|e| format!("{name}: {e}"))?;
            let render_ms = elapsed(render_start);
            let total_ms = elapsed(start);
            let queued_icons = icon_rx.try_iter().count();
            samples.push(json!({ "visit": visit, "prep_ms": prep_ms, "render_ms": render_ms, "total_ms": total_ms, "queued_icons_excluded": queued_icons }));
            if visit > 0 {
                prep_times.push(prep_ms);
                render_times.push(render_ms);
                total_times.push(total_ms);
            }
            if visit == 0 && ["monster.all", "quest.all", "calculator.equipment"].contains(name) {
                let screenshot_path = path.with_extension(format!("{name}.ppm"));
                let mut file =
                    std::fs::File::create(&screenshot_path).map_err(|e| e.to_string())?;
                write!(
                    file,
                    "P6\n{} {}\n255\n",
                    snapshot.width(),
                    snapshot.height()
                )
                .map_err(|e| e.to_string())?;
                let rgb: Vec<_> = snapshot
                    .as_bytes()
                    .chunks_exact(4)
                    .flat_map(|pixel| pixel[..3].iter().copied())
                    .collect();
                file.write_all(&rgb).map_err(|e| e.to_string())?;
                screenshots.push(json!({ "page": name, "file": screenshot_path.file_name().map(|name| name.to_string_lossy()), "width": snapshot.width(), "height": snapshot.height() }));
            }
        }
        eprintln!(
            "Page profile: {name} first {:.1} ms; repeat median {:.1} ms",
            samples[0]["total_ms"].as_f64().unwrap_or_default(),
            distribution(total_times.clone())["median_ms"]
                .as_f64()
                .unwrap_or_default()
        );
        measurements.push(json!({ "page": name, "rows": rows(&app, *page), "first": samples[0], "repeat_samples": 5,
            "repeat_prep": distribution(prep_times), "repeat_render": distribution(render_times), "repeat_total": distribution(total_times), "samples": samples }));
    }
    app.hide().map_err(|e| e.to_string())?;
    crate::ICON_CACHE.with(|slot| *slot.borrow_mut() = None);
    let report = json!({
        "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "debug_assertions": cfg!(debug_assertions), "renderer": "winit-software", "logical_width": 1280, "logical_height": 800,
        "scale_factor": app.window().scale_factor(), "language": "zh-en", "full_mode": true, "monster_gallery": false,
        "scope": "Real catalog and page models; synchronous query/model preparation + Slint layout and CPU snapshot raster. Includes synchronous icons and submission of asynchronous icon jobs. Excludes asynchronous icon decode/delivery, event-loop latency, native presentation, network/update checks, screenshot encoding and disk writes.",
        "first_visit_definition": "First visit within one shared process, after eager calculator/bonus initialization; not a cold OS cache or an independently cold process per page.",
        "repeat_definition": "Five revisits, each preceded by an untimed neutral home snapshot that unmounts the target; home revisits use settings as the intervening page.",
        "settings_scope": "Four full-mode tabs; keyboard settings are only available in widget mode. Update checks are not started.",
        "scenario_notes": {
            "monster": "Initial monster directory, before selecting a region or All.",
            "monster.all": "Reconstructs the complete monster page from home via publish; includes directory and region rebuild. An All click within an already-open monster page uses reload_board and may be faster.",
            "quest": "Initial quest summary/categories.",
            "quest.all": "Reconstructs the all-records quest page from home.",
            "calculator": "Fresh calculator entry with the indicated tab selected, using calculator_refresh. Switching tabs within an already-open calculator only changes visibility and is not measured here.",
            "settings.aliases": "Opening/selecting the Aliases tab, including alias-refresh. Reopening settings with the already-selected tab may reuse the model."
        },
        "icon_queue": "Queued jobs counted and discarded after each sample; no background decoder competes with these synchronous timings. Startup calculator/bonus icons remain in the shared cache.",
        "startup_ms_before_first_page": startup_ms, "startup": startup, "measurements": measurements, "screenshots": screenshots
    });
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
