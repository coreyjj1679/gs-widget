use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

use slint::{Image, Model, ModelRc, SharedString, VecModel};

use super::db;
use super::agf;
use super::icons::{cached_icon, image_from_icon, note_lines};
use super::list::{picture_key, plain_hits, publish};
use super::{App, Fact, Hit, IconJob, Session};

pub(crate) fn open_scope(
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

pub(crate) fn publish_item_sources(app: &App, rows: &[Hit], keep_sheet: bool) {
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
            detail: SharedString::default(),
            stat_kind: SharedString::default(),
            stat_element: SharedString::default(),
            stat_hp: SharedString::default(),
            stat_mp: SharedString::default(),
            stat_hit: SharedString::default(),
            stat_magic: SharedString::default(),
            stat_atk: SharedString::default(),
            stat_exp: SharedString::default(),
            drop1: Image::default(),
            drop2: Image::default(),
            drop3: Image::default(),
            drop4: Image::default(),
            drop_total: 0,
            battle: Image::default(),
            has_battle: false,
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

pub(crate) fn publish_hero_sections(app: &App, rows: Vec<Hit>, keep_sheet: bool) {
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
            detail: SharedString::default(),
            stat_kind: SharedString::default(),
            stat_element: SharedString::default(),
            stat_hp: SharedString::default(),
            stat_mp: SharedString::default(),
            stat_hit: SharedString::default(),
            stat_magic: SharedString::default(),
            stat_atk: SharedString::default(),
            stat_exp: SharedString::default(),
            drop1: Image::default(),
            drop2: Image::default(),
            drop3: Image::default(),
            drop4: Image::default(),
            drop_total: 0,
            battle: Image::default(),
            has_battle: false,
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

pub(crate) fn focus_section(app: &App, key: &str) {
    let links = app.get_links();
    let Some(model) = links.as_any().downcast_ref::<VecModel<Hit>>() else { return };
    for index in 0..model.row_count() {
        if model.row_data(index).is_some_and(|row| row.chip.as_str() == "section" && row.key.as_str() == key) {
            app.set_cursor(index as i32);
            return;
        }
    }
}

pub(crate) fn show_key(app: &App, catalog: &db::Catalog, session: &Session, key: &str, keep_sheet: bool) {
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
    app.set_show_battle(false);
    if let Some(code) = key.strip_prefix("monster:") {
        if let Some((path, frame)) = catalog.battle_sprite(code) {
            if let Some(image) = cached_icon(cache, &path, frame) {
                app.set_battle_icon(image);
                app.set_show_battle(true);
            }
        }
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

pub(crate) fn apply_sheet(app: &App, name: String, alt: String, id: String, lines: Vec<(String, String)>) {
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

pub(crate) fn clear_quest_nav(app: &App) {
    app.set_chain_prev(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
    app.set_chain_next(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
    app.set_reward_facts(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
    app.set_task_focus(-1);
    app.set_task_picks(ModelRc::new(VecModel::from(Vec::<i32>::new())));
    app.set_quest_npc("".into());
}

pub(crate) fn link_rows(
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
            detail: SharedString::default(),
            stat_kind: SharedString::default(),
            stat_element: SharedString::default(),
            stat_hp: SharedString::default(),
            stat_mp: SharedString::default(),
            stat_hit: SharedString::default(),
            stat_magic: SharedString::default(),
            stat_atk: SharedString::default(),
            stat_exp: SharedString::default(),
            drop1: Image::default(),
            drop2: Image::default(),
            drop3: Image::default(),
            drop4: Image::default(),
            drop_total: 0,
            battle: Image::default(),
            has_battle: false,
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
