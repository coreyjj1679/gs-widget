#![windows_subsystem = "windows"]

mod agf;
mod db;
mod hotkey;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel};

slint::include_modules!();

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

fn main() {
    let catalog = match db::Catalog::open() {
        Ok(catalog) => Rc::new(catalog),
        Err(message) => {
            eprintln!("{message}");
            let app = App::new().expect("視窗建立失敗");
            app.set_detail(message.into());
            app.run().expect("視窗執行失敗");
            return;
        }
    };

    let app = App::new().expect("視窗建立失敗");
    app.set_lang(db::load_lang().into());
    app.set_window_opacity(db::load_opacity());
    app.set_show_set_extra(db::load_set_extra());
    app.set_client_version(db::client_label().into());
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
        publish(&app, &search_catalog, &search_session, &search_favs.borrow(), &category, &text);
        app.set_detail("".into());
        app.set_viewing("".into());
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
    });

    let home_app = app.as_weak();
    let home_current = Rc::clone(&current);
    let home_history = Rc::clone(&history);
    let home_session = session.clone();
    app.on_went_home(move || {
        let Some(app) = home_app.upgrade() else { return };
        home_session.generation.fetch_add(1, Ordering::Relaxed);
        *home_current.borrow_mut() = String::new();
        home_history.borrow_mut().clear();
        app.set_hits(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
        app.set_links(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
    });

    let saved_size = Rc::new(Cell::new(db::load_window().map(|pref| (pref.width, pref.height)).unwrap_or((560.0, 740.0))));
    let placed_size = Rc::clone(&saved_size);
    let dock_size = Rc::new(Cell::new((dock.width, dock.height)));
    let dock_vertical = Rc::new(Cell::new(dock.vertical));
    let dock_park = Rc::new(Cell::new((0i32, 0i32)));
    let expand_at = Rc::new(Cell::new(None::<(i32, i32)>));
    let size_hold = Rc::new(Cell::new(Instant::now()));
    let shown_at = Instant::now();
    let collapse_app = app.as_weak();
    let dock_while_open = Rc::clone(&dock_size);
    let hold_on_switch = Rc::clone(&size_hold);
    let watch_saved = Rc::clone(&saved_size);
    app.on_collapse_changed(move |collapsed| {
        if collapsed && shown_at.elapsed() < Duration::from_millis(800) {
            return;
        }
        let Some(app) = collapse_app.upgrade() else { return };
        let (width, height) = window_logical(app.window());
        if collapsed {
            if width > 160.0 && height > 160.0 {
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
                let (width, height) = saved_size.get();
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
    let size_watch = Timer::default();
    size_watch.start(TimerMode::Repeated, Duration::from_millis(80), move || {
        if Instant::now() < watch_hold.get() {
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
        } else if width > 280.0 && height > 360.0 {
            let previous = watch_saved.get();
            watch_saved.set((width, height));
            if (previous.0 - width).abs() > 1.0 || (previous.1 - height).abs() > 1.0 {
                remember_window(app.window());
            }
        }
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
            if !app.get_collapsed() {
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
        }
        show_alias_rows(&app, &lang_catalog, &lang_aliases.borrow());
    });

    let open_app = app.as_weak();
    let open_catalog = catalog;
    let open_current = current;
    let open_session = session;
    let open_favs = Rc::clone(&favourites);
    let open_history = Rc::clone(&history);
    let fav_catalog = Rc::clone(&open_catalog);
    let fav_session = open_session.clone();
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
        let viewing = app.get_viewing().to_string();
        if !viewing.starts_with("item:") && !viewing.starts_with("set:") && !viewing.starts_with("hero:") {
            return;
        }
        let key = key.to_string();
        if app.get_item_source() == key {
            app.set_item_source("".into());
        } else {
            app.set_item_source(key.clone().into());
        }
        show_key(&app, &source_catalog, &source_session, &viewing, true);
        focus_section(&app, &key);
    });
    app.on_open_hit(move |key| {
        let Some(app) = open_app.upgrade() else { return };
        if key.is_empty() || key.starts_with("section:") {
            return;
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

    let fav_app = app.as_weak();
    let favs = Rc::clone(&favourites);
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
    hotkey::spawn(hot_tx);
    let hot_app = app.as_weak();
    let hot_timer = Timer::default();
    hot_timer.start(TimerMode::Repeated, Duration::from_millis(40), move || {
        let Some(app) = hot_app.upgrade() else { return };
        while let Ok(slot) = hot_rx.try_recv() {
            app.set_hotkey_nth(slot);
        }
    });
    let placed = app.as_weak();
    Timer::single_shot(Duration::from_millis(40), move || {
        let Some(app) = placed.upgrade() else { return };
        let window = app.window();
        if let Some(pref) = db::load_window() {
            placed_size.set((pref.width, pref.height));
            place_window(window, pref.width, pref.height, Some((pref.x, pref.y)));
        } else {
            let (width, height) = default_main_size(window.scale_factor());
            placed_size.set((width, height));
            place_window(window, width, height, None);
        }
        focus_window();
    });
    app.run().expect("視窗執行失敗");
}

fn place_window(window: &slint::Window, width: f32, height: f32, at: Option<(i32, i32)>) {
    let scale = window.scale_factor().max(1.0);
    let (screen_w, screen_h) = primary_screen();
    let pixel_w = (width * scale).round() as i32;
    let pixel_h = (height * scale).round() as i32;
    let (x, y) = match at {
        Some((x, y)) if x > -80 && y > -40 && x + 120 < screen_w && y + 80 < screen_h => (x, y),
        _ => ((screen_w - pixel_w).max(0) / 2, (screen_h - pixel_h).max(0) / 2),
    };
    window.set_size(slint::LogicalSize::new(width, height));
    window.set_position(slint::PhysicalPosition::new(x, y));
}

fn default_main_size(scale: f32) -> (f32, f32) {
    let scale = scale.max(1.0);
    let (screen_w, screen_h) = primary_screen();
    let logical_w = screen_w as f32 / scale;
    let logical_h = screen_h as f32 / scale;
    let width = (logical_w * 0.34).clamp(480.0, 780.0).min((logical_w - 48.0).max(420.0));
    let height = (logical_h * 0.74).clamp(640.0, 1000.0).min((logical_h - 64.0).max(560.0));
    (width, height)
}

fn default_icon_size() -> i32 {
    1
}

fn remember_window(window: &slint::Window) {
    let (width, height) = window_logical(window);
    if width < 280.0 || height < 360.0 {
        return;
    }
    let pos = window.position();
    db::save_window(&db::WindowPref {
        width,
        height,
        x: pos.x,
        y: pos.y,
    });
}

fn focus_window() {
    let title: Vec<u16> = "巨商\0".encode_utf16().collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd != 0 {
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

fn primary_screen() -> (i32, i32) {
    unsafe { (GetSystemMetrics(0), GetSystemMetrics(1)) }
}

/// Shift an expanded window back inside `work` (left, top, right, bottom).
/// A dock parked on the right or bottom keeps that edge and grows inward.
fn fit_on_screen(
    x: i32,
    y: i32,
    old_w: i32,
    old_h: i32,
    new_w: i32,
    new_h: i32,
    work: (i32, i32, i32, i32),
) -> (i32, i32) {
    let (left, top, right, bottom) = work;
    let old_w = old_w.max(1);
    let old_h = old_h.max(1);
    let new_w = new_w.max(1);
    let new_h = new_h.max(1);
    let mut nx = x;
    let mut ny = y;
    if nx + new_w > right {
        nx = x + old_w - new_w;
    }
    if ny + new_h > bottom {
        ny = y + old_h - new_h;
    }
    let max_x = (right - new_w).max(left);
    let max_y = (bottom - new_h).max(top);
    (nx.clamp(left, max_x), ny.clamp(top, max_y))
}

fn work_area_at(x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
    let point = WinPoint {
        x: x.saturating_add(w / 2),
        y: y.saturating_add(h / 2),
    };
    unsafe {
        let monitor = MonitorFromPoint(point, 2);
        let mut info = MonitorInfo {
            cb_size: std::mem::size_of::<MonitorInfo>() as u32,
            rc_monitor: WinRect::default(),
            rc_work: WinRect::default(),
            dw_flags: 0,
        };
        if monitor != 0 && GetMonitorInfoW(monitor, &mut info) != 0 {
            let area = info.rc_work;
            if area.right > area.left && area.bottom > area.top {
                return (area.left, area.top, area.right, area.bottom);
            }
        }
    }
    let (width, height) = primary_screen();
    (0, 0, width.max(1), height.max(1))
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WinPoint {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct MonitorInfo {
    cb_size: u32,
    rc_monitor: WinRect,
    rc_work: WinRect,
    dw_flags: u32,
}

#[link(name = "user32")]
extern "system" {
    fn GetSystemMetrics(index: i32) -> i32;
    fn MonitorFromPoint(point: WinPoint, flags: u32) -> isize;
    fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
    fn FindWindowW(class: *const u16, window: *const u16) -> isize;
    fn SetForegroundWindow(hwnd: isize) -> i32;
    fn OpenClipboard(hwnd: isize) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, mem: isize) -> isize;
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalLock(mem: isize) -> *mut u16;
    fn GlobalUnlock(mem: isize) -> i32;
    fn GlobalFree(mem: isize) -> isize;
}

fn copy_to_clipboard(text: &str) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let bytes = wide.len() * std::mem::size_of::<u16>();
    unsafe {
        if OpenClipboard(0) == 0 {
            return;
        }
        EmptyClipboard();
        let mem = GlobalAlloc(0x0002, bytes);
        if mem != 0 {
            let ptr = GlobalLock(mem);
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                GlobalUnlock(mem);
                if SetClipboardData(13, mem) == 0 {
                    GlobalFree(mem);
                }
            } else {
                GlobalFree(mem);
            }
        }
        CloseClipboard();
    }
}

fn open_scope(
    app: &App,
    catalog: &db::Catalog,
    session: &Session,
    favourites: &HashSet<String>,
    current: &RefCell<String>,
    history: &RefCell<Vec<String>>,
    category: &str,
) {
    *current.borrow_mut() = String::new();
    history.borrow_mut().clear();
    app.set_viewing("".into());
    app.set_viewing_monster(false);
    app.set_viewing_recipe(false);
    app.set_viewing_quest(false);
    app.set_viewing_npc(false);
    app.set_viewing_hero(false);
    app.set_viewing_item(false);
    app.set_viewing_set(false);
    app.set_item_tab("".into());
    app.set_viewing_favourite(false);
    app.set_query("".into());
    app.set_detail("".into());
    app.set_links(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
    app.set_show_icon(false);
    app.set_show_element(false);
    publish(app, catalog, session, favourites, category, "");
}

fn alias_code_list(codes: &str) -> Vec<String> {
    codes
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn alias_suggestions(catalog: &db::Catalog, query: &str, lang: &str, skip: &[String]) -> Vec<Hit> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    catalog
        .search("item", query, lang, "", "")
        .into_iter()
        .filter(|hit| hit.key.starts_with("item:"))
        .filter(|hit| {
            let code = hit.key.trim_start_matches("item:");
            !skip.iter().any(|item| item == code)
        })
        .take(8)
        .map(|hit| {
            let code = hit.key.trim_start_matches("item:");
            let (image, has_icon) = alias_image(&hit.icon);
            Hit {
                title: hit.title.into(),
                subtitle: format!("#{code}").into(),
                key: hit.key.into(),
                icon: image,
                has_icon,
                no_image: !has_icon,
                can_favourite: false,
                favourite: false,
                solo: false,
                has_badge: false,
                badge: Image::default(),
                aside: String::new().into(),
                chip: String::new().into(),
            }
        })
        .collect()
}

fn refresh_alias_icons(app: &App, catalog: &db::Catalog, rows: &[(String, String)]) {
    let lang = app.get_lang().to_string();
    let mut icons = Vec::new();
    for (index, (_, codes)) in rows.iter().enumerate() {
        for item in catalog.alias_items(codes, &lang) {
            let (image, has_icon) = alias_image(&item.icon);
            icons.push(Hit {
                title: item.title.clone().into(),
                subtitle: item.subtitle.clone().into(),
                key: item.key.clone().into(),
                icon: image,
                has_icon,
                no_image: !has_icon,
                can_favourite: false,
                favourite: false,
                solo: false,
                has_badge: false,
                badge: Image::default(),
                aside: index.to_string().into(),
                chip: String::new().into(),
            });
        }
    }
    app.set_alias_icons(ModelRc::new(VecModel::from(icons)));
}

fn alias_image(icon: &Option<(std::path::PathBuf, i32)>) -> (Image, bool) {
    if let Some((path, frame)) = icon {
        if let Ok(decoded) = agf::frame_rgba(path, *frame) {
            return (image_from_icon(decoded), true);
        }
    }
    (Image::default(), false)
}

fn show_alias_rows(app: &App, catalog: &db::Catalog, rows: &[(String, String)]) {
    let lang = app.get_lang().to_string();
    let mut icons = Vec::new();
    let shown: Vec<AliasRow> = rows
        .iter()
        .enumerate()
        .map(|(index, (name, codes))| {
            let items = catalog.alias_items(codes, &lang);
            for item in &items {
                let (image, has_icon) = alias_image(&item.icon);
                icons.push(Hit {
                    title: item.title.clone().into(),
                    subtitle: item.subtitle.clone().into(),
                    key: item.key.clone().into(),
                    icon: image,
                    has_icon,
                    no_image: !has_icon,
                    can_favourite: false,
                    favourite: false,
                    solo: false,
                    has_badge: false,
                    badge: Image::default(),
                    aside: index.to_string().into(),
                    chip: String::new().into(),
                });
            }
            AliasRow {
                name: name.into(),
                codes: codes.into(),
                detail: catalog.alias_names(codes, &lang).into(),
                count: items.len() as i32,
            }
        })
        .collect();
    app.set_alias_rows(ModelRc::new(VecModel::from(shown)));
    app.set_alias_icons(ModelRc::new(VecModel::from(icons)));
}

fn alias_text(rows: &[(String, String)]) -> String {
    rows.iter()
        .filter_map(|(name, codes)| {
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            let numbers: Vec<&str> = codes
                .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
                .map(str::trim)
                .filter(|code| !code.is_empty())
                .collect();
            if numbers.is_empty() {
                return None;
            }
            Some(format!("{name} {}", numbers.join(" ")))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn publish(app: &App, catalog: &db::Catalog, session: &Session, favourites: &HashSet<String>, category: &str, text: &str) {
    let lang = app.get_lang().to_string();
    let nation = app.get_nation().to_string();
    let at_root = app.get_place().is_empty() && app.get_area().is_empty() && nation.is_empty();
    let hits = if category == "monster" && app.get_favourites_only() && at_root {
        catalog.search("monster-all", text, &lang, "", "")
    } else {
        catalog.search_scoped(
            category,
            text,
            &lang,
            &app.get_place(),
            &app.get_nation(),
            &if category == "hero" { app.get_hero_group().to_string() } else { app.get_ship().to_string() },
            &app.get_area(),
        )
    };
    let only = category == "monster" && app.get_favourites_only();
    let hits = prioritize(catalog, &nation, hits, favourites, only);
    show_list(app, session, catalog, &nation, hits, favourites);
}

fn saved_row(catalog: &db::Catalog, nation: &str, key: &str, favourites: &HashSet<String>) -> bool {
    match key.strip_prefix("monster:") {
        Some(code) => favourites.contains(code),
        None => catalog.scope_saved(key, nation, favourites),
    }
}

fn prioritize(catalog: &db::Catalog, nation: &str, hits: Vec<db::Hit>, favourites: &HashSet<String>, only: bool) -> Vec<db::Hit> {
    let mut starred = Vec::new();
    let mut rest = Vec::new();
    for hit in hits {
        if saved_row(catalog, nation, &hit.key, favourites) {
            starred.push(hit);
        } else if !only {
            rest.push(hit);
        }
    }
    if only {
        starred.truncate(300);
    }
    starred.append(&mut rest);
    starred
}

fn sync_view_star(app: &App, favourites: &HashSet<String>) {
    let viewing = app.get_viewing().to_string();
    match viewing.strip_prefix("monster:") {
        Some(code) => {
            app.set_viewing_monster(true);
            app.set_viewing_favourite(favourites.contains(code));
        }
        None => {
            app.set_viewing_monster(false);
            app.set_viewing_favourite(false);
        }
    }
}

fn row_index(app: &App, key: &str) -> i32 {
    let hits = app.get_hits();
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return 0 };
    for index in 0..model.row_count() {
        if model.row_data(index).is_some_and(|row| row.key.as_str() == key) {
            return index as i32;
        }
    }
    0
}

fn picture_key(key: &str) -> bool {
    key.starts_with("item:")
        || key.starts_with("set:")
        || key.starts_with("recipe:")
        || key.starts_with("npc:")
        || key.starts_with("hero:")
}

fn show_list(app: &App, session: &Session, catalog: &db::Catalog, nation: &str, hits: Vec<db::Hit>, favourites: &HashSet<String>) {
    let gen = session.generation.fetch_add(1, Ordering::Relaxed) + 1;
    let mut rows = Vec::with_capacity(hits.len());
    let mut jobs = Vec::new();
    for hit in hits {
        let mut icon = Image::default();
        let mut has_icon = false;
        let no_image = hit.icon.is_none() && picture_key(&hit.key);
        if let Some((path, frame)) = hit.icon {
            has_icon = true;
            let cache_key = format!("{}#{frame}", path.display());
            if let Some(image) = session.cache.borrow().get(&cache_key) {
                icon = image.clone();
            } else {
                jobs.push(IconJob {
                    gen,
                    links: false,
                    cache_key,
                    row_key: hit.key.clone(),
                    path,
                    frame,
                });
            }
        }
        let can_favourite = hit.key.starts_with("monster:");
        let favourite = saved_row(catalog, nation, &hit.key, favourites);
        let (badge, has_badge) = match hit.badge {
            Some((path, frame)) => match cached_icon(&session.cache, &path, frame) {
                Some(image) => (image, true),
                None => (Image::default(), false),
            },
            None => (Image::default(), false),
        };
        rows.push(Hit {
            title: SharedString::from(hit.title),
            subtitle: SharedString::from(hit.subtitle),
            key: SharedString::from(hit.key),
            icon,
            has_icon,
            no_image,
            can_favourite,
            favourite,
            solo: hit.solo,
            has_badge,
            badge,
            aside: SharedString::from(hit.aside),
            chip: SharedString::from(hit.chip),
        });
    }
    app.set_hits(ModelRc::new(VecModel::from(rows)));
    for job in jobs {
        let _ = session.jobs.send(job);
    }
}

fn spawn_icon_worker(
    app: slint::Weak<App>,
    rx: Receiver<IconJob>,
    generation: Arc<AtomicU64>,
    link_generation: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let current = if job.links { &link_generation } else { &generation };
            if job.gen != current.load(Ordering::Relaxed) {
                continue;
            }
            let app = app.clone();
            let row_key = job.row_key;
            let links = job.links;
            match agf::frame_rgba(&job.path, job.frame) {
                Ok(icon) => {
                    let cache_key = job.cache_key;
                    let _ = app.upgrade_in_event_loop(move |app| {
                        let image = image_from_icon(icon);
                        remember_icon(&cache_key, image.clone());
                        paint_hit(&app, &row_key, image, true, links);
                    });
                }
                Err(_) => {
                    let _ = app.upgrade_in_event_loop(move |app| {
                        paint_hit(&app, &row_key, Image::default(), false, links);
                    });
                }
            }
        }
    });
}

fn remember_icon(cache_key: &str, image: Image) {
    ICON_CACHE.with(|slot| {
        if let Some(cache) = slot.borrow().as_ref() {
            cache.borrow_mut().insert(cache_key.to_string(), image);
        }
    });
}

fn paint_hit(app: &App, row_key: &str, icon: Image, has_icon: bool, links: bool) {
    let hits = if links { app.get_links() } else { app.get_hits() };
    if paint_model(&hits, row_key, icon.clone(), has_icon) {
        return;
    }
    if links {
        let related = app.get_related();
        paint_model(&related, row_key, icon, has_icon);
    }
}

fn paint_model(hits: &ModelRc<Hit>, row_key: &str, icon: Image, has_icon: bool) -> bool {
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return false };
    for index in 0..model.row_count() {
        let Some(mut row) = model.row_data(index) else { continue };
        if row.key.as_str() != row_key {
            continue;
        }
        row.icon = icon;
        row.has_icon = has_icon;
        if !has_icon && picture_key(&row.key) {
            row.no_image = true;
        }
        model.set_row_data(index, row);
        return true;
    }
    false
}

fn image_from_icon(icon: agf::Icon) -> Image {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(icon.width, icon.height);
    let pixels = buffer.make_mut_slice();
    for (index, chunk) in icon.rgba.chunks_exact(4).enumerate() {
        if let Some(pixel) = pixels.get_mut(index) {
            *pixel = Rgba8Pixel {
                r: chunk[0],
                g: chunk[1],
                b: chunk[2],
                a: chunk[3],
            };
        }
    }
    Image::from_rgba8(buffer)
}

fn note_lines(note: &str) -> i32 {
    if note.is_empty() {
        return 0;
    }
    let width = 28;
    note.chars().count().div_ceil(width).clamp(1, 8) as i32
}

fn publish_item_sources(app: &App, rows: &[Hit], keep_sheet: bool) {
    let mut groups: Vec<(String, String, Vec<Hit>)> = Vec::new();
    let mut current: Option<(String, String, Vec<Hit>)> = None;
    for row in rows {
        if row.chip.as_str() == "section" {
            if let Some(group) = current.take() {
                groups.push(group);
            }
            let id = row.key.strip_prefix("section:").unwrap_or(row.key.as_str()).to_string();
            current = Some((id, String::new(), Vec::new()));
        } else if let Some((_, note, items)) = current.as_mut() {
            if row.chip.as_str() == "note" {
                if !note.is_empty() {
                    note.push('\n');
                }
                note.push_str(row.title.as_str());
            } else {
                items.push(row.clone());
            }
        }
    }
    if let Some(group) = current {
        groups.push(group);
    }
    let preferred = if keep_sheet { app.get_item_source().to_string() } else { String::new() };
    let pick = if keep_sheet && (preferred.is_empty() || groups.iter().any(|(id, _, _)| id == &preferred)) {
        preferred
    } else if groups.iter().any(|(id, _, _)| id == "make") {
        "make".to_string()
    } else {
        groups.first().map(|(id, _, _)| id.clone()).unwrap_or_default()
    };
    let lang = app.get_lang().to_string();
    let mut shown = Vec::new();
    let mut source_note = String::new();
    for (id, note, items) in &groups {
        let methods = items.iter().filter(|item| item.key.starts_with("bay:")).count();
        let count = if id == "make" {
            if methods > 0 { methods } else if items.is_empty() { 0 } else { 1 }
        } else {
            items.len()
        };
        let title = if count == 0 {
            db::relation_label(id, &lang)
        } else {
            format!("{}  {}", db::relation_label(id, &lang), count)
        };
        shown.push(Hit {
            title: title.into(),
            subtitle: String::new().into(),
            key: id.into(),
            icon: Image::default(),
            has_icon: false,
            no_image: false,
            can_favourite: false,
            favourite: false,
            solo: false,
            has_badge: false,
            badge: Image::default(),
            aside: if id == &pick { "▼".into() } else { "▶".into() },
            chip: "section".into(),
        });
        if id == &pick {
            shown.extend(items.iter().cloned());
            source_note = note.clone();
        }
    }
    app.set_item_source(pick.into());
    app.set_source_note(source_note.clone().into());
    app.set_source_note_lines(note_lines(&source_note));
    app.set_sources(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
    app.set_links(ModelRc::new(VecModel::from(shown)));
    if !keep_sheet {
        app.set_cursor(0);
    }
    app.set_drop_offset(0.0);
}

fn publish_hero_sections(app: &App, rows: Vec<Hit>, keep_sheet: bool) {
    let mut prefix = Vec::new();
    let mut groups: Vec<(Hit, Vec<Hit>)> = Vec::new();
    let mut current: Option<(Hit, Vec<Hit>)> = None;
    for row in rows {
        if row.chip.as_str() == "section" {
            if let Some(group) = current.take() {
                groups.push(group);
            }
            current = Some((row, Vec::new()));
        } else if let Some((_, items)) = current.as_mut() {
            items.push(row);
        } else {
            prefix.push(row);
        }
    }
    if let Some(group) = current {
        groups.push(group);
    }
    let preferred = if keep_sheet { app.get_item_source().to_string() } else { String::new() };
    let pick = if groups.iter().any(|(header, _)| header.key.as_str() == preferred) {
        preferred
    } else if groups.iter().any(|(header, _)| header.key.as_str() == "need") {
        "need".to_string()
    } else {
        groups.first().map(|(header, _)| header.key.to_string()).unwrap_or_default()
    };
    let mut shown = prefix;
    for (header, items) in groups {
        let open = header.key.as_str() == pick;
        shown.push(Hit {
            title: header.title,
            subtitle: header.subtitle,
            key: header.key,
            icon: Image::default(),
            has_icon: false,
            no_image: false,
            can_favourite: false,
            favourite: false,
            solo: false,
            has_badge: false,
            badge: Image::default(),
            aside: if open { "▼".into() } else { "▶".into() },
            chip: "section".into(),
        });
        if open {
            shown.extend(items);
        }
    }
    app.set_item_source(pick.into());
    app.set_source_note("".into());
    app.set_source_note_lines(0);
    app.set_sources(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
    app.set_links(ModelRc::new(VecModel::from(shown)));
    if !keep_sheet {
        app.set_cursor(0);
    }
    app.set_drop_offset(0.0);
}

fn focus_section(app: &App, key: &str) {
    let links = app.get_links();
    let Some(model) = links.as_any().downcast_ref::<VecModel<Hit>>() else { return };
    for index in 0..model.row_count() {
        if model.row_data(index).is_some_and(|row| row.chip.as_str() == "section" && row.key.as_str() == key) {
            app.set_cursor(index as i32);
            return;
        }
    }
}

fn show_key(app: &App, catalog: &db::Catalog, session: &Session, key: &str, keep_sheet: bool) {
    if !keep_sheet {
        app.set_item_tab("".into());
        app.set_item_source("".into());
    }
    app.set_has_item_fx(false);
    app.set_has_set_fx(false);
    app.set_has_total_fx(false);
    let cache = &session.cache;
    let (detail, links, icon) = catalog.open_key(key, &app.get_lang());
    app.set_detail(detail.into());
    let gen = session.link_generation.fetch_add(1, Ordering::Relaxed) + 1;
    let related_hits = key
        .strip_prefix("monster:")
        .map(|code| catalog.monster_quests(code, &app.get_lang()))
        .unwrap_or_default();
    let (rows, mut jobs) = link_rows(cache, links, gen);
    let (related_rows, related_jobs) = link_rows(cache, related_hits, gen);
    jobs.extend(related_jobs);
    app.set_related(ModelRc::new(VecModel::from(related_rows)));
    if key.starts_with("item:") || key.starts_with("set:") {
        publish_item_sources(app, &rows, keep_sheet);
    } else if key.starts_with("hero:") {
        publish_hero_sections(app, rows, keep_sheet);
    } else {
        app.set_sources(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
        app.set_item_source("".into());
        app.set_source_note("".into());
        app.set_source_note_lines(0);
        app.set_links(ModelRc::new(VecModel::from(rows)));
    }
    for job in jobs {
        let _ = session.jobs.send(job);
    }
    let mut shown = false;
    if let Some((path, frame)) = icon {
        if let Ok(icon) = agf::frame_rgba(&path, frame) {
            app.set_item_icon(image_from_icon(icon));
            app.set_show_icon(true);
            shown = true;
        }
    }
    if !shown {
        app.set_show_icon(false);
    }
    app.set_viewing_no_image(picture_key(key) && !shown);
    let mut element_shown = false;
    if let Some(code) = key.strip_prefix("monster:") {
        if let Some((path, frame)) = catalog.element_icon(code) {
            if let Some(image) = cached_icon(cache, &path, frame) {
                app.set_element_icon(image);
                app.set_show_element(true);
                element_shown = true;
            }
        }
    }
    if !element_shown {
        app.set_show_element(false);
    }
    let lang = app.get_lang().to_string();
    app.set_sheet_note("".into());
    app.set_viewing_recipe(false);
    app.set_viewing_quest(false);
    app.set_viewing_npc(false);
    app.set_viewing_hero(false);
    app.set_viewing_item(false);
    app.set_viewing_set(false);
    clear_quest_nav(app);
    app.set_drop_offset(0.0);
    if let Some(code) = key.strip_prefix("monster:") {
        if let Some((name, alt, id, lines)) = catalog.monster_sheet(code, &lang) {
            apply_sheet(&app, name, alt, id, lines);
        }
    } else if let Some(code) = key.strip_prefix("item:") {
        if let Some((name, alt, id, lines, note)) = catalog.item_sheet(code.parse().unwrap_or(0), &lang) {
            apply_sheet(&app, name, alt, id, lines);
            app.set_sheet_note(note.into());
            app.set_viewing_item(true);
        }
    } else if let Some(code) = key.strip_prefix("set:") {
        if let Some((name, alt, id, lines, note)) = catalog.set_sheet(code.parse().unwrap_or(0), &lang) {
            app.set_sheet_title(name.into());
            app.set_sheet_alt(alt.into());
            app.set_sheet_code(id.into());
            app.set_has_item_fx(lines.iter().any(|row| row.2 == "fold:item"));
            app.set_has_set_fx(lines.iter().any(|row| row.2 == "fold:set"));
            app.set_has_total_fx(lines.iter().any(|row| row.2 == "fold:total"));
            app.set_fold_focus(if lines.iter().any(|row| row.2 == "fold:item") {
                0
            } else if lines.iter().any(|row| row.2 == "fold:set") {
                1
            } else {
                2
            });
            let facts: Vec<Fact> = lines
                .into_iter()
                .map(|(label, value, key, section)| {
                    let (icon, has_icon) = catalog
                        .linked_icon(&key)
                        .and_then(|(path, frame)| cached_icon(cache, &path, frame))
                        .map(|image| (image, true))
                        .unwrap_or_else(|| (Image::default(), false));
                    Fact {
                        label: label.into(),
                        value: value.into(),
                        key: key.into(),
                        icon,
                        has_icon,
                        section: section.into(),
                    }
                })
                .collect();
            app.set_facts(ModelRc::new(VecModel::from(facts)));
            app.set_sheet_note(note.into());
            app.set_viewing_item(true);
            app.set_viewing_set(true);
        }
    } else if let Some(id) = key.strip_prefix("recipe:") {
        if let Some((name, alt, code, lines)) = catalog.recipe_sheet(id.parse().unwrap_or(0), &lang) {
            apply_sheet(&app, name, alt, code, lines);
            app.set_viewing_recipe(true);
        } else {
            app.set_sheet_title("".into());
            app.set_sheet_alt("".into());
            app.set_sheet_code("".into());
            app.set_facts(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
        }
    } else if let Some(code) = key.strip_prefix("hero:") {
        if let Some((name, alt, heading, lines, note)) = catalog.hero_sheet(code, &lang) {
            apply_sheet(&app, name, alt, heading, lines);
            app.set_sheet_note(note.into());
            app.set_viewing_hero(true);
        } else {
            app.set_sheet_title("".into());
            app.set_sheet_alt("".into());
            app.set_sheet_code("".into());
            app.set_facts(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
        }
    } else if let Some(code) = key.strip_prefix("npc:") {
        if let Some((name, alt, map, lines, note)) = catalog.npc_sheet(code.parse().unwrap_or(0), &lang) {
            apply_sheet(&app, name, alt, map, lines);
            app.set_sheet_note(note.into());
            app.set_viewing_npc(true);
        } else {
            app.set_sheet_title("".into());
            app.set_sheet_alt("".into());
            app.set_sheet_code("".into());
            app.set_facts(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
        }
    } else if let Some(id) = key.strip_prefix("quest:") {
        let quest_id = id.parse().unwrap_or(0);
        if let Some((name, alt, code, tasks, note)) = catalog.quest_sheet(quest_id, &lang) {
            app.set_sheet_title(name.into());
            app.set_sheet_alt(alt.into());
            app.set_sheet_code(code.into());
            let mut facts = Vec::new();
            let mut picks = Vec::new();
            for (index, (label, value, key)) in tasks.into_iter().enumerate() {
                if !key.is_empty() {
                    picks.push(index as i32);
                }
                let (icon, has_icon) = catalog
                    .linked_icon(&key)
                    .and_then(|(path, frame)| cached_icon(cache, &path, frame))
                    .map(|image| (image, true))
                    .unwrap_or_else(|| (Image::default(), false));
                facts.push(Fact {
                    label: label.into(),
                    value: value.into(),
                    key: key.into(),
                    icon,
                    has_icon,
                    section: String::new().into(),
                });
            }
            app.set_facts(ModelRc::new(VecModel::from(facts)));
            app.set_sheet_note(note.into());
            app.set_viewing_quest(true);
            app.set_task_picks(ModelRc::new(VecModel::from(picks.clone())));
            app.set_task_focus(if picks.is_empty() { -1 } else { 0 });
            let rewards: Vec<Fact> = catalog
                .quest_reward_lines(quest_id, &lang)
                .into_iter()
                .map(|(label, value)| Fact {
                    label: label.into(),
                    value: value.into(),
                    key: String::new().into(),
                    icon: Image::default(),
                    has_icon: false,
                    section: String::new().into(),
                })
                .collect();
            app.set_reward_facts(ModelRc::new(VecModel::from(rewards)));
            let (prev, next) = catalog.quest_chain(quest_id, &lang);
            app.set_chain_prev(ModelRc::new(VecModel::from(plain_hits(prev.into_iter().collect()))));
            app.set_chain_next(ModelRc::new(VecModel::from(plain_hits(next))));
            app.set_quest_npc(catalog.quest_giver(quest_id).into());
        } else {
            app.set_sheet_title("".into());
            app.set_sheet_alt("".into());
            app.set_sheet_code("".into());
            app.set_facts(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
        }
    } else {
        app.set_sheet_title("".into());
        app.set_sheet_alt("".into());
        app.set_sheet_code("".into());
        app.set_sheet_note("".into());
        app.set_facts(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
    }
}

fn apply_sheet(app: &App, name: String, alt: String, id: String, lines: Vec<(String, String)>) {
    app.set_sheet_title(name.into());
    app.set_sheet_alt(alt.into());
    app.set_sheet_code(id.into());
    let facts: Vec<Fact> = lines
        .into_iter()
        .map(|(label, value)| Fact {
            label: label.into(),
            value: value.into(),
            key: String::new().into(),
            icon: Image::default(),
            has_icon: false,
            section: String::new().into(),
        })
        .collect();
    app.set_facts(ModelRc::new(VecModel::from(facts)));
}

fn clear_quest_nav(app: &App) {
    app.set_chain_prev(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
    app.set_chain_next(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
    app.set_reward_facts(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
    app.set_task_focus(-1);
    app.set_task_picks(ModelRc::new(VecModel::from(Vec::<i32>::new())));
    app.set_quest_npc("".into());
}

fn plain_hits(hits: Vec<db::Hit>) -> Vec<Hit> {
    hits.into_iter()
        .map(|hit| Hit {
            title: SharedString::from(hit.title),
            subtitle: SharedString::from(hit.subtitle),
            key: SharedString::from(hit.key),
            icon: Image::default(),
            has_icon: false,
            no_image: false,
            can_favourite: false,
            favourite: false,
            solo: hit.solo,
            has_badge: false,
            badge: Image::default(),
            aside: SharedString::from(hit.aside),
            chip: SharedString::from(hit.chip),
        })
        .collect()
}

fn link_rows(
    cache: &RefCell<HashMap<String, Image>>,
    hits: Vec<db::Hit>,
    gen: u64,
) -> (Vec<Hit>, Vec<IconJob>) {
    let mut rows = Vec::with_capacity(hits.len());
    let mut jobs = Vec::new();
    for hit in hits {
        let mut icon = Image::default();
        let has_icon = hit.icon.is_some();
        if let Some((path, frame)) = hit.icon {
            let cache_key = format!("{}#{frame}", path.display());
            if let Some(image) = cache.borrow().get(&cache_key) {
                icon = image.clone();
            } else {
                jobs.push(IconJob {
                    gen,
                    links: true,
                    cache_key,
                    row_key: hit.key.clone(),
                    path,
                    frame,
                });
            }
        }
        let no_image = !has_icon && (hit.key.starts_with("item:") || hit.key.starts_with("recipe:"));
        rows.push(Hit {
            title: SharedString::from(hit.title),
            subtitle: SharedString::from(hit.subtitle),
            key: SharedString::from(hit.key),
            icon,
            has_icon,
            no_image,
            can_favourite: false,
            favourite: false,
            solo: hit.solo,
            has_badge: false,
            badge: Image::default(),
            aside: SharedString::from(hit.aside),
            chip: SharedString::from(hit.chip),
        });
    }
    (rows, jobs)
}

#[cfg(test)]
mod tests {
    use super::fit_on_screen;

    #[test]
    fn vertical_dock_on_the_right_grows_left() {
        let (x, y) = fit_on_screen(1820, 80, 100, 640, 560, 740, (0, 0, 1920, 1080));
        assert_eq!((x, y), (1360, 80));
    }

    #[test]
    fn dock_on_the_left_stays() {
        let (x, y) = fit_on_screen(12, 40, 580, 84, 560, 740, (0, 0, 1920, 1080));
        assert_eq!((x, y), (12, 40));
    }

    #[test]
    fn dock_on_the_bottom_grows_up() {
        let (x, y) = fit_on_screen(40, 996, 580, 84, 560, 740, (0, 0, 1920, 1080));
        assert_eq!((x, y), (40, 340));
    }

    #[test]
    fn oversized_window_pins_to_the_origin() {
        let (x, y) = fit_on_screen(100, 100, 80, 80, 2000, 2000, (0, 0, 1920, 1080));
        assert_eq!((x, y), (0, 0));
    }
}

fn cached_icon(cache: &RefCell<HashMap<String, Image>>, path: &std::path::Path, frame: i32) -> Option<Image> {
    let key = format!("{}#{frame}", path.display());
    if let Some(image) = cache.borrow().get(&key) {
        return Some(image.clone());
    }
    let image = agf::frame_rgba(path, frame).ok().map(image_from_icon)?;
    cache.borrow_mut().insert(key, image.clone());
    Some(image)
}
