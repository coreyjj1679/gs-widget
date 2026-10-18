use std::collections::HashSet;
use std::sync::atomic::Ordering;

use slint::{Image, Model, ModelRc, SharedString, VecModel};

use super::db;
use super::icons::cached_icon;
use super::{App, Hit, IconJob, MonsterDrop, Session};

fn section_key(key: &str) -> bool {
    key.starts_with("craft:")
        || key.starts_with("rank:")
        || key.starts_with("land:")
        || key.starts_with("icat:")
        || key.starts_with("qsrc:")
        || key.starts_with("yard:")
        || key.starts_with("realm:")
}

pub(crate) fn publish(app: &App, catalog: &db::Catalog, session: &Session, favourites: &HashSet<String>, category: &str, text: &str) {
    let lang = app.get_lang().to_string();
    let nation = app.get_nation().to_string();
    let full_monsters = category == "monster" && app.get_full_mode();
    if full_monsters {
        refresh_region_rows(app, catalog);
    }
    let previous_scope = app.get_expanded().to_string();
    let at_root = app.get_place().is_empty() && app.get_area().is_empty() && nation.is_empty();
    let board = category == "monster"
        && app.get_full_mode()
        && text.trim().is_empty()
        && !app.get_favourites_only()
        && app.get_place().is_empty()
        && app.get_area().is_empty();
    if !full_monsters || !monster_scope_key(&previous_scope) {
        app.set_expanded("".into());
    }
    app.set_detail_key("".into());
    app.set_detail_index(-1);
    app.set_board_monsters(ModelRc::new(VecModel::from(Vec::new())));
    let query = monster_query(app);
    let only = category == "monster" && (app.get_favourites_only()
        || (full_monsters && !board && !app.get_filter_global() && previous_scope == "saved:"));
    let hits = if board {
        catalog.directory_hits(&lang)
    } else if full_monsters {
        let records = full_monster_records(
            catalog, text, &lang, &app.get_place(), &nation, &app.get_area(),
            &previous_scope, app.get_filter_global(),
        );
        full_monster_results(catalog, records, favourites, only, &query, true)
    } else if category == "monster" && app.get_favourites_only() && at_root {
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
    // The directory keeps All/Saved first and each country's habitats together.
    // Full-mode result rows have already applied their own favourite/sort policy above.
    let mut hits = if full_monsters { hits } else { prioritize(catalog, &nation, hits, favourites, only) };
    let full_root = category != "monster"
        && app.get_full_mode()
        && text.trim().is_empty()
        && app.get_place().is_empty()
        && app.get_area().is_empty();
    let mut opened = String::new();
    if full_root {
        for hit in &mut hits {
            if section_key(&hit.key) {
                hit.chip = "label".into();
                if opened.is_empty() {
                    opened = hit.key.clone();
                }
            }
        }
    }
    show_rows(app, session, catalog, &nation, hits, favourites, false);
    if board && (monster_scope_key(&previous_scope) || app.get_filter_global() || !nation.is_empty() || query.is_active()) {
        app.set_expanded(previous_scope.into());
        let scope = monster_filter_scope(app, catalog);
        let records = if scope == "saved:" {
            let all = catalog.monster_records("", &lang, "", "", "");
            full_monster_results(catalog, all, favourites, true, &query, false)
        } else {
            let all = catalog.scope_monsters(&scope, "", &lang, -1, -1, -1, "");
            full_monster_results(catalog, all, favourites, false, &query, false)
        };
        show_board_monsters(app, session, catalog, &nation, records, favourites);
    }
    if !opened.is_empty() {
        app.set_expanded(opened.clone().into());
        let records = catalog.section_records(&opened, &lang);
        show_board_monsters(app, session, catalog, &nation, records, favourites);
    }
}

fn monster_scope_key(key: &str) -> bool {
    key == "all:" || key == "saved:" || key.starts_with("jump:") || key.starts_with("nation:")
}

pub(crate) fn refresh_region_rows(app: &App, catalog: &db::Catalog) {
    let rows = catalog.region_rows(&app.get_region_query(), app.get_region_country(), &app.get_lang());
    app.set_region_rows(ModelRc::new(VecModel::from(plain_hits(rows))));
    app.set_directory_offset(0.0);
}

fn full_monster_records(catalog: &db::Catalog, query: &str, lang: &str, place: &str, nation: &str, area: &str, scope: &str, global: bool) -> Vec<db::Hit> {
    if global {
        return catalog.monster_records(query, lang, "", "", "");
    }
    // Full-mode directory selections live in expanded; widget paths use place/area/nation.
    if place.is_empty() && area.is_empty() && monster_scope_key(scope) {
        if scope == "saved:" {
            return catalog.monster_records(query, lang, "", "", "");
        }
        return catalog.scope_monsters(scope, query, lang, -1, -1, -1, "");
    }
    catalog.monster_records(query, lang, place, nation, area)
}

pub(crate) fn full_monster_results(catalog: &db::Catalog, records: Vec<db::Hit>, favourites: &HashSet<String>, only: bool, query: &db::MonsterQuery, prioritize_default: bool) -> Vec<db::Hit> {
    let mut filtered = catalog.refine_monsters(records, query);
    if only {
        filtered.retain(|hit| saved_row(catalog, "", &hit.key, favourites));
        // Sort every matching favourite before applying the existing saved-list size limit.
        filtered.truncate(300);
    } else if prioritize_default && !query.has_sort() {
        filtered = prioritize(catalog, "", filtered, favourites, false);
    }
    filtered
}

pub(crate) fn saved_row(catalog: &db::Catalog, nation: &str, key: &str, favourites: &HashSet<String>) -> bool {
    match key.strip_prefix("monster:") {
        Some(code) => favourites.contains(code),
        None => catalog.scope_saved(key, nation, favourites),
    }
}

pub(crate) fn prioritize(catalog: &db::Catalog, nation: &str, hits: Vec<db::Hit>, favourites: &HashSet<String>, only: bool) -> Vec<db::Hit> {
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

pub(crate) fn sync_view_star(app: &App, favourites: &HashSet<String>) {
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

pub(crate) fn row_index(app: &App, key: &str) -> i32 {
    let hits = app.get_hits();
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return 0 };
    for index in 0..model.row_count() {
        if model.row_data(index).is_some_and(|row| row.key.as_str() == key) {
            return index as i32;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::picture_key;

    #[test]
    fn picture_keys_are_the_pages_that_carry_an_icon() {
        assert!(picture_key("item:1"));
        assert!(picture_key("set:79"));
        assert!(picture_key("recipe:659"));
        assert!(picture_key("npc:1193"));
        assert!(picture_key("hero:U00"));
        assert!(!picture_key("monster:M25"));
        assert!(!picture_key("quest:1"));
    }

    #[test]
    fn place_names_sit_beside_the_nation_and_wrap() {
        let titles = vec!["朝鮮:".into(), "朝鮮地面".into(), "大觀嶺".into(), "高手洞窟".into()];
        let chips = vec!["label".into(), "".into(), "".into(), "".into()];
        let (xs, ys, _, _) = super::board_flow(&titles, &chips, 220);
        assert_eq!(ys[0], ys[1]);
        assert!(xs[1] > xs[0]);
        assert!(ys.iter().any(|y| *y > ys[1]));
    }

    #[test]
    fn attribute_bounds_ignore_blank_junk_and_negatives() {
        assert_eq!(super::number_bound(""), -1);
        assert_eq!(super::number_bound("  "), -1);
        assert_eq!(super::number_bound("90"), 90);
        assert_eq!(super::number_bound("1,200"), 1200);
        assert_eq!(super::number_bound("90a"), -1);
        assert_eq!(super::number_bound("-3"), -1);
    }

    #[test]
    fn full_monster_search_keeps_the_directory_scope_when_switching_global_and_back() {
        let catalog = super::db::Catalog::open().expect("db");
        let all = catalog.monster_records("", "zh-en", "", "", "");
        let (scope, region) = catalog.directory_hits("zh-en").into_iter()
            .filter(|hit| hit.key.starts_with("jump:"))
            .map(|hit| {
                let rows = catalog.scope_monsters(&hit.key, "", "zh-en", -1, -1, -1, "");
                (hit.key, rows)
            })
            .find(|(_, rows)| !rows.is_empty() && rows.len() < all.len())
            .expect("populated region");
        let expected: std::collections::HashSet<_> = region.iter().map(|hit| hit.key.clone()).collect();
        let outside = all.iter().find(|hit| !expected.contains(&hit.key)).expect("monster outside region");
        let query = outside.key.trim_start_matches("monster:");
        let current = super::full_monster_records(&catalog, query, "zh-en", "", "", "", &scope, false);
        assert!(current.iter().all(|hit| expected.contains(&hit.key)));
        assert!(!current.iter().any(|hit| hit.key == outside.key));
        let global = super::full_monster_records(&catalog, query, "zh-en", "", "", "", &scope, true);
        assert!(global.iter().any(|hit| hit.key == outside.key));
        let restored: std::collections::HashSet<_> = super::full_monster_records(&catalog, "", "zh-en", "", "", "", &scope, false)
            .into_iter().map(|hit| hit.key).collect();
        assert_eq!(restored, expected);
    }

    #[test]
    fn saved_drop_filter_includes_matches_after_the_first_three_hundred_favourites() {
        let catalog = super::db::Catalog::open().expect("db");
        let all = catalog.monster_records("", "zh-en", "", "", "");
        let favourites = all.iter().map(|hit| hit.key.trim_start_matches("monster:").to_string()).collect();
        let matching = catalog.filter_monsters(all.clone(), -1, -1, -1, "item:4106");
        assert!(matching.len() < 300);
        let later = all.iter().skip(300).find(|hit| matching.iter().any(|row| row.key == hit.key))
            .expect("matching favourite beyond the old cap");
        let query = super::db::MonsterQuery { drop: "item:4106".into(), ..Default::default() };
        let filtered = super::full_monster_results(&catalog, all.clone(), &favourites, true, &query, false);
        assert!(filtered.iter().any(|hit| hit.key == later.key));
        assert_eq!(filtered.len(), matching.len());
    }

    #[test]
    fn explicit_monster_sort_precedes_favourite_priority_and_the_saved_limit() {
        let catalog = super::db::Catalog::open().expect("db");
        let all = catalog.monster_records("", "zh-en", "", "", "");
        let query = super::db::MonsterQuery { sort_key: 4, sort_desc: true, ..Default::default() };
        let sorted = catalog.refine_monsters(all.clone(), &query);
        let lowest = sorted.last().expect("lowest HP");
        let one_favourite = std::collections::HashSet::from([lowest.key.trim_start_matches("monster:").to_string()]);
        let shown = super::full_monster_results(&catalog, all.clone(), &one_favourite, false, &query, true);
        assert_eq!(shown.first().unwrap().key, sorted.first().unwrap().key);
        assert_eq!(shown.last().unwrap().key, lowest.key);
        let default = super::full_monster_results(&catalog, all.clone(), &one_favourite, false, &Default::default(), true);
        assert_eq!(default.first().unwrap().key, lowest.key);
        let all_favourites = all.iter().map(|hit| hit.key.trim_start_matches("monster:").to_string()).collect();
        let shown = super::full_monster_results(&catalog, all.clone(), &all_favourites, true, &query, true);
        let actual: Vec<_> = shown.into_iter().map(|hit| hit.key).collect();
        let expected: Vec<_> = sorted.iter().take(300).map(|hit| hit.key.clone()).collect();
        assert_eq!(actual, expected);
        assert!(all.iter().skip(300).any(|hit| expected.contains(&hit.key)), "top results must include records beyond the original saved cap");
    }
}

pub(crate) fn picture_key(key: &str) -> bool {
    key.starts_with("item:")
        || key.starts_with("set:")
        || key.starts_with("recipe:")
        || key.starts_with("npc:")
        || key.starts_with("hero:")
}

pub(crate) fn monster_query(app: &App) -> db::MonsterQuery {
    let drop_key = app.get_filter_drop_key();
    db::MonsterQuery {
        element: app.get_filter_element() as i64,
        ela_min: number_bound(&app.get_filter_ela_min()),
        ela_max: number_bound(&app.get_filter_ela_max()),
        hit_min: number_bound(&app.get_filter_hit_min()),
        hit_max: number_bound(&app.get_filter_hit_max()),
        magic_min: number_bound(&app.get_filter_magic_min()),
        magic_max: number_bound(&app.get_filter_magic_max()),
        drop: if drop_key.is_empty() { app.get_filter_drop().to_string() } else { drop_key.to_string() },
        sort_key: app.get_monster_sort_key(),
        sort_desc: app.get_monster_sort_desc(),
    }
}

pub(crate) fn monster_filter_scope(app: &App, catalog: &db::Catalog) -> String {
    let mut current = app.get_expanded().to_string();
    if !monster_scope_key(&current) {
        let nation = app.get_nation();
        current = if nation.is_empty() { "all:".into() } else { format!("nation:{nation}") };
        app.set_expanded(current.clone().into());
    }
    let scope = if app.get_filter_global() { "all:".to_string() } else { current };
    let lang = app.get_lang();
    let title = catalog.directory_hits(&lang).into_iter().find(|hit| hit.key == scope).map(|hit| hit.title).unwrap_or_default();
    app.set_legacy_place_title(title.into());
    if app.get_legacy_mode() {
        app.set_menu_open(false);
    }
    scope
}

fn number_bound(text: &str) -> i64 {
    let text = text.trim().replace(',', "");
    match text.parse::<i64>() {
        Ok(value) if value >= 0 => value,
        _ => -1,
    }
}

pub(crate) fn set_row_favourite(app: &App, key: &str, on: bool) {
    mark_favourite(&app.get_board_monsters(), key, on);
    mark_favourite(&app.get_hits(), key, on);
}

fn mark_favourite(hits: &ModelRc<Hit>, key: &str, on: bool) {
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return };
    for index in 0..model.row_count() {
        let Some(mut row) = model.row_data(index) else { continue };
        if row.key.as_str() != key {
            continue;
        }
        row.favourite = on;
        model.set_row_data(index, row);
        return;
    }
}

pub(crate) fn show_home_saved(app: &App, catalog: &db::Catalog, session: &Session, favourites: &HashSet<String>) {
    let lang = app.get_lang().to_string();
    let all = catalog.search("monster-all", "", &lang, "", "");
    let saved = prioritize(catalog, "", all, favourites, true);
    show_rows(app, session, catalog, "", saved, favourites, false);
}

pub(crate) fn show_board_monsters(app: &App, session: &Session, catalog: &db::Catalog, nation: &str, hits: Vec<db::Hit>, favourites: &HashSet<String>) {
    show_rows(app, session, catalog, nation, hits, favourites, true);
}

fn show_rows(app: &App, session: &Session, catalog: &db::Catalog, nation: &str, hits: Vec<db::Hit>, favourites: &HashSet<String>, board: bool) {
    let codes: Vec<String> = hits
        .iter()
        .filter_map(|hit| hit.key.strip_prefix("monster:").map(str::to_string))
        .collect();
    let cards = if !codes.is_empty() && (app.get_monster_gallery() || (app.get_full_mode() && app.get_category() == "monster")) {
        catalog.monster_cards(&codes, &app.get_lang())
    } else {
        std::collections::HashMap::new()
    };
    let gen = session.generation.fetch_add(1, Ordering::Relaxed) + 1;
    let flow_titles: Vec<String> = hits.iter().map(|hit| hit.title.clone()).collect();
    let flow_chips: Vec<String> = hits.iter().map(|hit| hit.chip.clone()).collect();
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
        let brief = hit.key.strip_prefix("monster:").and_then(|code| cards.get(code));
        let monster_drops = brief.map(|card| {
            let items: Vec<MonsterDrop> = card.drop_items.iter().enumerate().map(|(slot, drop)| {
                let icon = drop.icon.as_ref().and_then(|(path, frame)| {
                    let cache_key = format!("{}#{frame}", path.display());
                    let icon = session.cache.borrow().get(&cache_key).cloned();
                    if icon.is_none() && app.get_full_mode() {
                        jobs.push(IconJob {
                            gen,
                            links: false,
                            cache_key,
                            row_key: format!("{}#monster-drop:{slot}:{}", hit.key, drop.key),
                            path: path.clone(),
                            frame: *frame,
                        });
                    }
                    icon
                });
                MonsterDrop {
                    key: drop.key.clone().into(),
                    title: drop.title.clone().into(),
                    has_icon: icon.is_some(),
                    icon: icon.unwrap_or_default(),
                }
            }).collect();
            ModelRc::new(VecModel::from(items))
        }).unwrap_or_default();
        let mut drops = vec![Image::default(); 10];
        let drop_total = brief.map(|card| card.drop_total).unwrap_or(0);
        if let Some(card) = brief {
            for (index, (path, frame)) in card.drops.iter().enumerate().take(10) {
                let cache_key = format!("{}#{frame}", path.display());
                if let Some(image) = session.cache.borrow().get(&cache_key) {
                    drops[index] = image.clone();
                } else {
                    jobs.push(IconJob {
                        gen,
                        links: false,
                        cache_key,
                        row_key: format!("{}#drop{index}", hit.key),
                        path: path.clone(),
                        frame: *frame,
                    });
                }
            }
        }
        let mut battle = Image::default();
        let mut has_battle = false;
        if let Some((path, frame)) = brief.and_then(|card| card.battle.clone()) {
            let cache_key = format!("{}#{frame}", path.display());
            if let Some(image) = session.cache.borrow().get(&cache_key) {
                battle = image.clone();
                has_battle = true;
            } else {
                has_battle = true;
                jobs.push(IconJob {
                    gen,
                    links: false,
                    cache_key,
                    row_key: format!("{}#battle", hit.key),
                    path,
                    frame,
                });
            }
        }
        rows.push(Hit {
            title: SharedString::from(hit.title),
            alternate_name: SharedString::from(brief.map(|card| card.alternate_name.as_str()).unwrap_or("")),
            subtitle: SharedString::from(hit.subtitle),
            detail: SharedString::from(brief.map(|card| card.summary.as_str()).unwrap_or("")),
            monster_drops,
            spawn_places: SharedString::from(brief.map(|card| card.spawn_places.as_str()).unwrap_or("")),
            stat_kind: SharedString::from(brief.map(|card| card.kind.as_str()).unwrap_or("")),
            stat_element: SharedString::from(brief.map(|card| card.element.as_str()).unwrap_or("")),
            stat_hp: SharedString::from(brief.map(|card| card.hp.as_str()).unwrap_or("")),
            stat_mp: SharedString::from(brief.map(|card| card.mp.as_str()).unwrap_or("")),
            stat_hit: SharedString::from(brief.map(|card| card.hit.as_str()).unwrap_or("")),
            stat_magic: SharedString::from(brief.map(|card| card.magic.as_str()).unwrap_or("")),
            stat_atk: SharedString::from(brief.map(|card| card.atk.as_str()).unwrap_or("")),
            stat_exp: SharedString::from(brief.map(|card| card.exp.as_str()).unwrap_or("")),
            drop1: drops[0].clone(),
            drop2: drops[1].clone(),
            drop3: drops[2].clone(),
            drop4: drops[3].clone(),
            drop5: drops[4].clone(),
            drop6: drops[5].clone(),
            drop7: drops[6].clone(),
            drop8: drops[7].clone(),
            drop9: drops[8].clone(),
            drop10: drops[9].clone(),
            drop_total,
            battle,
            has_battle,
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
    let model = ModelRc::new(VecModel::from(rows));
    if board {
        app.set_board_monsters(model);
    } else {
        app.set_hits(model);
        apply_flow(app, &flow_titles, &flow_chips);
    }
    for job in jobs {
        let _ = session.jobs.send(job);
    }
}

pub(crate) fn text_px(text: &str, font: i32) -> i32 {
    let mut px = 0;
    for ch in text.chars() {
        px += if (ch as u32) < 128 { font * 3 / 5 } else { font };
    }
    px.max(font)
}

pub(crate) fn board_flow(titles: &[String], chips: &[String], width: i32) -> (Vec<i32>, Vec<i32>, Vec<i32>, i32) {
    let width = width.max(160);
    let gap = 8;
    let row = 32;
    let mut xs = Vec::with_capacity(titles.len());
    let mut ys = Vec::with_capacity(titles.len());
    let mut ws = Vec::with_capacity(titles.len());
    let mut x = 0;
    let mut y = 0;
    let mut indent = 0;
    for (title, chip) in titles.iter().zip(chips.iter()) {
        let heading = chip == "label";
        let font = if heading { 16 } else { 14 };
        let pad = if heading { 12 } else { 18 };
        let w = (text_px(title, font) + pad).min(width).max(font + pad);
        if heading {
            if x > 0 {
                y += row;
            }
            xs.push(0);
            ys.push(y);
            ws.push(w.min(width));
            indent = w + gap;
            x = indent;
            continue;
        }
        if x > indent && x + w > width {
            y += row;
            x = indent;
        }
        let room = (width - x).max(48);
        let w = w.min(room);
        xs.push(x);
        ys.push(y);
        ws.push(w);
        x += w + gap;
    }
    let height = if titles.is_empty() { 0 } else { y + row };
    (xs, ys, ws, height)
}

fn apply_flow(app: &App, titles: &[String], chips: &[String]) {
    let span = app.get_board_span();
    let width = if span < 80.0 { 960.0 } else { span };
    let (xs, ys, ws, height) = board_flow(titles, chips, width.round() as i32);
    app.set_flow_x(ModelRc::new(VecModel::from(xs)));
    app.set_flow_y(ModelRc::new(VecModel::from(ys)));
    app.set_flow_w(ModelRc::new(VecModel::from(ws)));
    app.set_flow_h(height);
}

pub(crate) fn reflow_board(app: &App) {
    let model = app.get_hits();
    let Some(rows) = model.as_any().downcast_ref::<VecModel<Hit>>() else {
        return;
    };
    let mut titles = Vec::new();
    let mut chips = Vec::new();
    for index in 0..rows.row_count() {
        if let Some(row) = rows.row_data(index) {
            titles.push(row.title.to_string());
            chips.push(row.chip.to_string());
        }
    }
    apply_flow(app, &titles, &chips);
}

pub(crate) fn plain_hits(hits: Vec<db::Hit>) -> Vec<Hit> {
    hits.into_iter()
        .map(|hit| Hit {
            alternate_name: Default::default(),
            title: SharedString::from(hit.title),
            subtitle: SharedString::from(hit.subtitle),
            detail: SharedString::default(),
            monster_drops: Default::default(),
            spawn_places: Default::default(),
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
            drop5: Image::default(),
            drop6: Image::default(),
            drop7: Image::default(),
            drop8: Image::default(),
            drop9: Image::default(),
            drop10: Image::default(),
            drop_total: 0,
            battle: Image::default(),
            has_battle: false,
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
