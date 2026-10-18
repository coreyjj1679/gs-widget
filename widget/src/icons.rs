use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;

use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use super::agf;
use super::list::picture_key;
use super::{App, Hit, ICON_CACHE, IconJob};

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
    use super::note_lines;

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
