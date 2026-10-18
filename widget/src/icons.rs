use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;

use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use super::agf;
use super::list::picture_key;
use super::{App, Hit, ICON_CACHE, IconJob, MonsterDrop};

pub(crate) fn spawn_icon_worker(
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

pub(crate) fn remember_icon(cache_key: &str, image: Image) {
    ICON_CACHE.with(|slot| {
        if let Some(cache) = slot.borrow().as_ref() {
            cache.borrow_mut().insert(cache_key.to_string(), image);
        }
    });
}

pub(crate) fn show_battle_frame(app: &App, key: &str, image: Image) {
    paint_battle(&app.get_hits(), key, image.clone(), true);
    paint_battle(&app.get_board_monsters(), key, image, true);
}

pub(crate) fn paint_hit(app: &App, row_key: &str, icon: Image, has_icon: bool, links: bool) {
    if let Some((key, target)) = row_key.rsplit_once("#monster-drop:") {
        if let Some((slot, item_key)) = target.split_once(':') {
            if let Ok(slot) = slot.parse::<usize>() {
                if !paint_monster_drop(&app.get_hits(), key, slot, item_key, icon.clone(), has_icon) {
                    paint_monster_drop(&app.get_board_monsters(), key, slot, item_key, icon, has_icon);
                }
            }
        }
        return;
    }
    if let Some(key) = row_key.strip_suffix("#battle") {
        if !paint_battle(&app.get_hits(), key, icon.clone(), has_icon) {
            paint_battle(&app.get_board_monsters(), key, icon, has_icon);
        }
        return;
    }
    if let Some((key, slot)) = row_key.rsplit_once("#drop") {
        if let Ok(slot) = slot.parse::<usize>() {
            if !paint_drop(&app.get_hits(), key, slot, icon.clone(), has_icon) {
                paint_drop(&app.get_board_monsters(), key, slot, icon, has_icon);
            }
        }
        return;
    }
    let hits = if links { app.get_links() } else { app.get_hits() };
    if paint_model(&hits, row_key, icon.clone(), has_icon) {
        return;
    }
    if links {
        let related = app.get_related();
        paint_model(&related, row_key, icon, has_icon);
    } else {
        paint_model(&app.get_board_monsters(), row_key, icon, has_icon);
    }
}

fn paint_battle(hits: &ModelRc<Hit>, row_key: &str, icon: Image, has_icon: bool) -> bool {
    if !has_icon {
        return false;
    }
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return false };
    for index in 0..model.row_count() {
        let Some(mut row) = model.row_data(index) else { continue };
        if row.key.as_str() != row_key {
            continue;
        }
        row.battle = icon;
        row.has_battle = true;
        model.set_row_data(index, row);
        return true;
    }
    false
}

fn paint_monster_drop(hits: &ModelRc<Hit>, row_key: &str, slot: usize, item_key: &str, icon: Image, has_icon: bool) -> bool {
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return false };
    for index in 0..model.row_count() {
        let Some(mut row) = model.row_data(index) else { continue };
        if row.key.as_str() != row_key {
            continue;
        }
        let mut drops: Vec<MonsterDrop> = row.monster_drops.iter().collect();
        let Some(drop) = drops.get_mut(slot) else { return false };
        // Missing images keep their position; a delayed job must still match this slot's item.
        if drop.key.as_str() != item_key {
            return false;
        }
        drop.icon = icon;
        drop.has_icon = has_icon;
        // Replace the nested model identity so Slint refreshes the row's drop bindings.
        row.monster_drops = ModelRc::new(VecModel::from(drops));
        model.set_row_data(index, row);
        return true;
    }
    false
}

fn paint_drop(hits: &ModelRc<Hit>, row_key: &str, slot: usize, icon: Image, has_icon: bool) -> bool {
    if !has_icon || slot > 9 {
        return false;
    }
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return false };
    for index in 0..model.row_count() {
        let Some(mut row) = model.row_data(index) else { continue };
        if row.key.as_str() != row_key {
            continue;
        }
        match slot {
            0 => row.drop1 = icon,
            1 => row.drop2 = icon,
            2 => row.drop3 = icon,
            3 => row.drop4 = icon,
            4 => row.drop5 = icon,
            5 => row.drop6 = icon,
            6 => row.drop7 = icon,
            7 => row.drop8 = icon,
            8 => row.drop9 = icon,
            _ => row.drop10 = icon,
        }
        model.set_row_data(index, row);
        return true;
    }
    false
}

pub(crate) fn paint_model(hits: &ModelRc<Hit>, row_key: &str, icon: Image, has_icon: bool) -> bool {
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

pub(crate) fn image_from_icon(icon: agf::Icon) -> Image {
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

#[cfg(test)]
mod tests {
    use super::{note_lines, paint_monster_drop};
    use crate::{Hit, MonsterDrop};
    use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

    fn drop_model(count: usize) -> (ModelRc<Hit>, ModelRc<MonsterDrop>) {
        let drops = ModelRc::new(VecModel::from((0..count).map(|slot| MonsterDrop {
            key: format!("item:{slot}").into(),
            title: format!("Item {slot}").into(),
            ..Default::default()
        }).collect::<Vec<_>>()));
        let hits = ModelRc::new(VecModel::from(vec![Hit {
            key: "monster:test".into(),
            monster_drops: drops.clone(),
            ..Default::default()
        }]));
        (hits, drops)
    }

    #[test]
    fn missing_drop_icons_do_not_shift_the_click_target() {
        let (hits, _) = drop_model(2);
        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        assert!(paint_monster_drop(&hits, "monster:test", 1, "item:1", image.clone(), true));
        let drops = hits.row_data(0).expect("updated monster").monster_drops;
        let missing = drops.row_data(0).expect("missing image slot");
        assert_eq!(missing.key.as_str(), "item:0");
        assert!(!missing.has_icon);
        let loaded = drops.row_data(1).expect("loaded image slot");
        assert_eq!(loaded.key.as_str(), "item:1");
        assert_eq!(loaded.title.as_str(), "Item 1");
        assert!(loaded.has_icon);
        assert_eq!(loaded.icon.size().width, 1);
        assert_eq!(hits.row_data(0).expect("monster").drop1.size().width, 0);

        assert!(!paint_monster_drop(&hits, "monster:test", 0, "item:1", image, true));
        assert!(!drops.row_data(0).expect("unchanged slot").has_icon);
        assert!(paint_monster_drop(&hits, "monster:test", 1, "item:1", Image::default(), false));
        let failed = hits.row_data(0).expect("updated monster").monster_drops.row_data(1).expect("failed image slot");
        assert_eq!(failed.key.as_str(), "item:1");
        assert_eq!(failed.title.as_str(), "Item 1");
        assert!(!failed.has_icon);
    }

    #[test]
    fn interactive_drop_icons_are_not_limited_to_ten_slots() {
        let (hits, _) = drop_model(12);
        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        assert!(paint_monster_drop(&hits, "monster:test", 11, "item:11", image, true));
        let drops = hits.row_data(0).expect("updated monster").monster_drops;
        assert_eq!(drops.row_count(), 12);
        let last = drops.row_data(11).expect("twelfth drop");
        assert_eq!(last.key.as_str(), "item:11");
        assert!(last.has_icon);
        assert!(!paint_monster_drop(&hits, "monster:test", 12, "item:12", Image::default(), false));
    }

    #[test]
    fn loaded_drop_icons_replace_the_nested_model_identity_for_bindings() {
        let (hits, _) = drop_model(6);
        let before = hits.row_data(0).expect("initial monster");
        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        assert!(paint_monster_drop(&hits, "monster:test", 3, "item:3", image, true));
        let after = hits.row_data(0).expect("updated monster");
        assert_ne!(before.monster_drops, after.monster_drops, "The nested model identity must change to refresh its bindings");
        assert!(!before.monster_drops.row_data(3).expect("original drop").has_icon);
        assert!(after.monster_drops.row_data(3).expect("loaded drop").has_icon);
        for slot in [2, 4] {
            let previous = before.monster_drops.row_data(slot).expect("original neighbor");
            let current = after.monster_drops.row_data(slot).expect("unchanged neighbor");
            assert_eq!(current.key, previous.key);
            assert_eq!(current.title, previous.title);
            assert_eq!(current.has_icon, previous.has_icon);
            assert_eq!(current.icon.size(), previous.icon.size());
        }
    }

    #[test]
    fn note_height_wraps_by_width_and_stays_within_eight_lines() {
        assert_eq!(note_lines(""), 0);
        assert_eq!(note_lines("字"), 1);
        assert_eq!(note_lines(&"字".repeat(28)), 1);
        assert_eq!(note_lines(&"字".repeat(29)), 2);
        assert_eq!(note_lines(&"字".repeat(28 * 9)), 8);
    }
}

pub(crate) fn note_lines(note: &str) -> i32 {
    if note.is_empty() {
        return 0;
    }
    let width = 28;
    note.chars().count().div_ceil(width).clamp(1, 8) as i32
}

pub(crate) fn cached_icon(cache: &RefCell<HashMap<String, Image>>, path: &std::path::Path, frame: i32) -> Option<Image> {
    let key = format!("{}#{frame}", path.display());
    if let Some(image) = cache.borrow().get(&key) {
        return Some(image.clone());
    }
    let image = agf::frame_rgba(path, frame).ok().map(image_from_icon)?;
    cache.borrow_mut().insert(key, image.clone());
    Some(image)
}
