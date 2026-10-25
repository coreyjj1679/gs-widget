use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

use slint::{Image, Model, ModelRc, SharedString, VecModel};

use super::db;
use super::agf;
use super::icons::{cached_icon, image_from_icon, note_lines};
use super::list::{item_row_names, picture_key, plain_hits, publish};
use super::{App, Fact, Hit, IconJob, Session, QuestMapNode, QuestMapSegment};

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
    app.set_hero_stats(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
    app.set_hero_effects(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
    app.set_hero_spirit(false);
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

/// Show one source group at a time and return the keys of the listed rows.
pub(crate) fn publish_item_sources(app: &App, rows: &[Hit], keep_sheet: bool) -> HashSet<SharedString> {
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
            alternate_name: Default::default(),
            subtitle: String::new().into(),
            detail: SharedString::default(),
            monster_drops: Default::default(),
            enchant_equipment: Default::default(),
            enchant_materials: Default::default(),
            enchant_scroll: false,
            enchant_level: Default::default(),
            enchant_rate: -1,
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
    let drop_total = groups.iter().find(|(id, _, _)| id == "dropped-by").map(|(_, _, items)| items.len()).unwrap_or(0);
    app.set_item_drop_total(i32::try_from(drop_total).unwrap_or(i32::MAX));
    let listed = shown.iter().map(|row| row.key.clone()).collect();
    app.set_item_source(pick.into());
    app.set_source_note(source_note.clone().into());
    app.set_source_note_lines(note_lines(&source_note));
    app.set_sources(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
    app.set_links(ModelRc::new(VecModel::from(shown)));
    if !keep_sheet {
        app.set_cursor(0);
    }
    app.set_drop_offset(0.0);
    listed
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
    let pick = if (keep_sheet && preferred.is_empty()) || groups.iter().any(|(header, _)| header.key.as_str() == preferred) {
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
            alternate_name: Default::default(),
            subtitle: header.subtitle,
            detail: SharedString::default(),
            monster_drops: Default::default(),
            enchant_equipment: Default::default(),
            enchant_materials: Default::default(),
            enchant_scroll: false,
            enchant_level: Default::default(),
            enchant_rate: -1,
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
        app.set_drop_offset(0.0);
    }
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

/// Select and scroll to the drop row of the item a monster page was opened from.
pub(crate) fn reveal_source_drop(app: &App, item_key: &str) {
    let links = app.get_links();
    let Some(index) = (0..links.row_count())
        .find(|&row| links.row_data(row).is_some_and(|hit| hit.key.as_str() == item_key))
    else {
        return;
    };
    let index = i32::try_from(index).unwrap_or(i32::MAX);
    app.set_pending_cursor(index);
    app.set_drop_reveal_index(index);
    app.set_drop_reveal_request(app.get_drop_reveal_request().wrapping_add(1));
}

pub(crate) fn show_key(app: &App, catalog: &db::Catalog, session: &Session, key: &str, keep_sheet: bool) {
    super::perf::begin_update();
    let _timer = super::perf::Span::new("detail.open", 1);
    app.set_drop_reveal_index(-1);
    if !keep_sheet {
        app.set_item_tab("".into());
        app.set_item_source("".into());
    }
    app.set_has_item_fx(false);
    app.set_has_set_fx(false);
    app.set_has_total_fx(false);
    app.set_sheet_can_calculate(catalog.can_calculate_record(key));
    let cache = &session.cache;
    let (detail, links, icon) = catalog.open_key(key, &app.get_lang());
    app.set_detail(detail.into());
    let gen = session.link_generation.fetch_add(1, Ordering::Relaxed) + 1;
    let related_hits = key
        .strip_prefix("monster:")
        .map(|code| catalog.monster_quests(code, &app.get_lang()))
        .unwrap_or_default();
    let (rows, mut jobs) = link_rows(cache, links, gen, catalog, &app.get_lang());
    let (related_rows, related_jobs) = link_rows(cache, related_hits, gen, catalog, &app.get_lang());
    app.set_related(ModelRc::new(VecModel::from(related_rows)));
    app.set_item_drop_total(0);
    if key.starts_with("item:") || key.starts_with("set:") {
        let listed = publish_item_sources(app, &rows, keep_sheet);
        // Collapsed groups can hold hundreds of rows; decode only listed icons.
        jobs.retain(|job| listed.contains(job.row_key.as_str()));
    } else if key.starts_with("hero:") {
        publish_hero_sections(app, rows, keep_sheet);
    } else {
        app.set_sources(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
        app.set_item_source("".into());
        app.set_source_note("".into());
        app.set_source_note_lines(0);
        app.set_links(ModelRc::new(VecModel::from(rows)));
    }
    jobs.extend(related_jobs);
    for job in jobs {
        let _ = session.jobs.send(job);
    }
    // Expanding a character section only replaces related rows. Keep the
    // existing facts and images mounted so the scroll position stays stable.
    if keep_sheet && key.starts_with("hero:") {
        return;
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
    let sprite = if let Some(code) = key.strip_prefix("monster:") {
        catalog.battle_sprite(code)
    } else {
        key.strip_prefix("hero:").and_then(|code| catalog.hero_battle_sprite(code))
    };
    if let Some((path, frame)) = sprite {
        let first = agf::playable_frames(&path).ok().and_then(|frames| frames.first().copied()).unwrap_or(frame);
        if let Some(image) = cached_icon(cache, &path, first) {
            app.set_battle_icon(image);
            app.set_show_battle(true);
        }
    }
    let lang = app.get_lang().to_string();
    app.set_sheet_note("".into());
    app.set_sheet_secondary_note("".into());
    app.set_viewing_recipe(false);
    app.set_viewing_quest(false);
    app.set_viewing_npc(false);
    app.set_viewing_hero(false);
    app.set_hero_stats(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
    app.set_hero_effects(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
    app.set_hero_spirit(false);
    app.set_viewing_item(false);
    app.set_viewing_set(false);
    clear_quest_nav(app);
    if !keep_sheet {
        app.set_drop_offset(0.0);
    }
    if let Some(code) = key.strip_prefix("monster:") {
        if let Some((name, alt, id, lines)) = catalog.monster_sheet(code, &lang) {
            apply_sheet(&app, name, alt, id, lines);
        }
    } else if let Some(code) = key.strip_prefix("item:") {
        let code = code.parse().unwrap_or(0);
        if let Some((name, alt, id, lines, note)) = catalog.item_sheet(code, &lang) {
            let success_rate = catalog.enchant_scroll_info(&[code]).get(&code)
                .map(|info| (catalog.enchant_success_label(&lang), info.rate));
            apply_sheet_with_success_rate(&app, name, alt, id, lines, success_rate);
            app.set_sheet_note(note.into());
            app.set_sheet_secondary_note(catalog.item_secondary_note(code, &lang).into());
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
                        ..Fact::default()
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
            let stats = catalog.hero_stat_lines(code, &lang).into_iter().map(|(label, value)| Fact {
                label: label.into(), value: value.into(), ..Fact::default()
            }).collect::<Vec<_>>();
            app.set_hero_stats(ModelRc::new(VecModel::from(stats)));
            let effects = catalog.hero_effect_lines(code, &lang).into_iter().map(|(label, value)| Fact {
                label: label.into(), value: value.into(), ..Fact::default()
            }).collect::<Vec<_>>();
            app.set_hero_effects(ModelRc::new(VecModel::from(effects)));
            app.set_hero_spirit(catalog.hero_is_spirit(code));
            app.set_sheet_note(note.into());
            let (secondary_intro, mut secondary_facts) = catalog.hero_secondary_text(code, &lang);
            secondary_facts.extend(catalog.hero_secondary_effect_lines(code, &lang));
            let mut secondary = Vec::new();
            if !secondary_intro.is_empty() { secondary.push(secondary_intro); }
            secondary.extend(secondary_facts.into_iter().map(|(label, text)| format!("{label}\n{text}")));
            app.set_sheet_secondary_note(secondary.join("\n\n").into());
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
            let alternate_tasks = if app.get_full_mode() {
                db::secondary_lang(&lang)
                    .and_then(|secondary| catalog.quest_sheet(quest_id, secondary))
                    .map(|sheet| sheet.3).unwrap_or_default()
            } else { Vec::new() };
            for (index, (label, mut value, key)) in tasks.into_iter().enumerate() {
                if !key.is_empty() {
                    picks.push(index as i32);
                    if let Some((_, alternate, alternate_key)) = alternate_tasks.get(index) {
                        if alternate_key == &key && !alternate.is_empty() && alternate != &value
                            && catalog.copyable_name(&key, &lang, true).is_some() {
                            value = format!("{value}\n{alternate}");
                        }
                    }
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
                    ..Fact::default()
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
                    ..Fact::default()
                })
                .collect();
            app.set_reward_facts(ModelRc::new(VecModel::from(rewards)));
            let classification: Vec<_> = catalog.quest_classification_facts(quest_id, &lang).into_iter()
                .map(|(label, value, key)| Fact {
                    label: label.into(), value: value.into(), key: key.into(),
                    icon: Image::default(), has_icon: false, section: Default::default(),
                    ..Fact::default()
                }).collect();
            app.set_quest_classification(ModelRc::new(VecModel::from(classification)));
            let (dependencies, note) = catalog.quest_dependency_rows(quest_id, &lang);
            app.set_quest_dependencies(ModelRc::new(VecModel::from(plain_hits(dependencies))));
            app.set_quest_dependency_note(note.into());
            let graph = catalog.quest_dependency_graph(quest_id, &lang);
            let ids = graph.nodes.iter().filter_map(|node| node.hit.key.strip_prefix("quest:").and_then(|id| id.parse::<i64>().ok())).collect::<Vec<_>>();
            let item_previews = catalog.quest_item_previews(&ids, &lang);
            let mut pending_items = HashSet::new();
            let mut item_jobs = Vec::new();
            app.set_quest_map_nodes(ModelRc::new(VecModel::from(graph.nodes.into_iter().map(|node| {
                let items = node.hit.key.strip_prefix("quest:").and_then(|id| id.parse::<i64>().ok())
                    .and_then(|id| item_previews.get(&id)).cloned().unwrap_or_default();
                let preview = node.hit.key.strip_prefix("quest:")
                    .and_then(|id| id.parse::<i64>().ok())
                    .map(|id| catalog.quest_map_preview(id, &lang))
                    .unwrap_or_default();
                let alternate = node.hit.key.strip_prefix("quest:")
                    .and_then(|id| id.parse::<i64>().ok())
                    .map(|id| catalog.quest_alternate_title(id, &lang)).unwrap_or_default();
                QuestMapNode {
                    key: node.hit.key.into(), title: node.hit.title.into(), subtitle: node.hit.subtitle.into(),
                    relation: node.hit.aside.into(), x: node.x, y: node.y, state: node.state, cyclic: node.cyclic,
                    summary: preview.summary.into(), objectives: preview.objectives.into(),
                    npc: preview.npc.into(), npc_key: preview.npc_key.into(), npc_location: preview.npc_location.into(),
                    rewards: preview.rewards.into(), brief: preview.brief.into(),
                    reward_items: preview.reward_items.into(), reward_stats: preview.reward_stats.into(),
                    required_items: super::quest_icons::item_chips(catalog, session, &items.required, gen, true, &mut pending_items, &mut item_jobs),
                    reward_icons: super::quest_icons::item_chips(catalog, session, &items.rewards, gen, true, &mut pending_items, &mut item_jobs),
                    other_objectives: items.other_objectives.into(),
                    alternate: alternate.into(),
                }
            }).collect::<Vec<_>>())));
            for job in item_jobs { let _ = session.jobs.send(job); }
            app.set_quest_map_segments(ModelRc::new(VecModel::from(graph.segments.into_iter().map(|line| QuestMapSegment {
                x1: line.x1, y1: line.y1, x2: line.x2, y2: line.y2,
                cyclic: line.cyclic, unresolved: line.unresolved, highlighted: line.highlighted, arrow: line.arrow,
            }).collect::<Vec<_>>())));
            app.set_quest_map_width(graph.width);
            app.set_quest_map_height(graph.height);
            app.set_quest_map_focus_x(graph.selected_x);
            app.set_quest_map_focus_y(graph.selected_y);
            app.set_quest_map_note(graph.note.into());
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
    apply_sheet_with_success_rate(app, name, alt, id, lines, None);
}

fn apply_sheet_with_success_rate(
    app: &App,
    name: String,
    alt: String,
    id: String,
    lines: Vec<(String, String)>,
    success_rate: Option<(String, i64)>,
) {
    app.set_sheet_title(name.into());
    app.set_sheet_alt(alt.into());
    app.set_sheet_code(id.into());
    let facts: Vec<Fact> = lines
        .into_iter()
        .map(|(label, value)| {
            let rate = success_rate.as_ref()
                .filter(|(success_label, _)| success_label == &label)
                .and_then(|(_, rate)| i32::try_from(*rate).ok());
            Fact {
                label: label.into(),
                value: value.into(),
                key: String::new().into(),
                icon: Image::default(),
                has_icon: false,
                section: String::new().into(),
                success_rate: rate.unwrap_or_default(),
                has_success_rate: rate.is_some(),
            }
        })
        .collect();
    app.set_facts(ModelRc::new(VecModel::from(facts)));
}

pub(crate) fn clear_quest_nav(app: &App) {
    app.set_quest_map_nodes(ModelRc::new(VecModel::from(Vec::<QuestMapNode>::new())));
    app.set_quest_map_segments(ModelRc::new(VecModel::from(Vec::<QuestMapSegment>::new())));
    app.set_quest_dependencies(ModelRc::new(VecModel::from(Vec::<Hit>::new())));
    app.set_quest_classification(ModelRc::new(VecModel::from(Vec::<Fact>::new())));
    app.set_quest_dependency_note("".into());
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
    catalog: &db::Catalog,
    lang: &str,
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
        let (alternate_name, subtitle) = item_row_names(catalog, &hit.key, &hit.subtitle, lang);
        rows.push(Hit {
            title: SharedString::from(hit.title),
            alternate_name: SharedString::from(alternate_name),
            subtitle: SharedString::from(subtitle),
            detail: SharedString::default(),
            monster_drops: Default::default(),
            enchant_equipment: Default::default(),
            enchant_materials: Default::default(),
            enchant_scroll: false,
            enchant_level: Default::default(),
            enchant_rate: -1,
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
    use super::*;

    #[test]
    fn item_source_links_show_both_names_and_keep_the_quantity() {
        let catalog = db::Catalog::open().expect("db");
        let cache = RefCell::new(HashMap::new());
        for lang in ["zh-en", "en-zh"] {
            let mut item = catalog.search("item", "11656", lang, "", "").into_iter()
                .find(|row| row.key == "item:11656").expect("item");
            let expected_alternate = catalog.item_alternate_name(&item.key, lang);
            item.subtitle = format!("{expected_alternate} · × 3");
            let monster = catalog.monster_records("M24", lang, "", "", "").into_iter()
                .find(|row| row.key == "monster:M24").expect("deer");
            let monster_subtitle = monster.subtitle.clone();
            let (rows, _) = link_rows(&cache, vec![item, monster], 1, &catalog, lang);
            assert_eq!(rows[0].alternate_name.as_str(), expected_alternate);
            assert_eq!(rows[0].subtitle.as_str(), "× 3");
            assert_eq!(rows[1].alternate_name.as_str(), "");
            assert_eq!(rows[1].subtitle.as_str(), monster_subtitle);
        }
    }
}
