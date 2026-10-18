#![cfg_attr(windows, windows_subsystem = "windows")]

mod agf;
mod alias_ui;
mod changelog;
mod db;
mod hotkey;
mod icons;
mod list;
mod perf;
mod calculator_ui;
mod calculator_export;
mod sheet;
mod update;
mod update_ui;
mod window;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use slint::{Image, Model, ModelRc, Timer, TimerMode, VecModel};

slint::include_modules!();

use alias_ui::{alias_code_list, alias_suggestions, alias_text, refresh_alias_icons, show_alias_rows};
use icons::{image_from_icon, show_battle_frame, spawn_icon_worker};
use list::{monster_query, publish, reflow_board, row_index, show_board_monsters, show_home_saved, sync_view_star};
use sheet::{focus_section, open_scope, show_key};
use window::{
    copy_to_clipboard, default_icon_size, default_main_size, enters_full, fit_on_screen, focus_window, full_window_size,
    leaves_full, place_fitted, place_window, remember_window, work_area_at,
};

struct IconJob {
    gen: u64,
    links: bool,
    cache_key: String,
    row_key: String,
    path: std::path::PathBuf,
    frame: i32,
}

#[derive(Clone)]
struct Session {
    cache: Rc<RefCell<HashMap<String, Image>>>,
    jobs: Sender<IconJob>,
    generation: Arc<AtomicU64>,
    link_generation: Arc<AtomicU64>,
}

thread_local! {
    static ICON_CACHE: RefCell<Option<Rc<RefCell<HashMap<String, Image>>>>> = RefCell::new(None);
}

struct BoardList {
    key: String,
    lang: String,
    hits: Vec<db::Hit>,
}

struct AnimClip {
    code: String,
    path: std::path::PathBuf,
    frames: Vec<i32>,
}

fn reload_board(app: &App, catalog: &db::Catalog, session: &Session, favourites: &std::collections::HashSet<String>, cache: &RefCell<BoardList>) {
    let key = if app.get_full_mode() && app.get_category() == "monster" {
        list::monster_filter_scope(app, catalog)
    } else {
        app.get_expanded().to_string()
    };
    if key.is_empty() {
        return;
    }
    perf::begin_update();
    perf::navigation(&app.get_category());
    let _timer = perf::Span::new("list.reload_board", 0);
    let query = monster_query(app);
    let lang = app.get_lang().to_string();
    let hits = if key == "saved:" {
        let all = catalog.monster_records("", &lang, "", "", "");
        list::full_monster_results(catalog, all, favourites, true, &query, false)
    } else if key == "all:" || key.starts_with("jump:") || key.starts_with("nation:") {
        let base = {
            let mut cached = cache.borrow_mut();
            if cached.key != key || cached.lang != lang {
                cached.hits = catalog.scope_monsters(&key, "", &lang, -1, -1, -1, "");
                cached.key = key.clone();
                cached.lang = lang;
            }
            cached.hits.clone()
        };
        list::full_monster_results(catalog, base, favourites, false, &query, false)
    } else {
        catalog.section_records(&key, &lang)
    };
    let detail = app.get_detail_key().to_string();
    let index = hits.iter().position(|hit| hit.key == detail);
    show_board_monsters(app, session, catalog, &app.get_nation(), hits, favourites);
    match index {
        Some(index) => app.set_detail_index(index as i32),
        None => {
            app.set_detail_key("".into());
            app.set_detail_title("".into());
            app.set_detail_index(-1);
        }
    }
}

fn park_disabled(order: &mut Vec<i32>, on: &[bool; 9]) {
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();
    for id in order.iter().copied() {
        if on.get(id as usize).copied().unwrap_or(false) {
            enabled.push(id);
        } else {
            disabled.push(id);
        }
    }
    order.clear();
    order.extend(enabled);
    order.extend(disabled);
}

fn shown_order(order: &[i32], on: &[bool; 9]) -> Vec<i32> {
    order
        .iter()
        .copied()
        .filter(|id| on.get(*id as usize).copied().unwrap_or(false))
        .collect()
}

fn push_dock_models(app: &App, order: &[i32]) {
    let shown = shown_order(order, &dock_flags(app));
    app.set_dock_order(ModelRc::new(VecModel::from(order.to_vec())));
    app.set_dock_draft(ModelRc::new(VecModel::from(order.to_vec())));
    app.set_dock_shown(ModelRc::new(VecModel::from(shown)));
}

fn dock_flags(app: &App) -> [bool; 9] {
    [
        app.get_dock_monster(),
        app.get_dock_item(),
        app.get_dock_recipe(),
        app.get_dock_quest(),
        app.get_dock_patch(),
        app.get_dock_npc(),
        app.get_dock_hero(),
        app.get_dock_works(),
        app.get_dock_atlas(),
    ]
}

fn shift_order(order: &mut Vec<i32>, from: i32, to: i32) {
    let len = order.len();
    let from = usize::try_from(from).unwrap_or(len);
    let to = usize::try_from(to).unwrap_or(len);
    if from >= len || to >= len || from == to {
        return;
    }
    let item = order.remove(from);
    order.insert(to, item);
}

fn home_array(order: &[i32]) -> [i32; 9] {
    let mut out = [0, 1, 2, 3, 4, 5, 6, 7, 8];
    if order.len() == 9 {
        out.copy_from_slice(order);
    }
    out
}

fn window_logical(window: &slint::Window) -> (f32, f32) {
    let scale = window.scale_factor().max(1.0);
    let size = window.size();
    (size.width as f32 / scale, size.height as f32 / scale)
}

fn stored_full_size(scale: f32) -> (f32, f32) {
    db::load_window()
        .filter(|pref| pref.has_full_size && pref.full_width >= 280.0 && pref.full_height >= 360.0)
        .map(|pref| (pref.full_width, pref.full_height))
        .unwrap_or_else(|| full_window_size(scale))
}

fn refresh_mode_pages(app: &App, full: bool, catalog: &db::Catalog, session: &Session, favourites: &std::collections::HashSet<String>) {
    list::reset_catalog_context(app);
    if full {
        app.set_favourites_only(false);
    }
    if !app.get_category().is_empty() && app.get_viewing().is_empty() {
        let query = app.get_query().to_string();
        publish(app, catalog, session, favourites, &app.get_category(), &query);
    } else if app.get_category().is_empty() {
        if full {
            show_home_saved(app, catalog, session, favourites);
        } else {
            app.set_hits(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
        }
    }
}

fn remember_dock(app: &App, size: (f32, f32)) {
    db::save_dock(&db::DockPref {
        vertical: app.get_dock_vertical(),
        show: [
            app.get_dock_monster(),
            app.get_dock_item(),
            app.get_dock_recipe(),
            app.get_dock_quest(),
            app.get_dock_patch(),
            app.get_dock_npc(),
            app.get_dock_hero(),
            app.get_dock_works(),
            app.get_dock_atlas(),
        ],
        order: read_order(&app.get_dock_order()),
        width: size.0,
        height: size.1,
    });
}

fn read_order(model: &ModelRc<i32>) -> [i32; 9] {
    home_array(&model.iter().collect::<Vec<_>>())
}

fn edit_view(change: impl FnOnce(&mut db::ViewPref)) {
    let mut view = db::load_view();
    change(&mut view);
    db::save_view(&view);
}

/// Headless preflight used by release CI and the update helper. Always inspect
/// the bundle beside this executable, without migrating or writing preferences.
fn check_installation() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let root = executable.parent().ok_or("Missing installation directory")?;
    for relative in ["data/gersang.db", "data/client-version.txt", "data/client/Assets/Textures/UI/Portraits/PORTRAIT_INVEN.AGF"] {
        if !root.join(relative).is_file() {
            return Err(format!("Missing packaged file: {relative}"));
        }
    }
    let conn = rusqlite::Connection::open_with_flags(
        root.join("data/gersang.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ).map_err(|error| error.to_string())?;
    let integrity: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if integrity != "ok" { return Err(format!("Catalog integrity check failed: {integrity}")); }
    for table in ["items", "monsters", "sources"] {
        let count: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if count == 0 { return Err(format!("Empty catalog table: {table}")); }
    }
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if let Some(index) = args.iter().position(|arg| arg == "--profile-ui") {
        let result = args.get(index + 1).ok_or_else(|| "Usage: --profile-ui <report.json>".to_string())
            .and_then(|path| perf::benchmark_ui(std::path::Path::new(path)));
        if let Err(error) = result { eprintln!("UI performance profile failed: {error}"); std::process::exit(1); }
        return;
    }
    if let Some(index) = args.iter().position(|arg| arg == "--profile-catalog") {
        let result = args.get(index + 1).ok_or_else(|| "Usage: --profile-catalog <report.json>".to_string())
            .and_then(|path| perf::benchmark(std::path::Path::new(path)));
        if let Err(error) = result { eprintln!("Performance profile failed: {error}"); std::process::exit(1); }
        return;
    }
    if update::run_helper_if_requested() {
        return;
    }
    if std::env::args().any(|arg| arg == "--check-installation") {
        match check_installation() {
            Ok(()) => println!("Gersang {}: installation OK", env!("CARGO_PKG_VERSION")),
            Err(message) => {
                eprintln!("Installation check failed: {message}");
                std::process::exit(1);
            }
        }
        return;
    }
    // Hold a shared installation lock until this process exits. The helper
    // obtains it exclusively before replacing files, including when another
    // copy of this portable app is still open.
    let running_guard = update::running_guard();
    let catalog_timer = perf::Span::new("startup.catalog_initialize", 1);
    let initialization = match &running_guard {
        Ok(_) => db::initialize_user_data().and_then(|()| db::Catalog::open()),
        Err(message) => Err(message.clone()),
    };
    let catalog = match initialization {
        Ok(catalog) => Rc::new(catalog),
        Err(message) => {
            eprintln!("{message}");
            let app = StartupError::new().expect("視窗建立失敗");
            app.set_message(message.into());
            app.on_dismiss(|| { let _ = slint::quit_event_loop(); });
            app.run().expect("視窗執行失敗");
            return;
        }
    };

    drop(catalog_timer);
    let app = App::new().expect("視窗建立失敗");
    let _performance_timer = perf::install(&app);
    changelog::install(&app);
    let _update_ui = update_ui::install(&app);
    app.on_open_source(window::open_source_repository);
    #[cfg(target_os = "macos")]
    app.set_ui_font("PingFang TC".into());
    app.set_lang(db::load_lang().into());
    app.set_window_opacity(db::load_opacity());
    app.set_show_set_extra(db::load_set_extra());
    let view = db::load_view();
    app.set_pin_top(view.pin);
    app.set_battle_view(view.battle);
    app.set_monster_gallery(view.gallery);
    app.set_animate_battle(view.animate);
    app.set_hotkeys_on(view.hotkeys);
    app.set_legacy_mode(view.legacy);
    app.set_client_version(db::client_label().into());
    let element_icons: Vec<Image> = (-1..=5).map(|element| {
        catalog.element_icon_for_value(element)
            .and_then(|(path, frame)| agf::frame_rgba(&path, frame).ok())
            .map(image_from_icon).unwrap_or_default()
    }).collect();
    app.set_filter_element_icons(ModelRc::new(VecModel::from(element_icons)));
    let mut home = db::load_home();
    if !home.size_chosen {
        home.size = default_icon_size();
    }
    app.set_icon_size(home.size);
    app.set_home_order(ModelRc::new(VecModel::from(home.order.to_vec())));
    let home_order = Rc::new(RefCell::new(home.order.to_vec()));
    let home_size = Rc::new(Cell::new(home.size));
    let dock = db::load_dock();
    app.set_dock_vertical(dock.vertical);
    app.set_dock_monster(dock.show[0]);
    app.set_dock_item(dock.show[1]);
    app.set_dock_recipe(dock.show[2]);
    app.set_dock_quest(dock.show[3]);
    app.set_dock_patch(dock.show[4]);
    app.set_dock_npc(dock.show[5]);
    app.set_dock_hero(dock.show[6]);
    app.set_dock_works(dock.show[7]);
    app.set_dock_atlas(dock.show[8]);
    let mut dock_loaded = dock.order.to_vec();
    park_disabled(&mut dock_loaded, &dock.show);
    push_dock_models(&app, &dock_loaded);
    let dock_order = Rc::new(RefCell::new(dock_loaded.clone()));
    let dock_draft = Rc::new(RefCell::new(dock_loaded));
    if let Some((path, frame)) = catalog.hero_mark() {
        if let Ok(icon) = agf::frame_rgba(&path, frame) {
            app.set_hero_logo(image_from_icon(icon));
            app.set_hero_logo_ready(true);
        }
    }

    let current = Rc::new(RefCell::new(String::new()));
    let history: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    let icons = Rc::new(RefCell::new(HashMap::<String, Image>::new()));
    let (icon_tx, icon_rx) = mpsc::channel();
    let session = Session {
        cache: Rc::clone(&icons),
        jobs: icon_tx,
        generation: Arc::new(AtomicU64::new(0)),
        link_generation: Arc::new(AtomicU64::new(0)),
    };
    ICON_CACHE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&icons)));
    spawn_icon_worker(
        app.as_weak(),
        icon_rx,
        Arc::clone(&session.generation),
        Arc::clone(&session.link_generation),
    );

    calculator_ui::install(&app, Rc::clone(&catalog), Rc::clone(&icons));
    let favourites = Rc::new(RefCell::new(db::load_favourites()));
    let alias_rows = Rc::new(RefCell::new(db::alias_rows()));
    let alias_suggest: Rc<RefCell<Vec<Hit>>> = Rc::new(RefCell::new(Vec::new()));
    let alias_query_text: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    show_alias_rows(&app, &catalog, &alias_rows.borrow());
    let alias_app = app.as_weak();
    let alias_catalog = Rc::clone(&catalog);
    let alias_draft = Rc::clone(&alias_rows);
    app.on_alias_name(move |index, text| {
        let mut rows = alias_draft.borrow_mut();
        if let Some(row) = rows.get_mut(index as usize) {
            row.0 = text.to_string();
        }
    });
    let alias_draft = Rc::clone(&alias_rows);
    app.on_alias_codes(move |index, text| {
        let mut rows = alias_draft.borrow_mut();
        if let Some(row) = rows.get_mut(index as usize) {
            row.1 = text.to_string();
        }
    });
    let alias_draft = Rc::clone(&alias_rows);
    let alias_app = alias_app.clone();
    let alias_catalog = Rc::clone(&alias_catalog);
    app.on_alias_add(move || {
        alias_draft.borrow_mut().push((String::new(), String::new()));
        let len = alias_draft.borrow().len();
        let Some(app) = alias_app.upgrade() else { return };
        show_alias_rows(&app, &alias_catalog, &alias_draft.borrow());
        app.set_alias_edit(len.saturating_sub(1) as i32);
    });
    let alias_app = app.as_weak();
    let alias_catalog = Rc::clone(&catalog);
    let alias_suggest_query = Rc::clone(&alias_suggest);
    let alias_query_draft = Rc::clone(&alias_rows);
    let alias_query_text_slot = Rc::clone(&alias_query_text);
    app.on_alias_query(move |index, text| {
        let Some(app) = alias_app.upgrade() else { return };
        let text = text.trim().to_string();
        *alias_query_text_slot.borrow_mut() = text.clone();
        app.set_alias_target(index);
        app.set_alias_pick(0);
        let skip = alias_query_draft
            .borrow()
            .get(index as usize)
            .map(|(_, codes)| alias_code_list(codes))
            .unwrap_or_default();
        let lang = app.get_lang().to_string();
        let hits = alias_suggestions(&alias_catalog, &text, &lang, &skip);
        *alias_suggest_query.borrow_mut() = hits.clone();
        app.set_alias_hits(ModelRc::new(VecModel::from(hits)));
    });
    let alias_app = app.as_weak();
    let alias_suggest_toggle = Rc::clone(&alias_suggest);
    app.on_alias_toggle(move |index| {
        let Some(app) = alias_app.upgrade() else { return };
        let mut hits = alias_suggest_toggle.borrow_mut();
        let index = index as usize;
        if let Some(hit) = hits.get_mut(index) {
            hit.chip = if hit.chip.as_str() == "yes" { String::new().into() } else { "yes".into() };
        }
        app.set_alias_hits(ModelRc::new(VecModel::from(hits.clone())));
    });
    let alias_draft = Rc::clone(&alias_rows);
    let alias_app = app.as_weak();
    let alias_catalog = Rc::clone(&catalog);
    let alias_suggest_finish = Rc::clone(&alias_suggest);
    let alias_query_kept = Rc::clone(&alias_query_text);
    app.on_alias_finish(move |index| {
        let Some(app) = alias_app.upgrade() else { return };
        let hits = alias_suggest_finish.borrow().clone();
        let mut chosen: Vec<String> = hits
            .iter()
            .filter(|hit| hit.chip.as_str() == "yes")
            .filter_map(|hit| hit.key.strip_prefix("item:").map(str::to_string))
            .collect();
        if chosen.is_empty() {
            if let Some(hit) = hits.get(app.get_alias_pick() as usize) {
                if let Some(code) = hit.key.strip_prefix("item:") {
                    chosen.push(code.to_string());
                }
            }
        }
        if !chosen.is_empty() {
            let mut rows = alias_draft.borrow_mut();
            if let Some((_, codes)) = rows.get_mut(index as usize) {
                let mut parts: Vec<String> = codes
                    .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(str::to_string)
                    .collect();
                for code in chosen {
                    if !parts.iter().any(|part| part == &code) {
                        parts.push(code);
                    }
                }
                *codes = parts.join(" ");
            }
        }
        drop(hits);
        let rows = alias_draft.borrow().clone();
        alias_catalog.set_aliases(&alias_text(&rows));
        refresh_alias_icons(&app, &alias_catalog, &rows);
        let skip = rows.get(index as usize).map(|(_, codes)| alias_code_list(codes)).unwrap_or_default();
        let query = alias_query_kept.borrow().clone();
        let lang = app.get_lang().to_string();
        let again = alias_suggestions(&alias_catalog, &query, &lang, &skip);
        *alias_suggest_finish.borrow_mut() = again.clone();
        app.set_alias_hits(ModelRc::new(VecModel::from(again)));
        app.set_alias_pick(0);
    });
    let alias_draft = Rc::clone(&alias_rows);
    let alias_app = app.as_weak();
    let alias_catalog = Rc::clone(&catalog);
    app.on_alias_drop(move |index, key| {
        let code = key.strip_prefix("item:").unwrap_or("").trim().to_string();
        if !code.is_empty() {
            let mut rows = alias_draft.borrow_mut();
            if let Some((_, codes)) = rows.get_mut(index as usize) {
                let parts: Vec<String> = codes
                    .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
                    .map(str::trim)
                    .filter(|part| !part.is_empty() && *part != code)
                    .map(str::to_string)
                    .collect();
                *codes = parts.join(" ");
            }
        }
        let Some(app) = alias_app.upgrade() else { return };
        let rows = alias_draft.borrow().clone();
        alias_catalog.set_aliases(&alias_text(&rows));
        refresh_alias_icons(&app, &alias_catalog, &rows);
    });
    let alias_draft = Rc::clone(&alias_rows);
    let alias_app = app.as_weak();
    let alias_catalog = Rc::clone(&catalog);
    app.on_alias_refresh(move || {
        let Some(app) = alias_app.upgrade() else { return };
        show_alias_rows(&app, &alias_catalog, &alias_draft.borrow());
    });
    let alias_draft = Rc::clone(&alias_rows);
    let alias_app = app.as_weak();
    let alias_catalog = Rc::clone(&catalog);
    app.on_alias_commit(move |index, key| {
        let code = key.strip_prefix("item:").unwrap_or("").trim().to_string();
        if code.is_empty() {
            return;
        }
        {
            let mut rows = alias_draft.borrow_mut();
            let index = index as usize;
            if let Some((_, codes)) = rows.get_mut(index) {
                let mut parts: Vec<String> = codes
                    .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(str::to_string)
                    .collect();
                if !parts.iter().any(|part| part == &code) {
                    parts.push(code);
                }
                *codes = parts.join(" ");
            }
        }
        let Some(app) = alias_app.upgrade() else { return };
        show_alias_rows(&app, &alias_catalog, &alias_draft.borrow());
        app.set_alias_hits(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
        app.set_alias_target(-1);
        app.set_alias_pick(0);
    });
    let alias_draft = Rc::clone(&alias_rows);
    let alias_app = app.as_weak();
    let alias_catalog = Rc::clone(&catalog);
    app.on_alias_remove(move |index| {
        let mut rows = alias_draft.borrow_mut();
        let index = index as usize;
        if index < rows.len() {
            rows.remove(index);
        }
        drop(rows);
        let Some(app) = alias_app.upgrade() else { return };
        show_alias_rows(&app, &alias_catalog, &alias_draft.borrow());
    });
    let alias_draft = Rc::clone(&alias_rows);
    let alias_app = app.as_weak();
    let alias_catalog = Rc::clone(&catalog);
    app.on_alias_cancel(move || {
        *alias_draft.borrow_mut() = db::alias_rows();
        let Some(app) = alias_app.upgrade() else { return };
        show_alias_rows(&app, &alias_catalog, &alias_draft.borrow());
    });
    let alias_draft = Rc::clone(&alias_rows);
    let alias_app = app.as_weak();
    let alias_catalog = Rc::clone(&catalog);
    let alias_session = session.clone();
    let alias_favs = Rc::clone(&favourites);
    app.on_alias_save(move || {
        let text = alias_text(&alias_draft.borrow());
        alias_catalog.set_aliases(&text);
        *alias_draft.borrow_mut() = db::alias_rows();
        let Some(app) = alias_app.upgrade() else { return };
        show_alias_rows(&app, &alias_catalog, &alias_draft.borrow());
        let query = app.get_query().to_string();
        if !query.is_empty() {
            publish(&app, &alias_catalog, &alias_session, &alias_favs.borrow(), &app.get_category(), &query);
        }
    });

    let update_aliases = Rc::clone(&alias_rows);
    let update_app = app.as_weak();
    app.on_prepare_update(move || {
        // A restart must not silently discard an alias editor draft. Keep the
        // ready download and let the user resolve the draft in its normal UI.
        if *update_aliases.borrow() != db::alias_rows() {
            if let Some(app) = update_app.upgrade() {
                app.set_update_error(if app.get_lang() == "en-zh" {
                    "You have unsaved aliases. Save or cancel them in the Aliases tab before restarting."
                } else {
                    "別名有未儲存的修改。請先到「別名」儲存或取消，再重新啟動更新。"
                }.into());
            }
            return false;
        }
        true
    });

    let search_app = app.as_weak();
    let search_catalog = Rc::clone(&catalog);
    let search_current = Rc::clone(&current);
    let search_history = Rc::clone(&history);
    let search_session = session.clone();
    let search_favs = Rc::clone(&favourites);
    app.on_query_changed(move |category, text| {
        let Some(app) = search_app.upgrade() else { return };
        *search_current.borrow_mut() = String::new();
        search_history.borrow_mut().clear();
        let previous_detail = app.get_detail_key().to_string();
        let was_inline = app.get_viewing().is_empty();
        publish(&app, &search_catalog, &search_session, &search_favs.borrow(), &category, &text);
        let retained_detail = app.get_detail_key().to_string();
        app.set_viewing("".into());
        if !retained_detail.is_empty() && retained_detail == previous_detail && was_inline {
            // List icons have a separate generation; retaining the preview avoids
            // decoding its image and rebuilding its facts on every keystroke.
            return;
        }
        app.set_detail("".into());
        app.set_viewing_monster(false);
        app.set_viewing_recipe(false);
        app.set_viewing_quest(false);
        app.set_viewing_npc(false);
        app.set_viewing_hero(false);
        app.set_viewing_item(false);
        app.set_viewing_set(false);
        app.set_item_tab("".into());
        app.set_viewing_favourite(false);
        app.set_links(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
        app.set_show_icon(false);
        app.set_show_element(false);
        if retained_detail.is_empty() {
            search_session.link_generation.fetch_add(1, Ordering::Relaxed);
        } else {
            show_key(&app, &search_catalog, &search_session, &retained_detail, false);
            if !app.get_sheet_title().is_empty() {
                app.set_detail_title(app.get_sheet_title());
            }
        }
    });

    let home_app = app.as_weak();
    let home_current = Rc::clone(&current);
    let home_history = Rc::clone(&history);
    let home_session = session.clone();
    let home_catalog = Rc::clone(&catalog);
    let home_favs = Rc::clone(&favourites);
    app.on_went_home(move || {
        let Some(app) = home_app.upgrade() else { return };
        home_session.generation.fetch_add(1, Ordering::Relaxed);
        home_session.link_generation.fetch_add(1, Ordering::Relaxed);
        list::reset_catalog_context(&app);
        *home_current.borrow_mut() = String::new();
        home_history.borrow_mut().clear();
        app.set_links(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
        if app.get_full_mode() {
            show_home_saved(&app, &home_catalog, &home_session, &home_favs.borrow());
        } else {
            app.set_hits(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
        }
    });

    let startup_window = db::load_window();
    if startup_window.as_ref().is_some_and(|pref| pref.full) {
        app.set_full_mode(true);
    }
    let saved_size = Rc::new(Cell::new(startup_window.as_ref().map(|pref| (pref.width, pref.height)).unwrap_or((560.0, 740.0))));
    let placed_size = Rc::clone(&saved_size);
    let dock_size = Rc::new(Cell::new((dock.width, dock.height)));
    let dock_vertical = Rc::new(Cell::new(dock.vertical));
    let dock_park = Rc::new(Cell::new((0i32, 0i32)));
    let expand_at = Rc::new(Cell::new(None::<(i32, i32)>));
    let size_hold = Rc::new(Cell::new(Instant::now()));
    let startup_placed = Rc::new(Cell::new(false));
    let shown_at = Instant::now();
    let collapse_app = app.as_weak();
    let dock_while_open = Rc::clone(&dock_size);
    let hold_on_switch = Rc::clone(&size_hold);
    let watch_saved = Rc::clone(&saved_size);
    let mode_saved = Rc::clone(&saved_size);
    app.on_collapse_changed(move |collapsed| {
        if collapsed && shown_at.elapsed() < Duration::from_millis(800) {
            return;
        }
        let Some(app) = collapse_app.upgrade() else { return };
        let (width, height) = window_logical(app.window());
        if collapsed {
            if !app.get_full_mode() && width > 160.0 && height > 160.0 {
                saved_size.set((width, height));
            }
        } else if width >= 48.0 && height >= 48.0 && (width < 220.0 || height < 220.0) {
            dock_while_open.set((width, height));
            remember_dock(&app, dock_while_open.get());
        }
        hold_on_switch.set(Instant::now() + Duration::from_millis(500));
        let later = collapse_app.clone();
        let saved_size = Rc::clone(&saved_size);
        let dock_size = Rc::clone(&dock_while_open);
        let dock_park = Rc::clone(&dock_park);
        let expand_at = Rc::clone(&expand_at);
        Timer::single_shot(Duration::from_millis(16), move || {
            let Some(app) = later.upgrade() else { return };
            let window = app.window();
            if collapsed {
                let pos = window.position();
                let (width, height) = dock_size.get();
                window.set_size(slint::LogicalSize::new(width, height));
                if let Some((fx, fy)) = expand_at.replace(None) {
                    if (pos.x - fx).abs() <= 12 && (pos.y - fy).abs() <= 12 {
                        let (x, y) = dock_park.get();
                        window.set_position(slint::PhysicalPosition::new(x, y));
                    }
                }
            } else {
                let pos = window.position();
                let size = window.size();
                dock_park.set((pos.x, pos.y));
                let (width, height) = if app.get_full_mode() {
                    stored_full_size(window.scale_factor())
                } else {
                    saved_size.get()
                };
                let scale = window.scale_factor().max(1.0);
                let new_w = (width * scale).round() as i32;
                let new_h = (height * scale).round() as i32;
                let work = work_area_at(pos.x, pos.y, size.width as i32, size.height as i32);
                let (x, y) = fit_on_screen(
                    pos.x,
                    pos.y,
                    size.width as i32,
                    size.height as i32,
                    new_w,
                    new_h,
                    work,
                );
                expand_at.set(Some((x, y)));
                window.set_position(slint::PhysicalPosition::new(x, y));
                window.set_size(slint::LogicalSize::new(width, height));
            }
        });
    });
    let watch_app = app.as_weak();
    let watch_dock = Rc::clone(&dock_size);
    let watch_hold = Rc::clone(&size_hold);
    let watch_placed = Rc::clone(&startup_placed);
    let watch_full = Rc::new(Cell::new(
        startup_window.as_ref()
            .filter(|pref| pref.has_full_pos)
            .map(|pref| (pref.full_x, pref.full_y))
            .unwrap_or((0, 0)),
    ));
    let watch_full_size = Rc::new(Cell::new(
        startup_window.as_ref()
            .filter(|pref| pref.has_full_size)
            .map(|pref| (pref.full_width, pref.full_height))
            .unwrap_or((0.0, 0.0)),
    ));
    let watch_catalog = Rc::clone(&catalog);
    let watch_session = session.clone();
    let watch_favs = Rc::clone(&favourites);
    let size_watch = Timer::default();
    size_watch.start(TimerMode::Repeated, Duration::from_millis(80), move || {
        if !watch_placed.get() || Instant::now() < watch_hold.get() {
            return;
        }
        let Some(app) = watch_app.upgrade() else { return };
        let (width, height) = window_logical(app.window());
        if width < 48.0 || height < 48.0 {
            return;
        }
        if app.get_collapsed() {
            let previous = watch_dock.get();
            if (previous.0 - width).abs() > 1.0 || (previous.1 - height).abs() > 1.0 {
                watch_dock.set((width, height));
                remember_dock(&app, (width, height));
            }
            return;
        }
        let pos = app.window().position();
        if app.get_full_mode() {
            if leaves_full(width, height) {
                watch_hold.set(Instant::now() + Duration::from_millis(400));
                app.set_full_mode(false);
                watch_saved.set((width, height));
                let mut pref = db::load_window().unwrap_or(db::WindowPref::widget(width, height, pos.x, pos.y));
                pref.width = width;
                pref.height = height;
                pref.x = pos.x;
                pref.y = pos.y;
                pref.full = false;
                db::save_window(&pref);
                refresh_mode_pages(&app, false, &watch_catalog, &watch_session, &watch_favs.borrow());
            } else {
                let previous = watch_full.get();
                let previous_size = watch_full_size.get();
                let moved = previous != (pos.x, pos.y);
                let resized = (previous_size.0 - width).abs() > 1.0 || (previous_size.1 - height).abs() > 1.0;
                if moved || resized {
                    watch_full.set((pos.x, pos.y));
                    watch_full_size.set((width, height));
                    db::save_full_geometry(width, height, pos.x, pos.y);
                }
            }
            return;
        }
        if enters_full(width, height) {
            watch_hold.set(Instant::now() + Duration::from_millis(400));
            app.set_full_mode(true);
            watch_full.set((pos.x, pos.y));
            watch_full_size.set((width, height));
            db::save_full_geometry(width, height, pos.x, pos.y);
            refresh_mode_pages(&app, true, &watch_catalog, &watch_session, &watch_favs.borrow());
            return;
        }
        if width > 280.0 && height > 360.0 {
            let previous = watch_saved.get();
            watch_saved.set((width, height));
            if (previous.0 - width).abs() > 1.0 || (previous.1 - height).abs() > 1.0 {
                remember_window(app.window());
            }
        }
    });
    let mode_app = app.as_weak();
    let mode_hold = Rc::clone(&size_hold);
    let mode_catalog = Rc::clone(&catalog);
    let mode_session = session.clone();
    let mode_favs = Rc::clone(&favourites);
    app.on_mode_changed(move |full| {
        let Some(app) = mode_app.upgrade() else { return };
        mode_hold.set(Instant::now() + Duration::from_millis(500));
        let window = app.window();
        if full {
            if !app.get_collapsed() {
                let (width, height) = window_logical(window);
                if width > 280.0 && height > 360.0 {
                    mode_saved.set((width, height));
                    remember_window(window);
                }
            }
            let (width, height) = stored_full_size(window.scale_factor());
            let prefer = db::load_window().and_then(|pref| pref.has_full_pos.then_some((pref.full_x, pref.full_y)));
            let pos = place_fitted(window, width, height, prefer);
            db::save_full_geometry(width, height, pos.0, pos.1);
        } else {
            let pos = window.position();
            db::save_mode(false, Some((pos.x, pos.y)));
            if !app.get_collapsed() {
                if let Some(pref) = db::load_window() {
                    mode_saved.set((pref.width, pref.height));
                    place_fitted(window, pref.width, pref.height, Some((pref.x, pref.y)));
                }
            }
        }
        if full {
            app.set_favourites_only(false);
        }
        refresh_mode_pages(&app, full, &mode_catalog, &mode_session, &mode_favs.borrow());
    });
    let reflow_app = app.as_weak();
    app.on_reflow(move |span| {
        let Some(app) = reflow_app.upgrade() else { return };
        if app.get_flow_h() > 0 && (app.get_board_span() - span).abs() < 20.0 {
            return;
        }
        app.set_board_span(span);
        reflow_board(&app);
    });
    let dock_remember = Rc::clone(&dock_size);
    let dock_axis = Rc::clone(&dock_vertical);
    let dock_order_watch = Rc::clone(&dock_order);
    let dock_changed_app = app.as_weak();
    app.on_dock_changed(move |vertical, _, _, _, _, _, _, _, _, _| {
        let Some(app) = dock_changed_app.upgrade() else { return };
        let (width, height) = dock_remember.get();
        let swapped = if vertical != dock_axis.get() {
            (height, width)
        } else {
            (width, height)
        };
        dock_axis.set(vertical);
        dock_remember.set(swapped);
        let order = dock_order_watch.borrow().clone();
        push_dock_models(&app, &order);
        remember_dock(&app, swapped);
    });
    let shift_app = app.as_weak();
    let shift_draft = Rc::clone(&dock_draft);
    app.on_dock_shift(move |index, delta| {
        let Some(app) = shift_app.upgrade() else { return };
        let mut draft = shift_draft.borrow_mut();
        shift_order(&mut draft, index, index + delta);
        app.set_dock_draft(ModelRc::new(VecModel::from(draft.clone())));
    });
    let moved_app = app.as_weak();
    let moved_draft = Rc::clone(&dock_draft);
    let moved_order = Rc::clone(&dock_order);
    let moved_size = Rc::clone(&dock_size);
    app.on_dock_moved(move |from, to| {
        let Some(app) = moved_app.upgrade() else { return };
        let mut draft = moved_draft.borrow_mut();
        shift_order(&mut draft, from, to);
        park_disabled(&mut draft, &dock_flags(&app));
        let next = draft.clone();
        drop(draft);
        *moved_order.borrow_mut() = next.clone();
        push_dock_models(&app, &next);
        remember_dock(&app, moved_size.get());
    });
    let park_app = app.as_weak();
    let park_draft = Rc::clone(&dock_draft);
    let park_order = Rc::clone(&dock_order);
    let park_size = Rc::clone(&dock_size);
    app.on_dock_park(move || {
        let Some(app) = park_app.upgrade() else { return };
        let mut draft = park_draft.borrow_mut();
        park_disabled(&mut draft, &dock_flags(&app));
        let next = draft.clone();
        drop(draft);
        *park_order.borrow_mut() = next.clone();
        push_dock_models(&app, &next);
        remember_dock(&app, park_size.get());
    });
    let save_app = app.as_weak();
    let save_order = Rc::clone(&dock_order);
    let save_draft = Rc::clone(&dock_draft);
    let save_size = Rc::clone(&dock_size);
    app.on_dock_order_save(move || {
        let Some(app) = save_app.upgrade() else { return };
        let draft = save_draft.borrow().clone();
        *save_order.borrow_mut() = draft.clone();
        app.set_dock_order(ModelRc::new(VecModel::from(draft)));
        remember_dock(&app, save_size.get());
    });
    let cancel_app = app.as_weak();
    let cancel_order = Rc::clone(&dock_order);
    let cancel_draft = Rc::clone(&dock_draft);
    app.on_dock_order_cancel(move || {
        let Some(app) = cancel_app.upgrade() else { return };
        let order = cancel_order.borrow().clone();
        *cancel_draft.borrow_mut() = order.clone();
        app.set_dock_draft(ModelRc::new(VecModel::from(order)));
    });
    let reorder_app = app.as_weak();
    let reorder_order = Rc::clone(&home_order);
    let reorder_size = Rc::clone(&home_size);
    app.on_home_reordered(move |from, to| {
        let Some(app) = reorder_app.upgrade() else { return };
        let mut order = reorder_order.borrow_mut();
        shift_order(&mut order, from, to);
        db::save_home(&db::HomePref {
            size: reorder_size.get(),
            size_chosen: true,
            order: home_array(&order),
        });
        app.set_home_order(ModelRc::new(VecModel::from(order.clone())));
    });
    let size_order = Rc::clone(&home_order);
    let size_cell = Rc::clone(&home_size);
    app.on_icon_size_set(move |size| {
        let size = size.clamp(0, 2);
        size_cell.set(size);
        db::save_home(&db::HomePref {
            size,
            size_chosen: true,
            order: home_array(&size_order.borrow()),
        });
    });
    let close_app = app.as_weak();
    let close_dock = Rc::clone(&dock_size);
    let drag_app = app.as_weak();
    app.on_dock_drag(move |dx, dy| {
        let Some(app) = drag_app.upgrade() else { return };
        let window = app.window();
        let scale = window.scale_factor();
        let pos = window.position();
        window.set_position(slint::PhysicalPosition::new(
            pos.x + (dx * scale).round() as i32,
            pos.y + (dy * scale).round() as i32,
        ));
    });
    app.on_request_close(move || {
        if let Some(app) = close_app.upgrade() {
            if app.get_full_mode() && !app.get_collapsed() {
                let (width, height) = window_logical(app.window());
                let pos = app.window().position();
                db::save_full_geometry(width, height, pos.x, pos.y);
            } else if !app.get_collapsed() {
                remember_window(app.window());
            }
            if app.get_collapsed() {
                let window = app.window();
                let scale = window.scale_factor().max(1.0);
                let size = window.size();
                let width = size.width as f32 / scale;
                let height = size.height as f32 / scale;
                if width >= 48.0 && height >= 48.0 {
                    close_dock.set((width, height));
                    remember_dock(&app, close_dock.get());
                }
            }
        }
        let _ = slint::quit_event_loop();
    });

    app.on_opacity_changed(move |value| {
        db::save_opacity(value);
    });
    app.on_pin_changed(move |mode| {
        let mode = mode.clamp(0, 2);
        edit_view(|view| view.pin = mode);
    });
    app.on_battle_changed(move |battle| {
        edit_view(|view| view.battle = battle);
    });
    app.on_gallery_changed(move |gallery| {
        edit_view(|view| view.gallery = gallery);
    });
    app.on_animate_changed(move |animate| {
        edit_view(|view| view.animate = animate);
    });
    let hotkeys_app = app.as_weak();
    app.on_hotkeys_changed(move |on| {
        edit_view(|view| view.hotkeys = on);
        if let Some(app) = hotkeys_app.upgrade() {
            hotkey::set_enabled(on && !app.get_full_mode());
        }
    });
    let hotkey_scope_app = app.as_weak();
    app.on_hotkey_scope_changed(move |widget_mode| {
        if let Some(app) = hotkey_scope_app.upgrade() {
            hotkey::set_enabled(widget_mode && app.get_hotkeys_on());
        }
    });
    let legacy_app = app.as_weak();
    app.on_choose_legacy(move |on| {
        if let Some(app) = legacy_app.upgrade() {
            app.set_legacy_mode(on);
        }
        edit_view(|view| view.legacy = on);
    });
    let extra_app = app.as_weak();
    let extra_catalog = Rc::clone(&catalog);
    let extra_session = session.clone();
    app.on_show_set_extra_changed(move |on| {
        db::save_set_extra(on);
        let Some(app) = extra_app.upgrade() else { return };
        let viewing = app.get_viewing().to_string();
        if viewing.starts_with("set:") {
            show_key(&app, &extra_catalog, &extra_session, &viewing, true);
        }
    });

    let lang_app = app.as_weak();
    let lang_catalog = Rc::clone(&catalog);
    let lang_current = Rc::clone(&current);
    let lang_session = session.clone();
    let lang_favs = Rc::clone(&favourites);
    let lang_aliases = Rc::clone(&alias_rows);
    app.on_lang_changed(move |lang| {
        let Some(app) = lang_app.upgrade() else { return };
        let lang = lang.to_string();
        db::save_lang(&lang);
        let nation = app.get_nation().to_string();
        let area = app.get_area().to_string();
        if !area.is_empty() && app.get_category() != "quest" {
            app.set_area_name(lang_catalog.area_name(&nation, &area, &lang).into());
        }
        let place = app.get_place().to_string();
        if app.get_category() == "quest" && !place.is_empty() {
            app.set_place_name(db::source_label(&place, &lang).into());
        }
        if let Some(id) = place.strip_prefix("span:") {
            app.set_place_name(lang_catalog.span_name(&nation, id, &lang).into());
        } else if place.parse::<i64>().is_ok() {
            app.set_place_name(lang_catalog.region_name(&place, &lang).into());
        }
        let category = app.get_category();
        if !category.is_empty() {
            publish(&app, &lang_catalog, &lang_session, &lang_favs.borrow(), &category, &app.get_query());
        }
        let key = lang_current.borrow().clone();
        if !key.is_empty() {
            show_key(&app, &lang_catalog, &lang_session, &key, false);
            sync_view_star(&app, &lang_favs.borrow());
        } else if !app.get_detail_key().is_empty() {
            let cursor = app.get_cursor();
            let offset = app.get_list_offset();
            show_key(&app, &lang_catalog, &lang_session, &app.get_detail_key(), false);
            app.set_cursor(cursor);
            app.set_list_offset(offset);
            if !app.get_sheet_title().is_empty() {
                app.set_detail_title(app.get_sheet_title());
            }
        }
        show_alias_rows(&app, &lang_catalog, &lang_aliases.borrow());
        app.invoke_calculator_refresh();
        if app.get_full_mode() && app.get_category().is_empty() {
            show_home_saved(&app, &lang_catalog, &lang_session, &lang_favs.borrow());
        }
    });

    let open_app = app.as_weak();
    let open_catalog = catalog;
    let open_current = current;
    let open_session = session;
    let open_favs = Rc::clone(&favourites);
    let open_history = Rc::clone(&history);
    let fav_catalog = Rc::clone(&open_catalog);
    let fav_session = open_session.clone();
    let board_list = Rc::new(RefCell::new(BoardList {
        key: String::new(),
        lang: String::new(),
        hits: Vec::new(),
    }));
    let restore_app = app.as_weak();
    let restore_catalog = Rc::clone(&open_catalog);
    let restore_current = Rc::clone(&open_current);
    let restore_session = open_session.clone();
    let restore_favs = Rc::clone(&open_favs);
    let restore_history = Rc::clone(&open_history);
    app.on_restore_view(move || {
        let Some(app) = restore_app.upgrade() else { return false };
        let Some(key) = restore_history.borrow_mut().pop() else { return false };
        *restore_current.borrow_mut() = key.clone();
        app.set_viewing(key.clone().into());
        show_key(&app, &restore_catalog, &restore_session, &key, false);
        sync_view_star(&app, &restore_favs.borrow());
        true
    });
    let forget_current = Rc::clone(&open_current);
    let forget_history = Rc::clone(&open_history);
    app.on_forget_view(move || {
        *forget_current.borrow_mut() = String::new();
        forget_history.borrow_mut().clear();
    });
    let source_app = app.as_weak();
    let source_catalog = Rc::clone(&open_catalog);
    let source_session = open_session.clone();
    app.on_source_changed(move |key| {
        let Some(app) = source_app.upgrade() else { return };
        let inline = app.get_viewing().is_empty() && app.get_full_mode();
        let viewing = if inline { app.get_detail_key().to_string() } else { app.get_viewing().to_string() };
        if !viewing.starts_with("item:") && !viewing.starts_with("set:") && !viewing.starts_with("hero:") {
            return;
        }
        let key = key.to_string();
        if app.get_item_source() == key {
            app.set_item_source("".into());
        } else {
            app.set_item_source(key.clone().into());
        }
        let cursor = app.get_cursor();
        let offset = app.get_list_offset();
        show_key(&app, &source_catalog, &source_session, &viewing, true);
        if inline {
            app.set_cursor(cursor);
            app.set_list_offset(offset);
        } else {
            focus_section(&app, &key);
        }
    });
    let toggle_app = app.as_weak();
    let toggle_catalog = Rc::clone(&open_catalog);
    let toggle_session = open_session.clone();
    let toggle_favs = Rc::clone(&favourites);
    let toggle_list = Rc::clone(&board_list);
    app.on_toggle_board(move |key| {
        let Some(app) = toggle_app.upgrade() else { return };
        let key = key.to_string();
        if key.is_empty() {
            return;
        }
        let full_monsters = app.get_full_mode() && app.get_category() == "monster";
        let was_global = full_monsters && app.get_filter_global();
        if full_monsters {
            app.set_filter_global(false);
        }
        if app.get_expanded().as_str() == key && !was_global {
            if full_monsters && key == "all:" {
                return;
            }
            app.set_expanded("".into());
            app.set_detail_key("".into());
            app.set_detail_title("".into());
            app.set_detail_index(-1);
            app.set_board_monsters(ModelRc::new(VecModel::from(Vec::new())));
            toggle_session.generation.fetch_add(1, Ordering::Relaxed);
            return;
        }
        app.set_expanded(key.clone().into());
        app.set_detail_key("".into());
        app.set_detail_title("".into());
        app.set_detail_index(-1);
        reload_board(&app, &toggle_catalog, &toggle_session, &toggle_favs.borrow(), &toggle_list);
    });
    let detail_app = app.as_weak();
    let detail_catalog = Rc::clone(&open_catalog);
    let detail_session = open_session.clone();
    app.on_toggle_detail(move |key, index| {
        let Some(app) = detail_app.upgrade() else { return };
        let key = key.to_string();
        if key.is_empty() {
            return;
        }
        if app.get_detail_key().as_str() == key {
            app.set_detail_key("".into());
            app.set_detail_title("".into());
            app.set_detail_index(-1);
            return;
        }
        app.set_detail_key(key.clone().into());
        let title = [app.get_hits(), app.get_board_monsters()].into_iter()
            .find_map(|rows| (0..rows.row_count()).filter_map(|row| rows.row_data(row))
                .find(|row| row.key.as_str() == key).map(|row| row.title))
            .unwrap_or_default();
        app.set_detail_title(title);
        app.set_detail_index(index);
        let cursor = app.get_cursor();
        let offset = app.get_list_offset();
        show_key(&app, &detail_catalog, &detail_session, &key, false);
        if app.get_full_mode() && app.get_category() != "monster" {
            app.set_cursor(if app.get_catalog_searching() || app.get_catalog_sections().row_count() == 0 { index } else { cursor });
            app.set_list_offset(offset);
        }
    });
    let inline_app = app.as_weak();
    let inline_catalog = Rc::clone(&open_catalog);
    let inline_session = open_session.clone();
    app.on_reload_inline_detail(move || {
        let Some(app) = inline_app.upgrade() else { return };
        let key = app.get_detail_key().to_string();
        if !key.is_empty() {
            // A linked page replaces the shared facts and drops; restore the open row without toggling it.
            let cursor = app.get_cursor();
            let offset = app.get_list_offset();
            show_key(&app, &inline_catalog, &inline_session, &key, false);
            if app.get_category() != "monster" {
                app.set_cursor(cursor);
                app.set_list_offset(offset);
            }
        }
    });
    let suggest_app = app.as_weak();
    let suggest_catalog = Rc::clone(&open_catalog);
    app.on_suggest_drops(move |query| {
        let Some(app) = suggest_app.upgrade() else { return };
        let choices = suggest_catalog.drop_options(&query, &app.get_lang());
        app.set_drop_options(ModelRc::new(VecModel::from(list::plain_hits(choices))));
    });
    let regions_app = app.as_weak();
    let regions_catalog = Rc::clone(&open_catalog);
    app.on_refine_regions(move || {
        let Some(app) = regions_app.upgrade() else { return };
        if !app.get_full_mode() || app.get_category() != "monster" {
            return;
        }
        list::refresh_region_rows(&app, &regions_catalog);
    });
    let drop_monsters_app = app.as_weak();
    app.on_find_drop_monsters(move |key, title| {
        let Some(app) = drop_monsters_app.upgrade() else { return };
        if !app.get_full_mode() || !key.strip_prefix("item:").and_then(|code| code.parse::<i64>().ok()).is_some_and(|code| code > 0) {
            return;
        }
        app.set_category("monster".into());
        app.set_nation("".into());
        app.set_nation_name("".into());
        app.set_area("".into());
        app.set_area_name("".into());
        app.set_place("".into());
        app.set_place_name("".into());
        app.set_query("".into());
        app.set_search_open(false);
        app.set_favourites_only(false);
        app.set_filter_global(false);
        app.set_filter_element(-1);
        app.set_filter_ela_min("".into());
        app.set_filter_ela_max("".into());
        app.set_filter_hit_min("".into());
        app.set_filter_hit_max("".into());
        app.set_filter_magic_min("".into());
        app.set_filter_magic_max("".into());
        app.set_filter_drop_key(key);
        app.set_filter_drop(title);
        app.set_expanded("all:".into());
        app.set_list_offset(0.0);
        app.set_cursor(0);
        // Reuse category navigation so an open item page and its detail history are cleared too.
        app.invoke_query_changed("monster".into(), "".into());
    });
    let filter_app = app.as_weak();
    let filter_catalog = Rc::clone(&open_catalog);
    let filter_session = open_session.clone();
    let filter_favs = Rc::clone(&favourites);
    let filter_list = Rc::clone(&board_list);
    let filter_ticket = Rc::new(Cell::new(0u64));
    app.on_apply_filter(move |now| {
        let id = filter_ticket.get().wrapping_add(1);
        filter_ticket.set(id);
        let ticket = Rc::clone(&filter_ticket);
        let app = filter_app.clone();
        let catalog = Rc::clone(&filter_catalog);
        let session = filter_session.clone();
        let favs = Rc::clone(&filter_favs);
        let list = Rc::clone(&filter_list);
        let fire = move || {
            if ticket.get() != id {
                return;
            }
            let Some(app) = app.upgrade() else { return };
            if !app.get_full_mode() || app.get_category() != "monster" {
                return;
            }
            app.set_list_offset(0.0);
            app.set_cursor(0);
            if !app.get_query().trim().is_empty() || app.get_favourites_only()
                || !app.get_place().is_empty() || !app.get_area().is_empty() || !app.get_nation().is_empty()
            {
                publish(&app, &catalog, &session, &favs.borrow(), "monster", &app.get_query());
            } else {
                reload_board(&app, &catalog, &session, &favs.borrow(), &list);
            }
        };
        if now {
            fire();
        } else {
            Timer::single_shot(Duration::from_millis(160), fire);
        }
    });
    let anim_catalog = Rc::clone(&open_catalog);
    let saved_open_catalog = Rc::clone(&open_catalog);
    let saved_open_session = open_session.clone();
    let saved_open_favs = Rc::clone(&open_favs);
    let saved_open_current = Rc::clone(&open_current);
    let saved_open_history = Rc::clone(&open_history);
    app.on_open_hit(move |key| {
        let Some(app) = open_app.upgrade() else { return };
        let mut key = key.to_string();
        if key.is_empty() || key.starts_with("section:") {
            return;
        }
        if let Some(rest) = key.strip_prefix("jump:").map(str::to_string) {
            if let Some((nation, tail)) = rest.split_once('|') {
                let lang = app.get_lang();
                app.set_nation(nation.into());
                app.set_nation_name(db::nation_label(nation, &lang).into());
                app.set_area("".into());
                app.set_area_name("".into());
                app.set_place("".into());
                app.set_place_name("".into());
                key = tail.to_string();
            }
        }
        if let Some(id) = key.strip_prefix("nation:") {
            let lang = app.get_lang();
            app.set_nation(id.into());
            app.set_nation_name(db::nation_label(id, &lang).into());
            app.set_area("".into());
            app.set_area_name("".into());
            app.set_place("".into());
            app.set_place_name("".into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "monster");
            return;
        }
        if let Some(id) = key.strip_prefix("area:") {
            let lang = app.get_lang().to_string();
            let nation = app.get_nation().to_string();
            app.set_area(id.into());
            app.set_area_name(open_catalog.area_name(&nation, id, &lang).into());
            app.set_place("".into());
            app.set_place_name("".into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "monster");
            return;
        }
        if let Some(id) = key.strip_prefix("span:") {
            let lang = app.get_lang().to_string();
            let nation = app.get_nation().to_string();
            app.set_place(format!("span:{id}").into());
            app.set_place_name(open_catalog.span_name(&nation, id, &lang).into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "monster");
            return;
        }
        if let Some(kind) = key.strip_prefix("icat:") {
            let lang = app.get_lang();
            app.set_place(kind.into());
            app.set_place_name(db::item_category_label(kind, &lang).into());
            if kind == "set" {
                app.set_area("0".into());
            } else {
                app.set_area("".into());
            }
            app.set_area_name("".into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "item");
            return;
        }
        if let Some(kind) = key.strip_prefix("isub:") {
            let lang = app.get_lang();
            app.set_area(kind.into());
            app.set_area_name(db::weapon_family_label(kind, &lang).into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "item");
            return;
        }
        if let Some(source) = key.strip_prefix("craft:") {
            let lang = app.get_lang();
            app.set_place(source.into());
            app.set_place_name(db::source_label(source, &lang).into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "recipe");
            return;
        }
        if let Some(rank) = key.strip_prefix("rank:") {
            let lang = app.get_lang();
            app.set_place(rank.into());
            app.set_place_name(db::hero_rank_label(rank, &lang).into());
            app.set_area("".into());
            app.set_area_name("".into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "hero");
            return;
        }
        if let Some(mask) = key.strip_prefix("land:") {
            let lang = app.get_lang();
            app.set_place(mask.into());
            app.set_place_name(db::hero_land_label(mask, &lang).into());
            app.set_area("".into());
            app.set_area_name("".into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "hero");
            return;
        }
        if let Some(id) = key.strip_prefix("realm:") {
            let lang = app.get_lang();
            app.set_category("atlas".into());
            app.set_place(id.into());
            app.set_place_name(open_catalog.realm_name(id, &lang).into());
            app.set_area("".into());
            app.set_area_name("".into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "atlas");
            return;
        }
        if let Some(code) = key.strip_prefix("town:") {
            let lang = app.get_lang().to_string();
            let region = open_catalog.town_region(code);
            app.set_category("atlas".into());
            if !region.is_empty() {
                app.set_place(region.clone().into());
                app.set_place_name(open_catalog.realm_name(&region, &lang).into());
            }
            app.set_area(code.into());
            app.set_area_name(open_catalog.town_name(code, &lang).into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "atlas");
            return;
        }
        if let Some(code) = key.strip_prefix("yard:") {
            let lang = app.get_lang();
            app.set_category("works".into());
            app.set_place(code.into());
            app.set_place_name(open_catalog.work_building_name(code, &lang).into());
            app.set_area("".into());
            app.set_area_name("".into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "works");
            return;
        }
        if let Some(code) = key.strip_prefix("bay:") {
            let lang = app.get_lang();
            app.set_category("works".into());
            let parent = open_catalog.work_parent(code);
            if !parent.is_empty() {
                app.set_place(parent.clone().into());
                app.set_place_name(open_catalog.work_building_name(&parent, &lang).into());
            } else {
                app.set_place("".into());
                app.set_place_name("".into());
            }
            app.set_area(code.into());
            app.set_area_name(open_catalog.work_facility_name(code, &lang).into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "works");
            return;
        }
        if let Some(rank) = key.strip_prefix("band:") {
            let lang = app.get_lang();
            app.set_area(rank.into());
            app.set_area_name(db::hero_tier_label(rank, &lang).into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "hero");
            return;
        }
        if let Some(source) = key.strip_prefix("qsrc:") {
            let lang = app.get_lang();
            app.set_place(source.into());
            app.set_place_name(db::source_label(source, &lang).into());
            app.set_area("".into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "quest");
            return;
        }
        if let Some(id) = key.strip_prefix("region:") {
            let lang = app.get_lang();
            app.set_place(id.into());
            app.set_place_name(open_catalog.region_name(id, &lang).into());
            open_scope(&app, &open_catalog, &open_session, &open_favs.borrow(), &open_current, &open_history, "monster");
            return;
        }
        let owned = key.to_string();
        let kind = owned.split_once(':').map(|(name, _)| name).unwrap_or("");
        if !matches!(kind, "item" | "npc" | "monster" | "recipe" | "quest" | "hero" | "set" | "patch" | "chart") {
            return;
        }
        let previous = open_current.borrow().clone();
        if app.get_viewing().is_empty() || previous.is_empty() {
            open_history.borrow_mut().clear();
        } else if previous != owned {
            let mut stack = open_history.borrow_mut();
            stack.push(previous);
            if stack.len() > 16 {
                stack.remove(0);
            }
        }
        *open_current.borrow_mut() = owned.clone();
        app.set_viewing(owned.clone().into());
        show_key(&app, &open_catalog, &open_session, &owned, false);
        sync_view_star(&app, &open_favs.borrow());
    });

    let saved_open_app = app.as_weak();
    let boot_catalog = Rc::clone(&saved_open_catalog);
    let boot_session = saved_open_session.clone();
    app.on_home_open(move |key| {
        let Some(app) = saved_open_app.upgrade() else { return };
        let key = key.to_string();
        if !key.starts_with("monster:") {
            return;
        }
        app.set_category("monster".into());
        app.set_nation("".into());
        app.set_nation_name("".into());
        app.set_area("".into());
        app.set_area_name("".into());
        app.set_place("".into());
        app.set_place_name("".into());
        app.set_query("".into());
        app.set_favourites_only(false);
        app.set_expanded("".into());
        app.set_detail_key("".into());
        app.set_detail_index(-1);
        publish(&app, &saved_open_catalog, &saved_open_session, &saved_open_favs.borrow(), "monster", "");
        *saved_open_current.borrow_mut() = key.clone();
        saved_open_history.borrow_mut().clear();
        app.set_viewing(key.clone().into());
        show_key(&app, &saved_open_catalog, &saved_open_session, &key, false);
        sync_view_star(&app, &saved_open_favs.borrow());
    });

    let fav_app = app.as_weak();
    let favs = Rc::clone(&favourites);
    let fav_list = Rc::clone(&board_list);
    app.on_toggle_favourite(move |key| {
        let Some(app) = fav_app.upgrade() else { return };
        let Some(code) = key.strip_prefix("monster:") else { return };
        {
            let mut set = favs.borrow_mut();
            if !set.remove(code) {
                set.insert(code.to_string());
            }
        }
        db::save_favourites(&favs.borrow());
        sync_view_star(&app, &favs.borrow());
        let starred = favs.borrow().contains(code);
        if app.get_full_mode() && app.get_category().is_empty() {
            show_home_saved(&app, &fav_catalog, &fav_session, &favs.borrow());
            return;
        }
        if app.get_full_mode() && !app.get_expanded().is_empty() {
            if app.get_expanded().as_str() == "saved:" {
                reload_board(&app, &fav_catalog, &fav_session, &favs.borrow(), &fav_list);
            } else {
                list::set_row_favourite(&app, key.as_str(), starred);
            }
            return;
        }
        app.set_stick_cursor(true);
        publish(&app, &fav_catalog, &fav_session, &favs.borrow(), &app.get_category(), &app.get_query());
        let index = row_index(&app, key.as_str());
        if app.get_viewing().is_empty() {
            app.set_cursor(index);
            app.set_list_offset(0.0);
        } else {
            app.set_saved_cursor(index);
        }
    });

    let copy_app = app.as_weak();
    app.on_copy_note(move || {
        let Some(app) = copy_app.upgrade() else { return };
        copy_to_clipboard(&app.get_detail());
        app.set_copied(true);
        let later = copy_app.clone();
        Timer::single_shot(Duration::from_millis(1200), move || {
            if let Some(app) = later.upgrade() {
                app.set_copied(false);
            }
        });
    });

    let (hot_tx, hot_rx) = mpsc::channel();
    hotkey::spawn(hot_tx, app.get_hotkeys_on() && !app.get_full_mode());
    let hot_app = app.as_weak();
    let hot_timer = Timer::default();
    hot_timer.start(TimerMode::Repeated, Duration::from_millis(40), move || {
        let Some(app) = hot_app.upgrade() else { return };
        while let Ok(slot) = hot_rx.try_recv() {
            if !app.get_full_mode() && app.get_hotkeys_on() {
                app.set_hotkey_nth(slot);
            }
        }
    });
    let placed = app.as_weak();
    let placed_hold = Rc::clone(&size_hold);
    Timer::single_shot(Duration::from_millis(40), move || {
        let Some(app) = placed.upgrade() else { return };
        let window = app.window();
        if let Some(pref) = startup_window {
            placed_size.set((pref.width, pref.height));
            if pref.full {
                let (width, height) = if pref.has_full_size {
                    (pref.full_width, pref.full_height)
                } else {
                    full_window_size(window.scale_factor())
                };
                let at = pref.has_full_pos.then_some((pref.full_x, pref.full_y));
                place_fitted(window, width, height, at);
            } else {
                place_window(window, pref.width, pref.height, Some((pref.x, pref.y)));
            }
        } else {
            let (width, height) = default_main_size(window.scale_factor());
            placed_size.set((width, height));
            place_window(window, width, height, None);
        }
        // Native resize events arrive after the placement request. Keep the
        // watcher from saving the initial widget size over the restored mode.
        placed_hold.set(Instant::now() + Duration::from_millis(500));
        startup_placed.set(true);
        focus_window();
    });
    let anim_app = app.as_weak();
    let anim_tick = Rc::new(Cell::new(0i32));
    let anim_clip = Rc::new(RefCell::new(AnimClip {
        code: String::new(),
        path: std::path::PathBuf::new(),
        frames: Vec::new(),
    }));
    let anim_timer = Timer::default();
    anim_timer.start(TimerMode::Repeated, Duration::from_millis(140), move || {
        let Some(app) = anim_app.upgrade() else { return };
        if !app.get_battle_view() || !app.get_animate_battle() {
            return;
        }
        let code = app
            .get_viewing()
            .strip_prefix("monster:")
            .map(str::to_string)
            .or_else(|| app.get_detail_key().strip_prefix("monster:").map(str::to_string));
        let Some(code) = code else { return };
        let mut clip = anim_clip.borrow_mut();
        if clip.code != code {
            clip.code = code.clone();
            clip.frames.clear();
            anim_tick.set(0);
            if let Some((path, _)) = anim_catalog.battle_sprite(&code) {
                if let Ok(frames) = agf::playable_frames(&path) {
                    clip.path = path;
                    clip.frames = frames;
                }
            }
        }
        if clip.frames.len() < 2 {
            return;
        }
        let tick = anim_tick.get().wrapping_add(1);
        anim_tick.set(tick);
        let frame = clip.frames[(tick as usize) % clip.frames.len()];
        let path = clip.path.clone();
        drop(clip);
        let Ok(icon) = agf::frame_rgba(&path, frame) else { return };
        let image = image_from_icon(icon);
        app.set_battle_icon(image.clone());
        show_battle_frame(&app, &format!("monster:{code}"), image);
    });
    if app.get_full_mode() && app.get_category().is_empty() {
        show_home_saved(&app, &boot_catalog, &boot_session, &favourites.borrow());
    }
    app.run().expect("視窗執行失敗");
}

#[cfg(test)]
mod tests {
    use super::{home_array, park_disabled, shift_order, shown_order};

    #[test]
    fn disabled_dock_buttons_move_to_the_end() {
        let mut order = vec![0, 1, 2, 3, 4, 5, 6, 7, 8];
        let mut on = [true; 9];
        on[2] = false;
        on[8] = false;
        park_disabled(&mut order, &on);
        assert_eq!(order, vec![0, 1, 3, 4, 5, 6, 7, 2, 8]);
        assert_eq!(shown_order(&order, &on), vec![0, 1, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn shift_order_moves_one_slot_and_ignores_a_bad_index() {
        let mut order = vec![0, 1, 2, 3];
        shift_order(&mut order, 0, 2);
        assert_eq!(order, vec![1, 2, 0, 3]);
        shift_order(&mut order, 9, 0);
        assert_eq!(order, vec![1, 2, 0, 3]);
    }

    #[test]
    fn home_array_keeps_only_a_full_order() {
        assert_eq!(home_array(&[8, 7, 6, 5, 4, 3, 2, 1, 0]), [8, 7, 6, 5, 4, 3, 2, 1, 0]);
        assert_eq!(home_array(&[1, 2]), [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn battle_frames_skip_blanks_and_reuse_the_decoded_frame() {
        let catalog = crate::db::Catalog::open().expect("db");
        let (path, _) = catalog.battle_sprite("M96").expect("sprite");
        let frames = crate::agf::playable_frames(&path).expect("frames");
        assert!(frames.len() >= 2, "sprite has {} playable frames", frames.len());
        assert!(frames.windows(2).all(|pair| pair[0] < pair[1]));
        let first = crate::agf::frame_rgba(&path, frames[0]).expect("frame");
        let again = crate::agf::frame_rgba(&path, frames[0]).expect("cached frame");
        assert!(first.width > 0 && first.height > 0);
        assert_eq!(first.rgba.len(), first.width as usize * first.height as usize * 4);
        assert_eq!(first.rgba, again.rgba);
    }
}
