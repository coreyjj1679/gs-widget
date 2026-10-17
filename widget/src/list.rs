use std::collections::HashSet;
use std::sync::atomic::Ordering;

use slint::{Image, Model, ModelRc, SharedString, VecModel};

use super::db;
use super::icons::cached_icon;
use super::{App, Hit, IconJob, Session};

pub(crate) fn publish(app: &App, catalog: &db::Catalog, session: &Session, favourites: &HashSet<String>, category: &str, text: &str) {
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
}

pub(crate) fn picture_key(key: &str) -> bool {
    key.starts_with("item:")
        || key.starts_with("set:")
        || key.starts_with("recipe:")
        || key.starts_with("npc:")
        || key.starts_with("hero:")
}

pub(crate) fn show_list(app: &App, session: &Session, catalog: &db::Catalog, nation: &str, hits: Vec<db::Hit>, favourites: &HashSet<String>) {
    let cards = if app.get_monster_gallery() {
        let codes: Vec<String> = hits
            .iter()
            .filter_map(|hit| hit.key.strip_prefix("monster:").map(str::to_string))
            .collect();
        catalog.monster_cards(&codes, &app.get_lang())
    } else {
        std::collections::HashMap::new()
    };
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
        let brief = hit.key.strip_prefix("monster:").and_then(|code| cards.get(code));
        let mut drops = [Image::default(), Image::default(), Image::default(), Image::default()];
        let drop_total = brief.map(|card| card.drop_total).unwrap_or(0);
        if let Some(card) = brief {
            for (index, (path, frame)) in card.drops.iter().enumerate().take(4) {
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
            subtitle: SharedString::from(hit.subtitle),
            detail: SharedString::default(),
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
    app.set_hits(ModelRc::new(VecModel::from(rows)));
    for job in jobs {
        let _ = session.jobs.send(job);
    }
}

pub(crate) fn plain_hits(hits: Vec<db::Hit>) -> Vec<Hit> {
    hits.into_iter()
        .map(|hit| Hit {
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
