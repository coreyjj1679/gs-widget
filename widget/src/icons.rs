use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use super::agf;
use super::list::picture_key;
use super::{App, Hit, ICON_CACHE, IconJob, MonsterDrop};

struct IconTarget {
    generation: u64,
    links: bool,
    row_key: String,
}

struct DecodedIcon {
    cache_key: String,
    icon: Option<agf::Icon>,
    targets: Vec<IconTarget>,
}

fn current_generation(links: bool, generation: &AtomicU64, link_generation: &AtomicU64) -> u64 {
    if links { link_generation } else { generation }.load(Ordering::Relaxed)
}

fn discard_stale_targets(targets: &mut Vec<IconTarget>, generation: &AtomicU64, link_generation: &AtomicU64) {
    targets.retain(|target| target.generation == current_generation(target.links, generation, link_generation));
}

pub(crate) fn spawn_icon_worker(
    app: slint::Weak<App>,
    rx: Receiver<IconJob>,
    generation: Arc<AtomicU64>,
    link_generation: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let started = Instant::now();
            let mut decoded: Vec<DecodedIcon> = Vec::new();
            let mut by_cache_key = HashMap::new();
            let mut job_count = 0;
            let mut next = Some(first);
            while let Some(job) = next {
                if job.gen == current_generation(job.links, &generation, &link_generation) {
                    let target = IconTarget { generation: job.gen, links: job.links, row_key: job.row_key };
                    if let Some(&index) = by_cache_key.get(&job.cache_key) {
                        let result: &mut DecodedIcon = &mut decoded[index];
                        result.targets.push(target);
                    } else {
                        let icon = agf::frame_rgba(&job.path, job.frame).ok();
                        by_cache_key.insert(job.cache_key.clone(), decoded.len());
                        decoded.push(DecodedIcon { cache_key: job.cache_key, icon, targets: vec![target] });
                    }
                    job_count += 1;
                }
                // Keep each UI delivery small, and let the first decoded icons appear promptly.
                if job_count >= 32 || started.elapsed() >= Duration::from_millis(4) {
                    break;
                }
                next = rx.try_recv().ok();
            }
            if decoded.is_empty() {
                continue;
            }
            super::perf::record("icon.decode", started.elapsed(), job_count);
            let generation = generation.clone();
            let link_generation = link_generation.clone();
            let queued = Instant::now();
            let _ = app.upgrade_in_event_loop(move |app| {
                super::perf::record("icon.queue_wait", queued.elapsed(), job_count);
                let started = Instant::now();
                let mut painted = 0;
                for result in decoded {
                    let mut targets = result.targets;
                    discard_stale_targets(&mut targets, &generation, &link_generation);
                    if targets.is_empty() {
                        continue;
                    }
                    let has_icon = result.icon.is_some();
                    let image = match result.icon {
                        Some(icon) => {
                            let image = image_from_icon(icon);
                            remember_icon(&result.cache_key, image.clone());
                            image
                        }
                        None => Image::default(),
                    };
                    for target in targets {
                        paint_hit(&app, &target.row_key, image.clone(), has_icon, target.links);
                        painted += 1;
                    }
                }
                super::perf::record("icon.paint", started.elapsed(), painted);
            });
        }
    });
}

struct RowIndex {
    model: Weak<dyn Model<Data = Hit>>,
    count: usize,
    positions: HashMap<SharedString, usize>,
}

thread_local! {
    static ROW_INDICES: RefCell<HashMap<usize, RowIndex>> = RefCell::new(HashMap::new());
}

// Published models keep their keys/order stable; row edits update display fields only.
fn indexed_row(hits: &ModelRc<Hit>, row_key: &str) -> Option<(usize, Hit)> {
    let model: Rc<dyn Model<Data = Hit>> = hits.clone().try_into().ok()?;
    let identity = Rc::as_ptr(&model) as *const () as usize;
    ROW_INDICES.with(|slot| {
        let mut indices = slot.borrow_mut();
        // Weak references avoid retaining the previous screen's rows and image buffers.
        indices.retain(|_, index| index.model.strong_count() > 0);
        let entry = indices.entry(identity).or_insert_with(|| build_row_index(&model));
        if entry.count != model.row_count() {
            *entry = build_row_index(&model);
        }
        let position = entry.positions.get(row_key).copied();
        if let Some((index, row)) = position.and_then(|index| model.row_data(index).map(|row| (index, row))) {
            if row.key.as_str() == row_key {
                return Some((index, row));
            }
            // Current screens replace their model when ordering changes; also handle an
            // in-place reorder defensively without painting a different row.
            *entry = build_row_index(&model);
            return entry.positions.get(row_key).copied().and_then(|index| model.row_data(index).map(|row| (index, row)));
        }
        None
    })
}

fn build_row_index(model: &Rc<dyn Model<Data = Hit>>) -> RowIndex {
    let mut positions = HashMap::with_capacity(model.row_count());
    for index in 0..model.row_count() {
        if let Some(row) = model.row_data(index) {
            positions.entry(row.key).or_insert(index);
        }
    }
    RowIndex { model: Rc::downgrade(model), count: model.row_count(), positions }
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
    let Some((index, mut row)) = indexed_row(hits, row_key) else { return false };
    row.battle = icon;
    row.has_battle = true;
    model.set_row_data(index, row);
    true
}

fn paint_monster_drop(hits: &ModelRc<Hit>, row_key: &str, slot: usize, item_key: &str, icon: Image, has_icon: bool) -> bool {
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return false };
    let Some((index, mut row)) = indexed_row(hits, row_key) else { return false };
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
    true
}

fn paint_drop(hits: &ModelRc<Hit>, row_key: &str, slot: usize, icon: Image, has_icon: bool) -> bool {
    if !has_icon || slot > 9 {
        return false;
    }
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return false };
    let Some((index, mut row)) = indexed_row(hits, row_key) else { return false };
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
    true
}

pub(crate) fn paint_model(hits: &ModelRc<Hit>, row_key: &str, icon: Image, has_icon: bool) -> bool {
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return false };
    let Some((index, mut row)) = indexed_row(hits, row_key) else { return false };
    row.icon = icon;
    row.has_icon = has_icon;
    if !has_icon && picture_key(&row.key) {
        row.no_image = true;
    }
    model.set_row_data(index, row);
    true
}

pub(crate) fn image_from_icon(icon: agf::Icon) -> Image {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(icon.width, icon.height);
    buffer.make_mut_bytes().copy_from_slice(&icon.rgba);
    Image::from_rgba8(buffer)
}

#[cfg(test)]
mod tests {
    use super::{discard_stale_targets, indexed_row, note_lines, paint_model, paint_monster_drop, IconTarget};
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
    fn row_index_is_scoped_to_the_model_and_handles_removed_or_reordered_rows() {
        let rows = vec![
            Hit { key: "item:a".into(), ..Default::default() },
            Hit { key: "item:b".into(), ..Default::default() },
        ];
        let first = ModelRc::new(VecModel::from(rows.clone()));
        let second = ModelRc::new(VecModel::from(rows.into_iter().rev().collect::<Vec<_>>()));
        assert_eq!(indexed_row(&first, "item:b").unwrap().0, 1);
        assert_eq!(indexed_row(&second, "item:b").unwrap().0, 0);
        assert!(paint_model(&first, "item:b", Image::default(), false));
        assert!(first.row_data(1).unwrap().no_image);
        assert!(!second.row_data(0).unwrap().no_image);
        let model = first.as_any().downcast_ref::<VecModel<Hit>>().unwrap();
        model.swap(0, 1);
        assert_eq!(indexed_row(&first, "item:b").unwrap().0, 0);
        model.remove(0);
        assert!(indexed_row(&first, "item:b").is_none());
        assert_eq!(indexed_row(&first, "item:a").unwrap().0, 0);
        assert!(!paint_model(&first, "item:missing", Image::default(), true));
    }

    #[test]
    fn decoded_results_discard_stale_rows_without_canceling_the_other_screen() {
        use std::sync::atomic::{AtomicU64, Ordering};
        let generation = AtomicU64::new(4);
        let links = AtomicU64::new(7);
        let mut decoded = vec![
            IconTarget { generation: 4, links: false, row_key: "item:old-list".into() },
            IconTarget { generation: 7, links: true, row_key: "item:current-link".into() },
        ];
        // The catalog changed while decoded results waited in the UI event queue.
        generation.fetch_add(1, Ordering::Relaxed);
        discard_stale_targets(&mut decoded, &generation, &links);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].row_key, "item:current-link");
        links.fetch_add(1, Ordering::Relaxed);
        discard_stale_targets(&mut decoded, &generation, &links);
        assert!(decoded.is_empty());
    }

    #[test]
    #[ignore = "manual benchmark: compares previous linear scans with indexed updates"]
    fn profile_icon_row_updates() {
        use std::time::Instant;
        let count = 2_000;
        let rows = (0..count).map(|index| Hit {
            key: format!("item:{index}").into(),
            title: format!("Catalog item {index}").into(),
            ..Default::default()
        }).collect::<Vec<_>>();
        let baseline = VecModel::from(rows.clone());
        let optimized = ModelRc::new(VecModel::from(rows));
        let keys = (0..count).map(|index| format!("item:{index}")).collect::<Vec<_>>();
        let started = Instant::now();
        for key in &keys {
            for index in 0..baseline.row_count() {
                let mut row = baseline.row_data(index).unwrap();
                if row.key.as_str() == key {
                    row.icon = Image::default();
                    row.has_icon = true;
                    baseline.set_row_data(index, row);
                    break;
                }
            }
        }
        let baseline_ms = started.elapsed().as_secs_f64() * 1_000.0;
        let started = Instant::now();
        for key in &keys {
            assert!(paint_model(&optimized, key, Image::default(), true));
        }
        let optimized_ms = started.elapsed().as_secs_f64() * 1_000.0;
        eprintln!("icon rows={count} baseline_ms={baseline_ms:.3} indexed_ms={optimized_ms:.3}");
        for index in 0..count {
            assert_eq!(baseline.row_data(index).unwrap().has_icon, optimized.row_data(index).unwrap().has_icon);
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
