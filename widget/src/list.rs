use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

use slint::{Image, Model, ModelRc, SharedString, VecModel};

use super::db;
use super::icons::cached_icon;
use super::{App, EnchantEquipment, Hit, IconJob, MonsterDrop, QuestBrowseGroup, QuestItemRow, Session};

pub(crate) fn publish_quest_groups(app: &App, catalog: &db::Catalog, grouping: &str, query: &str) {
    let needle = query.trim().to_lowercase();
    let groups = catalog.quest_browse_groups(grouping, &app.get_lang()).into_iter()
        .filter(|row| needle.is_empty() || format!("{} {}", row.title, row.subtitle).to_lowercase().contains(&needle))
        .map(|row| QuestBrowseGroup {
            key: row.key.into(), title: row.title.into(), subtitle: row.subtitle.into(), count: row.aside.into(),
        }).collect::<Vec<_>>();
    app.set_quest_browse_groups(ModelRc::new(VecModel::from(groups)));
}

fn section_key(key: &str) -> bool {
    key.starts_with("craft:")
        || key.starts_with("rank:")
        || key.starts_with("land:")
        || key.starts_with("land-rank:")
        || key.starts_with("icat:")
        || key.starts_with("ecat:")
        || key.starts_with("qsrc:")
        || key.starts_with("yard:")
        || key.starts_with("realm:")
}

#[derive(Debug, PartialEq)]
struct CatalogPreview {
    key: String,
    title: String,
    index: i32,
}

fn catalog_scope(previous: &str, keep: bool, sections: &[db::Hit]) -> String {
    if keep && sections.iter().any(|row| row.key == previous) {
        previous.to_owned()
    } else {
        sections.first().map(|row| row.key.clone()).unwrap_or_default()
    }
}

fn catalog_preview(keep: bool, key: &str, title: &str, rows: &[db::Hit]) -> CatalogPreview {
    if !keep || key.is_empty() {
        return CatalogPreview { key: String::new(), title: String::new(), index: -1 };
    }
    let selected = rows.iter().enumerate().find(|(_, row)| row.key == key);
    CatalogPreview {
        key: key.to_owned(),
        title: selected.map(|(_, row)| row.title.clone()).unwrap_or_else(|| title.to_owned()),
        index: selected.map(|(index, _)| index as i32).unwrap_or(-1),
    }
}

pub(crate) fn reset_catalog_context(app: &App) {
    app.set_catalog_category("".into());
    app.set_catalog_searching(false);
    app.set_catalog_sections(ModelRc::new(VecModel::from(Vec::new())));
    app.set_detail_key("".into());
    app.set_detail_title("".into());
    app.set_detail_index(-1);
    app.set_catalog_browse_offset(0.0);
    app.set_directory_offset(0.0);
}

pub(crate) fn publish(app: &App, catalog: &db::Catalog, session: &Session, favourites: &HashSet<String>, category: &str, text: &str) {
    super::perf::begin_update();
    super::perf::navigation(category);
    let _timer = super::perf::Span::new("list.publish", 0);
    let lang = app.get_lang().to_string();
    let nation = app.get_nation().to_string();
    if category == "quest" && app.get_full_mode() {
        reset_catalog_context(app);
        app.set_expanded("".into());
        app.set_board_monsters(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
        let primary = catalog.quest_categories(&lang);
        let group = |keys: &[&str]| plain_hits(primary.iter().filter(|row| keys.contains(&row.key.as_str())).cloned().collect());
        app.set_quest_all(plain_hits(primary.iter().filter(|row| row.key == "qcat:all").cloned().collect()).pop().unwrap_or_default());
        app.set_quest_standard_all(plain_hits(primary.iter().filter(|row| row.key == "qcat:standard").cloned().collect()).pop().unwrap_or_default());
        app.set_quest_recurring_categories(ModelRc::new(VecModel::from(group(&["qcat:daily", "qcat:weekly", "qcat:scheduled"]))));
        app.set_quest_growth_categories(ModelRc::new(VecModel::from(group(&["qcat:starter", "qcat:intro"]))));
        app.set_quest_other_categories(ModelRc::new(VecModel::from(group(&["qcat:letter", "qcat:mercenary", "qcat:dispatch"]))));
        let category = app.get_quest_category().to_string();
        let mut categories = primary.clone();
        if category.starts_with("qgroup:") {
            let mut selected = primary[0].clone();
            selected.key = category.clone();
            selected.title = catalog.quest_browse_group_title(&category, &lang);
            categories.insert(1, selected);
        }
        app.set_quest_category_names(ModelRc::new(VecModel::from(categories.iter().map(|row| SharedString::from(row.title.as_str())).collect::<Vec<_>>())));
        app.set_quest_categories(ModelRc::new(VecModel::from(plain_hits(categories.clone()))));
        let title = categories.iter().find(|row| row.key == category).map(|row| row.title.clone()).unwrap_or_default();
        app.set_quest_category_title(title.into());
        // Searching switches to records while preserving the selected category/group.
        if !text.trim().is_empty() { app.set_quest_summary(false); }
        if app.get_quest_summary() {
            let entries = [
                ("chain", "按任務鏈", "By quest chain", "沿完整流程與分支瀏覽", "Follow complete paths and branches"),
                ("npc", "按 NPC", "By NPC", "從委託人尋找任務", "Find quests by their quest giver"),
                ("region", "按地區", "By region", "依 NPC 所在地；未註明者另列", "By NPC location, with an unknown group"),
            ].into_iter().map(|(key, zh, en, zh_note, en_note)| {
                let mut row = primary[0].clone();
                row.key = key.into();
                row.title = db::ui_text(&lang, zh, en);
                row.subtitle = db::ui_text(&lang, zh_note, en_note);
                row.aside = catalog.quest_browse_groups(key, &lang).len().to_string();
                row
            }).collect();
            app.set_quest_browse_entries(ModelRc::new(VecModel::from(plain_hits(entries))));
            if !app.get_quest_grouping().is_empty() {
                publish_quest_groups(app, catalog, &app.get_quest_grouping(), &app.get_quest_group_query());
            }
        }
        let hits = if app.get_quest_summary() {
            primary
        } else if category.starts_with("qgroup:") {
            catalog.quest_browse_group_records(&category, text, &lang)
        } else {
            catalog.quest_category_records(&category, text, &lang)
        };
        let ids: Vec<_> = hits.iter().filter_map(|hit| hit.key.strip_prefix("quest:").and_then(|id| id.parse::<i64>().ok())).collect();
        let summaries = catalog.quest_list_summaries(&ids, &lang);
        let item_previews = catalog.quest_item_previews(&ids, &lang);
        let item_sets: Vec<db::QuestItemSet> = hits.iter().map(|hit| {
            hit.key.strip_prefix("quest:").and_then(|id| id.parse::<i64>().ok())
                .and_then(|id| item_previews.get(&id).cloned()).unwrap_or_default()
        }).collect();
        let mut alternates = Vec::with_capacity(hits.len());
        let mut objectives = Vec::with_capacity(hits.len());
        let mut rewards = Vec::with_capacity(hits.len());
        let mut reward_items = Vec::with_capacity(hits.len());
        let mut reward_stats = Vec::with_capacity(hits.len());
        for hit in &hits {
            let id = hit.key.strip_prefix("quest:").and_then(|id| id.parse::<i64>().ok()).unwrap_or_default();
            alternates.push(SharedString::from(catalog.quest_alternate_title(id, &lang)));
            let summary = summaries.get(&id).cloned().unwrap_or_default();
            objectives.push(SharedString::from(summary.summary));
            rewards.push(SharedString::from(summary.rewards));
            reward_items.push(SharedString::from(summary.reward_items));
            reward_stats.push(SharedString::from(summary.reward_stats));
        }
        app.set_quest_alternate_titles(ModelRc::new(VecModel::from(alternates)));
        app.set_quest_objective_summaries(ModelRc::new(VecModel::from(objectives)));
        app.set_quest_reward_summaries(ModelRc::new(VecModel::from(rewards)));
        app.set_quest_reward_items(ModelRc::new(VecModel::from(reward_items)));
        app.set_quest_reward_stats(ModelRc::new(VecModel::from(reward_stats)));
        app.set_quest_map_open(false);
        show_rows(app, session, catalog, &nation, hits, favourites, false);
        let gen = session.generation.load(Ordering::Relaxed);
        let mut pending = HashSet::new();
        let mut jobs = Vec::new();
        let item_rows = item_sets.into_iter().map(|items| QuestItemRow {
            required_items: super::quest_icons::item_chips(catalog, session, &items.required, gen, false, &mut pending, &mut jobs),
            reward_icons: super::quest_icons::item_chips(catalog, session, &items.rewards, gen, false, &mut pending, &mut jobs),
            other_objectives: items.other_objectives.into(),
        }).collect::<Vec<_>>();
        app.set_quest_item_rows(ModelRc::new(VecModel::from(item_rows)));
        for job in jobs { let _ = session.jobs.send(job); }
        return;
    }
    let full_monsters = category == "monster" && app.get_full_mode();
    if full_monsters {
        refresh_region_rows(app, catalog);
    }
    let previous_scope = app.get_expanded().to_string();
    let full_catalog = !category.is_empty()
        && category != "monster"
        && app.get_full_mode()
        && app.get_place().is_empty()
        && app.get_area().is_empty();
    let keep_catalog = full_catalog && app.get_catalog_category() == category;
    let previous_detail = app.get_detail_key().to_string();
    let previous_title = app.get_detail_title().to_string();
    let previous_offset = app.get_list_offset();
    let was_searching = app.get_catalog_searching();
    let searching = !text.trim().is_empty();
    let at_root = app.get_place().is_empty() && app.get_area().is_empty() && nation.is_empty();
    let board = category == "monster"
        && app.get_full_mode()
        && text.trim().is_empty()
        && !app.get_favourites_only()
        && app.get_place().is_empty()
        && app.get_area().is_empty();
    if (!full_monsters && !keep_catalog) || (full_monsters && !monster_scope_key(&previous_scope)) {
        app.set_expanded("".into());
    }
    if keep_catalog && searching && !was_searching {
        app.set_catalog_browse_offset(app.get_list_offset());
    }
    if !keep_catalog {
        reset_catalog_context(app);
        app.set_board_monsters(ModelRc::new(VecModel::from(Vec::new())));
    }
    let query = monster_query(app);
    let only = category == "monster" && (app.get_favourites_only()
        || (full_monsters && !board && !app.get_filter_global() && previous_scope == "saved:"));
    let query_timer = super::perf::Span::new("list.query", 0);
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
    drop(query_timer);
    // The directory keeps All/Saved first and each country's habitats together.
    // Full-mode result rows have already applied their own favourite/sort policy above.
    let mut hits = if full_monsters { hits } else { prioritize(catalog, &nation, hits, favourites, only) };
    let mut browse_records = None;
    let mut catalog_position = None;
    let mut preview = None;
    let mut search_sections = None;
    if full_catalog {
        // Keep a stable directory while the middle pane searches the whole category.
        // Search results intentionally retain the existing global search semantics.
        let mut directory = if searching {
            catalog.search_scoped(
                category, "", &lang, "", &nation,
                &if category == "hero" { app.get_hero_group().to_string() } else { app.get_ship().to_string() },
                "",
            )
        } else {
            hits.clone()
        };
        for hit in &mut directory {
            if section_key(&hit.key) {
                hit.chip = "label".into();
            }
        }
        let mut opened = catalog_scope(&previous_scope, keep_catalog || category == "hero", &directory);
        if category == "hero" && app.get_hero_group() == "land" {
            if hero_region_parent(&previous_scope).is_some_and(|parent| directory.iter().any(|row| row.key == parent))
                && !catalog.section_records(&previous_scope, &lang).is_empty()
            {
                opened = previous_scope.clone();
            }
            directory = hero_region_directory(catalog, directory, &opened, &lang);
        }
        if !searching {
            hits = directory.clone();
        }
        let sections: Vec<_> = directory.into_iter().filter(|row| section_key(&row.key)).collect();
        if !searching && !opened.is_empty() {
            browse_records = Some(catalog.section_records(&opened, &lang));
        }
        let rows = if searching || sections.is_empty() {
            &hits
        } else {
            browse_records.as_deref().unwrap_or(&[])
        };
        let keep_preview = keep_catalog && (previous_scope.is_empty() || previous_scope == opened);
        if !keep_preview {
            app.set_catalog_browse_offset(0.0);
            app.set_directory_offset(0.0);
        }
        let selected = catalog_preview(keep_preview, &previous_detail, &previous_title, rows);
        let cursor = if searching {
            0
        } else if sections.is_empty() {
            selected.index.max(0)
        } else {
            sections.iter().position(|row| row.key == opened).map(|index| index as i32).unwrap_or(0)
        };
        let offset = if !keep_preview || searching {
            0.0
        } else if was_searching {
            app.get_catalog_browse_offset()
        } else {
            previous_offset
        };
        catalog_position = Some((cursor, offset));
        preview = Some(selected);
        app.set_catalog_category(category.into());
        app.set_catalog_searching(searching);
        app.set_catalog_sections(ModelRc::new(VecModel::from(plain_hits(sections.clone()))));
        if searching { search_sections = Some(sections); }
        app.set_expanded(opened.clone().into());
    }
    if full_catalog {
        // Slint runs changed hits after this callback returns. Let the explicit
        // catalog position below survive that deferred default reset.
        app.set_stick_cursor(true);
    }
    show_rows(app, session, catalog, &nation, hits, favourites, false);
    if full_catalog && !searching && app.get_catalog_sections().row_count() > 0 {
        app.set_catalog_sections(app.get_hits());
    }
    if let Some(sections) = search_sections {
        show_catalog_sections(app, session, sections);
    }
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
    if let Some(records) = browse_records {
        show_board_monsters(app, session, catalog, &nation, records, favourites);
    }
    if let Some(preview) = preview {
        app.set_detail_key(preview.key.into());
        app.set_detail_title(preview.title.into());
        app.set_detail_index(preview.index);
    }
    if let Some((cursor, offset)) = catalog_position {
        app.set_cursor(cursor);
        app.set_list_offset(offset);
    }
}

fn show_catalog_sections(app: &App, session: &Session, sections: Vec<db::Hit>) {
    let mut rows = plain_hits(sections.clone());
    let gen = session.generation.load(Ordering::Relaxed);
    let mut jobs = Vec::new();
    for (row, source) in rows.iter_mut().zip(sections) {
        let Some((path, frame)) = source.icon else { continue };
        row.has_icon = true;
        let cache_key = format!("{}#{frame}", path.display());
        if let Some(image) = session.cache.borrow().get(&cache_key) {
            row.icon = image.clone();
        } else {
            jobs.push(IconJob { gen, links: false, cache_key, row_key: source.key, path, frame });
        }
    }
    app.set_catalog_sections(ModelRc::new(VecModel::from(rows)));
    for job in jobs { let _ = session.jobs.send(job); }
}

fn hero_region_parent(key: &str) -> Option<String> {
    if key.starts_with("land:") {
        Some(key.to_string())
    } else {
        key.strip_prefix("land-rank:").and_then(|scope| scope.split_once(':'))
            .map(|(nation, _)| format!("land:{nation}"))
    }
}

fn hero_region_directory(catalog: &db::Catalog, regions: Vec<db::Hit>, selected: &str, lang: &str) -> Vec<db::Hit> {
    let parent = hero_region_parent(selected);
    let mut rows = Vec::new();
    for mut region in regions {
        region.chip = "label".into();
        let ranks = if parent.as_deref() == Some(region.key.as_str()) {
            region.aside = "expanded".into();
            catalog.hero_region_ranks(region.key.strip_prefix("land:").unwrap_or_default(), lang)
        } else {
            Vec::new()
        };
        rows.push(region);
        rows.extend(ranks.into_iter().map(|mut rank| {
            rank.chip = "label".into();
            rank
        }));
    }
    rows
}

pub(crate) fn refresh_hero_region_directory(app: &App, catalog: &db::Catalog, session: &Session, favourites: &HashSet<String>, selected: &str) {
    let lang = app.get_lang();
    let regions = catalog.search_scoped("hero", "", &lang, "", "", "land", "");
    let rows = hero_region_directory(catalog, regions, selected, &lang);
    let cursor_key = if selected.is_empty() { app.get_expanded().to_string() } else { selected.to_string() };
    let cursor = rows.iter().position(|row| row.key == cursor_key).unwrap_or(0);
    app.set_stick_cursor(true);
    show_rows(app, session, catalog, &app.get_nation(), rows, favourites, false);
    app.set_catalog_sections(app.get_hits());
    app.set_cursor(cursor as i32);
    app.invoke_reveal_list(cursor as i32);
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
    fn region_directory_keeps_scoped_ranks_under_their_parent() {
        let catalog = super::db::Catalog::open().expect("catalog");
        let regions = catalog.search_scoped("hero", "", "zh", "", "", "land", "");
        let rows = super::hero_region_directory(&catalog, regions.clone(), "land-rank:1:0", "zh");
        let korea = rows.iter().position(|row| row.key == "land:1").unwrap();
        assert_eq!(rows[korea].aside, "expanded");
        assert_eq!(rows[korea + 1].key, "land-rank:1:0");
        assert_eq!(rows[korea + 1].title, "傭兵");
        let ranks: Vec<_> = rows.iter().filter(|row| row.key.starts_with("land-rank:")).collect();
        assert_eq!(ranks.len(), catalog.hero_region_ranks("1", "zh").len());
        assert!(ranks.iter().all(|row| row.key.starts_with("land-rank:1:")));
        let mercenaries = catalog.section_records(&rows[korea + 1].key, "zh");
        assert!(mercenaries.iter().any(|row| row.key == "hero:U08" && row.title == "儒生"));
        assert!(mercenaries.len() < catalog.section_records("land:1", "zh").len());
        assert!(catalog.section_records("land-rank:1:invalid", "zh").is_empty());
        let closed = super::hero_region_directory(&catalog, regions.clone(), "", "zh");
        assert_eq!(closed.len(), regions.len());
        assert!(closed.iter().all(|row| row.aside != "expanded"));
    }

    #[test]
    fn catalog_search_keeps_set_preview_through_no_results_and_clear() {
        let catalog = super::db::Catalog::open().expect("db");
        let sections = catalog.search("item", "", "zh-en", "", "");
        let records = catalog.section_records("icat:set", "zh-en");
        let selected = records.last().expect("set records");
        let scope = super::catalog_scope("icat:set", true, &sections);
        assert_eq!(scope, "icat:set");
        let no_results = catalog.search("item", "catalog-no-results-8675309", "zh-en", "", "");
        assert!(no_results.is_empty());
        let searching = super::catalog_preview(true, &selected.key, &selected.title, &no_results);
        assert_eq!(searching.key, selected.key);
        assert_eq!(searching.title, selected.title);
        assert_eq!(searching.index, -1);
        let restored = super::catalog_preview(true, &searching.key, &searching.title, &catalog.section_records(&scope, "zh-en"));
        assert_eq!(restored.key, selected.key);
        assert_eq!(restored.index as usize, records.len() - 1);
    }

    #[test]
    fn catalog_search_result_preview_survives_return_to_a_different_browse_section() {
        let catalog = super::db::Catalog::open().expect("db");
        let results = catalog.search("item", "11656", "en-zh", "", "");
        let selected = results.iter().find(|row| row.key == "item:11656").expect("Essence of Stars");
        let matching = super::catalog_preview(true, &selected.key, "old title", &results);
        assert_eq!(matching.title, selected.title);
        assert!(matching.index >= 0);
        let sets = catalog.section_records("icat:set", "en-zh");
        let restored = super::catalog_preview(true, &matching.key, &matching.title, &sets);
        assert_eq!(restored.key, "item:11656");
        assert_eq!(restored.title, selected.title);
        assert_eq!(restored.index, -1);
    }

    #[test]
    fn catalog_context_reset_discards_the_previous_category_selection() {
        let catalog = super::db::Catalog::open().expect("db");
        let sections = catalog.search("quest", "", "zh-en", "", "");
        assert!(!sections.is_empty());
        assert_eq!(super::catalog_scope("icat:set", false, &sections), sections[0].key);
        assert_eq!(super::catalog_scope("icat:set", true, &sections), sections[0].key);
        let reset = super::catalog_preview(false, "set:79", "Old set", &[]);
        assert!(reset.key.is_empty());
        assert!(reset.title.is_empty());
        assert_eq!(reset.index, -1);
    }

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
    fn item_rows_separate_the_translation_without_losing_metadata() {
        let catalog = super::db::Catalog::open().expect("db");
        let rows = catalog.search("item", "11656", "zh-en", "", "");
        let row = rows.iter().find(|row| row.key == "item:11656").expect("Essence of Stars");
        let (alternate, metadata) = super::item_row_names(&catalog, &row.key, &row.subtitle, "zh-en");
        assert!(!alternate.is_empty());
        assert_eq!(metadata, "#11656");
        assert_eq!(super::item_row_names(&catalog, &row.key, "Cost · 100", "zh-en").1, "Cost · 100");
        assert_eq!(super::item_row_names(&catalog, &row.key, &alternate, "zh-en").1, "");
        assert_eq!(super::item_row_names(&catalog, "quest:1", "Quest description", "zh-en"), (String::new(), "Quest description".into()));
        for lang in ["zh-en", "en-zh"] {
            for row in catalog.search("item", "", lang, "", "") {
                let (alternate, metadata) = super::item_row_names(&catalog, &row.key, &row.subtitle, lang);
                assert!(!alternate.is_empty(), "{}", row.key);
                assert!(!metadata.is_empty(), "{} count", row.key);
                assert!(!metadata.contains(&alternate), "{}", row.key);
            }
        }
    }

    #[test]
    fn enchant_previews_keep_slots_quantities_and_reuse_cached_icons() {
        use std::cell::RefCell;
        use std::collections::HashMap;
        use slint::{Image, Model, Rgba8Pixel, SharedPixelBuffer};

        let catalog = super::db::Catalog::open().expect("db");
        let cache = RefCell::new(HashMap::new());
        let path = std::path::PathBuf::from("/tmp/enchant-equipment-test.agf");
        let mut first = catalog.search("item", "1103", "zh-en", "", "").into_iter()
            .find(|row| row.key == "item:1103").expect("base equipment");
        first.icon = None;
        let mut second = first.clone();
        second.icon = Some((path.clone(), 7));
        first.subtitle = "× 10".into();
        second.subtitle = "× 2".into();
        let equipment = vec![first, second];
        let mut jobs = Vec::new();
        let cold = super::enchant_preview_rows(&cache, &catalog, "item:1175", &equipment, super::EnchantPreviewKind::Equipment, "zh-en", 3, &mut jobs);
        assert_eq!(cold.row_count(), 2);
        assert!(cold.iter().all(|item| !item.has_icon));
        assert!(cold.iter().all(|item| item.quantity.is_empty()));
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].row_key, "item:1175#enchant-equipment:1:item:1103");
        assert_eq!(jobs[0].gen, 3);
        assert!(!jobs[0].links);
        assert_eq!(jobs[0].path, path);
        assert_eq!(jobs[0].frame, 7);
        assert_eq!(cold.row_data(1).unwrap().alternate_name.as_str(), catalog.item_alternate_name("item:1103", "zh-en"));
        let material = super::enchant_preview_rows(&cache, &catalog, "item:1175", &equipment, super::EnchantPreviewKind::Material, "zh-en", 3, &mut jobs);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[1].row_key, "item:1175#enchant-material:1:item:1103");
        assert!(material.iter().all(|item| !item.has_icon));
        assert_eq!(material.row_data(0).unwrap().quantity.as_str(), "× 10");
        assert_eq!(material.row_data(1).unwrap().quantity.as_str(), "× 2");

        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(2, 2));
        cache.borrow_mut().insert(jobs[0].cache_key.clone(), image);
        jobs.clear();
        let warm = super::enchant_preview_rows(&cache, &catalog, "item:1175", &equipment, super::EnchantPreviewKind::Equipment, "zh-en", 4, &mut jobs);
        let warm_material = super::enchant_preview_rows(&cache, &catalog, "item:1175", &equipment, super::EnchantPreviewKind::Material, "zh-en", 4, &mut jobs);
        assert!(jobs.is_empty());
        assert!(!warm.row_data(0).unwrap().has_icon);
        assert!(warm.row_data(1).unwrap().has_icon);
        assert_eq!(warm.row_data(1).unwrap().icon.size().width, 2);
        assert!(!warm_material.row_data(0).unwrap().has_icon);
        assert!(warm_material.row_data(1).unwrap().has_icon);
        assert_eq!(warm_material.row_data(0).unwrap().quantity.as_str(), "× 10");
        assert_eq!(warm_material.row_data(1).unwrap().quantity.as_str(), "× 2");
        for slot in 0..2 {
            let before = cold.row_data(slot).unwrap();
            let after = warm.row_data(slot).unwrap();
            assert_eq!(before.key, after.key);
            assert_eq!(before.title, after.title);
            assert_eq!(before.alternate_name, after.alternate_name);
        }
    }

    #[test]
    fn monster_row_metadata_keeps_the_location_without_repeating_its_name() {
        let catalog = super::db::Catalog::open().expect("db");
        for lang in ["zh-en", "en-zh"] {
            let hit = catalog.monster_records("M24", lang, "", "", "").into_iter()
                .find(|row| row.key == "monster:M24").expect("deer");
            let cards = catalog.monster_cards(&["M24".into()], lang);
            let alternate = &cards.get("M24").expect("deer card").alternate_name;
            assert!(!alternate.is_empty());
            let metadata = super::name_metadata(alternate, &hit.subtitle);
            assert!(metadata.contains("M24"));
            assert_eq!(format!("{alternate} · {metadata}"), hit.subtitle);
        }
        assert_eq!(super::name_metadata("Deer", "Deer Forest"), "Deer Forest");
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

    #[test]
    fn drop_rate_sort_keeps_highest_first_across_search_and_favourites() {
        let catalog = super::db::Catalog::open().expect("db");
        let all = super::full_monster_records(&catalog, "", "zh-en", "", "", "", "all:", false);
        let favourites = std::collections::HashSet::from(["O39".into(), "O14".into(), "O12".into()]);
        let query = super::db::MonsterQuery { drop: "item:11608".into(), sort_key: 8, sort_desc: true, ..Default::default() };
        let ranked = super::full_monster_results(&catalog, all.clone(), &favourites, false, &query, true);
        assert_eq!(ranked.len(), 13);
        assert_eq!(ranked.iter().take(3).map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
            ["monster:O12", "monster:O142", "monster:O192"]);
        assert_eq!(ranked.last().unwrap().key, "monster:O39", "unknown favourite stays behind known rates");
        let saved = super::full_monster_results(&catalog, all, &favourites, true, &query, true);
        assert_eq!(saved.iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
            ["monster:O12", "monster:O14", "monster:O39"]);
        let searched = super::full_monster_records(&catalog, "黑市", "zh-en", "", "", "", "all:", false);
        let ranked = super::full_monster_results(&catalog, searched, &favourites, false, &query, true);
        assert_eq!(ranked.iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
            ["monster:O193", "monster:O194"]);
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

fn valid_range(minimum: &str, maximum: &str) -> bool {
    let parse = |text: &str| -> Result<Option<i64>, ()> {
        let text = text.trim();
        if text.is_empty() { return Ok(None); }
        if !text.split(',').all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit())) {
            return Err(());
        }
        text.replace(',', "").parse::<i64>().map(Some).map_err(|_| ())
    };
    match (parse(minimum), parse(maximum)) {
        (Ok(Some(min)), Ok(Some(max))) => min <= max,
        (Ok(_), Ok(_)) => true,
        _ => false,
    }
}

pub(crate) fn validate_monster_filters(app: &App) -> bool {
    for (minimum, maximum, zh, en, ko) in [
        (app.get_filter_ela_min(), app.get_filter_ela_max(), "體力", "HP", "체력"),
        (app.get_filter_hit_min(), app.get_filter_hit_max(), "打擊抗性", "Hit resistance", "타격 저항"),
        (app.get_filter_magic_min(), app.get_filter_magic_max(), "魔法抗性", "Magic resistance", "마법 저항"),
    ] {
        if !valid_range(&minimum, &maximum) {
            crate::feedback::error_for(app, "monster-filter",
                &format!("{zh}範圍無效：請輸入非負整數，最低值不可大於最高值；留空代表不限。"),
                &format!("Invalid {en} range: use non-negative whole numbers, with minimum no greater than maximum. Leave blank for no limit."),
                &format!("{ko} 범위가 올바르지 않습니다. 0 이상의 정수를 입력하고 최솟값이 최댓값보다 크지 않게 하세요. 제한이 없으면 비워 두세요."));
            return false;
        }
    }
    if app.get_feedback_source() == "monster-filter" { app.invoke_dismiss_feedback(); }
    true
}

#[cfg(test)]
mod range_validation_tests {
    #[test]
    fn invalid_ranges_are_not_silently_treated_as_unbounded() {
        for (min, max) in [("abc", ""), ("-1", ""), ("3.5", ""), ("100", "10"), ("1,,000", ""), ("9999999999999999999999", "")] {
            assert!(!super::valid_range(min, max), "{min}..{max}");
        }
        for (min, max) in [("", ""), ("0", ""), ("", "2,000"), ("10", "10"), (" 1,000 ", "2,000")] {
            assert!(super::valid_range(min, max), "{min}..{max}");
        }
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

fn name_metadata<'a>(alternate: &str, subtitle: &'a str) -> &'a str {
    // Older catalog hits put the translation before the row's code/count. Keep that
    // metadata, while the dedicated alternate-name line displays the translation once.
    if alternate.is_empty() {
        subtitle
    } else if subtitle == alternate {
        ""
    } else {
        subtitle.strip_prefix(alternate)
            .and_then(|rest| rest.strip_prefix(" · "))
            .unwrap_or(subtitle)
    }
}

pub(crate) fn item_row_names(catalog: &db::Catalog, key: &str, subtitle: &str, lang: &str) -> (String, String) {
    let alternate = catalog.item_alternate_name(key, lang);
    let metadata = name_metadata(&alternate, subtitle).to_string();
    (alternate, metadata)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnchantPreviewKind {
    Equipment,
    Material,
}

impl EnchantPreviewKind {
    pub(crate) fn marker(self) -> &'static str {
        match self {
            Self::Equipment => "#enchant-equipment:",
            Self::Material => "#enchant-material:",
        }
    }
}

fn enchant_preview_rows(
    cache: &RefCell<HashMap<String, Image>>,
    catalog: &db::Catalog,
    row_key: &str,
    records: &[db::Hit],
    kind: EnchantPreviewKind,
    lang: &str,
    gen: u64,
    jobs: &mut Vec<IconJob>,
) -> ModelRc<EnchantEquipment> {
    let items = records.iter().enumerate().map(|(slot, item)| {
        let image = item.icon.as_ref().and_then(|(path, frame)| {
            let cache_key = format!("{}#{frame}", path.display());
            let image = cache.borrow().get(&cache_key).cloned();
            if image.is_none() {
                jobs.push(IconJob {
                    gen,
                    links: false,
                    cache_key,
                    row_key: format!("{row_key}{}{slot}:{}", kind.marker(), item.key),
                    path: path.clone(),
                    frame: *frame,
                });
            }
            image
        });
        EnchantEquipment {
            key: item.key.clone().into(),
            title: item.title.clone().into(),
            alternate_name: catalog.item_alternate_name(&item.key, lang).into(),
            quantity: if kind == EnchantPreviewKind::Material { item.subtitle.clone().into() } else { SharedString::default() },
            has_icon: image.is_some(),
            icon: image.unwrap_or_default(),
        }
    }).collect::<Vec<_>>();
    ModelRc::new(VecModel::from(items))
}

fn show_rows(app: &App, session: &Session, catalog: &db::Catalog, nation: &str, hits: Vec<db::Hit>, favourites: &HashSet<String>, board: bool) {
    let _timer = super::perf::Span::new("list.rows", hits.len());
    let lang = app.get_lang().to_string();
    let cards_timer = super::perf::Span::new("list.monster_cards", hits.len());
    let codes: Vec<String> = hits
        .iter()
        .filter_map(|hit| hit.key.strip_prefix("monster:").map(str::to_string))
        .collect();
    let cards = if !codes.is_empty() && (app.get_monster_gallery() || (app.get_full_mode() && app.get_category() == "monster")) {
        catalog.monster_cards(&codes, &app.get_lang())
    } else {
        std::collections::HashMap::new()
    };
    drop(cards_timer);
    let (enchant_equipment, enchant_materials, enchant_info, enchant_abilities) = if app.get_full_mode() && app.get_category() == "enchant" {
        let codes = hits.iter().filter_map(|hit| {
            hit.key.strip_prefix("item:").and_then(|code| code.parse::<i64>().ok())
        }).collect::<Vec<_>>();
        (
            catalog.enchant_applicable_equipment(&codes, &lang),
            catalog.enchant_preview_materials(&codes, &lang),
            catalog.enchant_scroll_info(&codes),
            catalog.enchant_preview_abilities(&codes, &lang),
        )
    } else {
        (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new())
    };
    // A section opens beside the directory that is still loading its icons.
    // Keep both models in the same generation; only replacing the main list
    // invalidates their pending jobs. Jobs for old section rows are keyed and
    // cannot paint a different record when a section changes.
    let gen = if board {
        session.generation.load(Ordering::Relaxed)
    } else {
        session.generation.fetch_add(1, Ordering::Relaxed) + 1
    };
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
                    drop_rate: drop.drop_rate.clone().into(),
                    selected: drop.key == app.get_filter_drop_key().as_str(),
                    alternate_name: catalog.item_alternate_name(&drop.key, &lang).into(),
                    has_icon: icon.is_some(),
                    icon: icon.unwrap_or_default(),
                }
            }).collect();
            ModelRc::new(VecModel::from(items))
        }).unwrap_or_default();
        let mut drops = vec![Image::default(); 10];
        let drop_total = brief.map(|card| card.drop_total).unwrap_or(0);
        // Full-mode board rows use monster_drops and are never shown by compact
        // MonsterCard, so skip their duplicate drop image jobs. Keep these slots
        // on hits: switching to compact while a detail is open reuses that list
        // on Back without republishing it.
        if let Some(card) = brief.filter(|_| !app.get_full_mode() || !board) {
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
        let alternate_name = brief.map(|card| card.alternate_name.clone())
            .unwrap_or_else(|| catalog.item_alternate_name(&hit.key, &lang));
        let subtitle = name_metadata(&alternate_name, &hit.subtitle);
        let code = hit.key.strip_prefix("item:").and_then(|code| code.parse::<i64>().ok());
        let equipment = code.and_then(|code| enchant_equipment.get(&code));
        let materials = code.and_then(|code| enchant_materials.get(&code));
        let info = code.and_then(|code| enchant_info.get(&code));
        let abilities = code.and_then(|code| enchant_abilities.get(&code));
        let enchant_scroll = info.is_some();
        let enchant_level = info.map(|info| info.level.to_string()).unwrap_or_default().into();
        let enchant_rate = info.and_then(|info| i32::try_from(info.rate).ok()).unwrap_or_default();
        let enchant_bonuses = abilities.map(|abilities| abilities.summary.clone()).unwrap_or_default().into();
        let enchant_bonuses_recorded = abilities.is_some_and(|abilities| abilities.recorded);
        let enchant_equipment = equipment.map(|equipment| {
            enchant_preview_rows(&session.cache, catalog, &hit.key, equipment, EnchantPreviewKind::Equipment, &lang, gen, &mut jobs)
        }).unwrap_or_default();
        let enchant_materials = materials.map(|materials| {
            enchant_preview_rows(&session.cache, catalog, &hit.key, materials, EnchantPreviewKind::Material, &lang, gen, &mut jobs)
        }).unwrap_or_default();
        rows.push(Hit {
            title: SharedString::from(hit.title),
            alternate_name: SharedString::from(alternate_name),
            subtitle: SharedString::from(subtitle),
            detail: SharedString::from(brief.map(|card| card.summary.as_str()).unwrap_or("")),
            monster_drops,
            enchant_equipment,
            enchant_materials,
            enchant_scroll,
            enchant_level,
            enchant_rate,
            enchant_bonuses,
            enchant_bonuses_recorded,
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
    let model_timer = super::perf::Span::new("list.model_set", rows.len());
    let model = ModelRc::new(VecModel::from(rows));
    if board {
        app.set_board_monsters(model);
    } else {
        app.set_hits(model);
        apply_flow(app, &flow_titles, &flow_chips);
    }
    drop(model_timer);
    super::perf::record("icons.queued", std::time::Duration::ZERO, jobs.len());
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
            enchant_equipment: Default::default(),
            enchant_materials: Default::default(),
            enchant_scroll: false,
            enchant_level: Default::default(),
            enchant_rate: 0,
            enchant_bonuses: Default::default(),
            enchant_bonuses_recorded: false,
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
