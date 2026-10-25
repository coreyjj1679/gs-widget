use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use super::agf;
use super::list::{picture_key, EnchantPreviewKind};
use super::{
    App, EnchantEquipment, Hit, IconJob, MonsterDrop, QuestItemChip, QuestItemRow, QuestMapNode,
    ICON_CACHE,
};

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

fn discard_stale_targets(
    targets: &mut Vec<IconTarget>,
    generation: &AtomicU64,
    link_generation: &AtomicU64,
) {
    targets.retain(|target| {
        target.generation == current_generation(target.links, generation, link_generation)
    });
}

pub(crate) fn spawn_icon_worker(
    app: slint::Weak<App>,
    rx: Receiver<IconJob>,
    generation: Arc<AtomicU64>,
    link_generation: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        process_icon_jobs(
            rx,
            &generation,
            &link_generation,
            |decoded, job_count, delivered| {
                let generation = generation.clone();
                let link_generation = link_generation.clone();
                let queued = Instant::now();
                app.upgrade_in_event_loop(move |app| {
                    super::perf::record("icon.queue_wait", queued.elapsed(), job_count);
                    let started = Instant::now();
                    let mut painted = 0;
                    for result in decoded {
                        let mut targets = result.targets;
                        discard_stale_targets(&mut targets, &generation, &link_generation);
                        if targets.is_empty() {
                            continue;
                        }
                        let (image, has_icon) =
                            resolve_decoded_icon(&result.cache_key, result.icon);
                        for target in targets {
                            paint_hit(&app, &target.row_key, image.clone(), has_icon, target.links);
                            painted += 1;
                        }
                    }
                    super::perf::record("icon.paint", started.elapsed(), painted);
                    // This unbounded channel never blocks the UI. If the window or
                    // queued closure is dropped, the receiver disconnects instead.
                    let _ = delivered.send(());
                })
                .is_ok()
            },
        );
    });
}

fn process_icon_jobs(
    rx: Receiver<IconJob>,
    generation: &AtomicU64,
    link_generation: &AtomicU64,
    mut deliver: impl FnMut(Vec<DecodedIcon>, usize, Sender<()>) -> bool,
) {
    while let Ok(first) = rx.recv() {
        let started = Instant::now();
        let mut decoded: Vec<DecodedIcon> = Vec::new();
        let mut by_cache_key = HashMap::new();
        let mut job_count = 0;
        let mut next = Some(first);
        while let Some(job) = next {
            if job.gen == current_generation(job.links, generation, link_generation) {
                let target = IconTarget {
                    generation: job.gen,
                    links: job.links,
                    row_key: job.row_key,
                };
                if let Some(&index) = by_cache_key.get(&job.cache_key) {
                    let result: &mut DecodedIcon = &mut decoded[index];
                    result.targets.push(target);
                } else {
                    let icon = agf::frame_rgba(&job.path, job.frame).ok();
                    by_cache_key.insert(job.cache_key.clone(), decoded.len());
                    decoded.push(DecodedIcon {
                        cache_key: job.cache_key,
                        icon,
                        targets: vec![target],
                    });
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
        // At most one decoded batch waits in the UI queue. Previously a
        // large page could enqueue every batch before rendering/input got
        // another turn. Waiting also lets navigation invalidate old jobs
        // before more pixels are copied from the AGF cache.
        let (delivered, acknowledgement) = mpsc::channel();
        if !deliver(decoded, job_count, delivered) || acknowledgement.recv().is_err() {
            break;
        }
    }
}

fn resolve_decoded_icon(cache_key: &str, icon: Option<agf::Icon>) -> (Image, bool) {
    let cached = ICON_CACHE.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|cache| cache.borrow().get(cache_key).cloned())
    });
    if let Some(image) = cached {
        // A different row/batch may already have uploaded this same artwork.
        // Share the image identity rather than copying its RGBA buffer again.
        return (image, true);
    }
    match icon {
        Some(icon) => {
            let image = image_from_icon(icon);
            remember_icon(cache_key, image.clone());
            (image, true)
        }
        None => (Image::default(), false),
    }
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
    if let Some(item_key) = row_key.strip_prefix("#quest-item:") {
        paint_quest_item_rows(&app.get_quest_item_rows(), item_key, icon.clone(), has_icon);
        paint_quest_map_items(&app.get_quest_map_nodes(), item_key, icon, has_icon);
        return;
    }
    for kind in [EnchantPreviewKind::Equipment, EnchantPreviewKind::Material] {
        if let Some((key, target)) = row_key.rsplit_once(kind.marker()) {
            if let Some((slot, item_key)) = target.split_once(':') {
                if let Ok(slot) = slot.parse::<usize>() {
                    if !paint_enchant_preview(&app.get_hits(), kind, key, slot, item_key, icon.clone(), has_icon) {
                        paint_enchant_preview(&app.get_board_monsters(), kind, key, slot, item_key, icon, has_icon);
                    }
                }
            }
            return;
        }
    }
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
    if links {
        // Quick search rows can share keys with the record open behind them.
        paint_model(&app.get_quick_results(), row_key, icon.clone(), has_icon);
    }
    let painted = if links {
        paint_model(&hits, row_key, icon.clone(), has_icon)
    } else {
        paint_catalog_models(&hits, &app.get_catalog_sections(), row_key, icon.clone(), has_icon)
    };
    if painted {
        return;
    }
    if links {
        let related = app.get_related();
        paint_model(&related, row_key, icon, has_icon);
    } else {
        paint_model(&app.get_board_monsters(), row_key, icon, has_icon);
    }
}

fn paint_catalog_models(hits: &ModelRc<Hit>, sections: &ModelRc<Hit>, row_key: &str, icon: Image, has_icon: bool) -> bool {
    let painted = paint_model(hits, row_key, icon.clone(), has_icon);
    // Browsing can share one model; searching keeps the directory in a separate
    // model. Both use indexed key lookup, after the worker's generation check.
    let section_painted = sections != hits && paint_model(sections, row_key, icon, has_icon);
    painted || section_painted
}

fn painted_quest_chips(
    chips: &ModelRc<QuestItemChip>,
    item_key: &str,
    icon: &Image,
    has_icon: bool,
) -> Option<ModelRc<QuestItemChip>> {
    if !chips.iter().any(|chip| chip.key.as_str() == item_key) {
        return None;
    }
    // Keep every occurrence and its quantity, including missing-image placeholders.
    // A fresh nested model identity makes the outer row's Slint bindings refresh.
    Some(ModelRc::new(VecModel::from(chips.iter().map(|mut chip| {
        if chip.key.as_str() == item_key {
            chip.icon = icon.clone();
            chip.has_icon = has_icon;
        }
        chip
    }).collect::<Vec<_>>())))
}

fn paint_quest_item_rows(rows: &ModelRc<QuestItemRow>, item_key: &str, icon: Image, has_icon: bool) -> bool {
    let Some(model) = rows.as_any().downcast_ref::<VecModel<QuestItemRow>>() else { return false };
    let mut painted = false;
    for index in 0..model.row_count() {
        let Some(mut row) = model.row_data(index) else { continue };
        let required = painted_quest_chips(&row.required_items, item_key, &icon, has_icon);
        let rewards = painted_quest_chips(&row.reward_icons, item_key, &icon, has_icon);
        if required.is_none() && rewards.is_none() {
            continue;
        }
        if let Some(items) = required { row.required_items = items; }
        if let Some(items) = rewards { row.reward_icons = items; }
        model.set_row_data(index, row);
        painted = true;
    }
    painted
}

fn paint_quest_map_items(rows: &ModelRc<QuestMapNode>, item_key: &str, icon: Image, has_icon: bool) -> bool {
    let Some(model) = rows.as_any().downcast_ref::<VecModel<QuestMapNode>>() else { return false };
    let mut painted = false;
    for index in 0..model.row_count() {
        let Some(mut row) = model.row_data(index) else { continue };
        let required = painted_quest_chips(&row.required_items, item_key, &icon, has_icon);
        let rewards = painted_quest_chips(&row.reward_icons, item_key, &icon, has_icon);
        if required.is_none() && rewards.is_none() {
            continue;
        }
        if let Some(items) = required { row.required_items = items; }
        if let Some(items) = rewards { row.reward_icons = items; }
        model.set_row_data(index, row);
        painted = true;
    }
    painted
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

fn paint_enchant_preview(hits: &ModelRc<Hit>, kind: EnchantPreviewKind, row_key: &str, slot: usize, item_key: &str, icon: Image, has_icon: bool) -> bool {
    let Some(model) = hits.as_any().downcast_ref::<VecModel<Hit>>() else { return false };
    let Some((index, mut row)) = indexed_row(hits, row_key) else { return false };
    if !row.enchant_scroll {
        return false;
    }
    let mut items: Vec<EnchantEquipment> = match kind {
        EnchantPreviewKind::Equipment => row.enchant_equipment.iter().collect(),
        EnchantPreviewKind::Material => row.enchant_materials.iter().collect(),
    };
    let Some(item) = items.get_mut(slot) else { return false };
    // A section change can reuse a row key, so validate both slot and item identity.
    if item.key.as_str() != item_key {
        return false;
    }
    item.icon = icon;
    item.has_icon = has_icon;
    let items = ModelRc::new(VecModel::from(items));
    match kind {
        EnchantPreviewKind::Equipment => row.enchant_equipment = items,
        EnchantPreviewKind::Material => row.enchant_materials = items,
    }
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
    use super::{discard_stale_targets, indexed_row, note_lines, paint_catalog_models, paint_enchant_preview, paint_model, paint_monster_drop, paint_quest_item_rows, paint_quest_map_items, EnchantPreviewKind, IconTarget};
    use crate::{EnchantEquipment, Hit, MonsterDrop, QuestItemChip, QuestItemRow, QuestMapNode};
    use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

    fn queued_job(row: &str, generation: u64, links: bool) -> crate::IconJob {
        crate::IconJob {
            gen: generation,
            links,
            cache_key: "worker-test-missing-icon".into(),
            row_key: row.into(),
            path: "worker-test-missing-icon.agf".into(),
            frame: 0,
        }
    }

    #[test]
    fn icon_worker_waits_for_ui_delivery_and_skips_canceled_backlog() {
        use std::sync::{atomic::{AtomicU64, Ordering}, mpsc, Arc};
        use std::time::Duration;
        let (jobs, queue) = mpsc::channel();
        let (deliveries, results) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(4));
        let worker_generation = Arc::clone(&generation);
        for index in 0..65 {
            jobs.send(queued_job(&format!("item:old-{index}"), 4, false)).unwrap();
        }
        let worker = std::thread::spawn(move || {
            super::process_icon_jobs(queue, &worker_generation, &AtomicU64::new(7), |batch, count, done| {
                deliveries.send((batch, count, done)).is_ok()
            });
        });
        let (batch, count, done) = results.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(count > 0 && count <= 32);
        assert_eq!(batch.len(), 1, "Duplicate artwork is decoded once per batch");
        assert_eq!(batch[0].targets.len(), count);
        assert!(matches!(results.recv_timeout(Duration::from_millis(50)), Err(mpsc::RecvTimeoutError::Timeout)),
            "No second delivery may queue until the UI acknowledges the first");

        // Navigation cancels the old list while the UI is busy, but independent
        // linked images and the new list must still be delivered afterward.
        generation.store(5, Ordering::Relaxed);
        jobs.send(queued_job("item:new", 5, false)).unwrap();
        jobs.send(queued_job("item:linked", 7, true)).unwrap();
        jobs.send(queued_job("item:stale-link", 6, true)).unwrap();
        drop(jobs);
        done.send(()).unwrap();
        let mut current = Vec::new();
        while let Ok((batch, _, done)) = results.recv_timeout(Duration::from_secs(2)) {
            for icon in batch {
                current.extend(icon.targets.into_iter().map(|target| (target.row_key, target.links)));
            }
            done.send(()).unwrap();
        }
        worker.join().unwrap();
        assert_eq!(current, vec![("item:new".into(), false), ("item:linked".into(), true)]);
    }

    #[test]
    fn dropped_ui_delivery_stops_icon_worker_without_waiting_for_more_jobs() {
        use std::sync::{atomic::AtomicU64, mpsc};
        let (jobs, queue) = mpsc::channel();
        jobs.send(queued_job("item:closing", 1, false)).unwrap();
        // Keep the producer alive: completion must come from the dropped UI
        // acknowledgement, not from the job queue disconnecting.
        let worker = std::thread::spawn(move || {
            super::process_icon_jobs(queue, &AtomicU64::new(1), &AtomicU64::new(1), |_, _, done| {
                drop(done);
                true
            });
        });
        worker.join().unwrap();
        assert!(jobs.send(queued_job("item:after-close", 1, false)).is_err());
    }

    #[test]
    fn duplicate_decoded_icons_reuse_uploaded_image_and_failures_are_not_cached() {
        use std::cell::RefCell;
        use std::collections::HashMap;
        use std::rc::Rc;
        let cache = Rc::new(RefCell::new(HashMap::new()));
        let previous = crate::ICON_CACHE.with(|slot| slot.replace(Some(Rc::clone(&cache))));
        let first = crate::agf::Icon { width: 2, height: 3, rgba: vec![255; 24] };
        let (image, found) = super::resolve_decoded_icon("same-artwork", Some(first));
        assert!(found);
        assert_eq!(image.size().width, 2);
        // A distinguishable buffer proves that the cached image was reused,
        // rather than allocating another upload for a later row's result.
        let duplicate = crate::agf::Icon { width: 1, height: 1, rgba: vec![255; 4] };
        let (reused, found) = super::resolve_decoded_icon("same-artwork", Some(duplicate));
        assert!(found);
        assert_eq!(reused.size(), image.size());
        assert_eq!(cache.borrow().len(), 1);
        let (failed, found) = super::resolve_decoded_icon("missing-artwork", None);
        assert!(!found);
        assert_eq!(failed.size().width, 0);
        assert!(!cache.borrow().contains_key("missing-artwork"));
        crate::ICON_CACHE.with(|slot| *slot.borrow_mut() = previous);
    }

    #[test]
    fn directory_portraits_update_shared_and_separate_search_models_by_key() {
        let sections = ModelRc::new(VecModel::from(vec![Hit {
            key: "land-rank:1:0".into(),
            title: "傭兵".into(),
            aside: "expanded".into(),
            ..Default::default()
        }]));
        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        assert!(paint_catalog_models(&sections, &sections, "land-rank:1:0", image.clone(), true));
        assert!(sections.row_data(0).unwrap().has_icon);

        let results = ModelRc::new(VecModel::from(vec![Hit {
            key: "hero:U08".into(),
            title: "儒生".into(),
            ..Default::default()
        }]));
        assert!(paint_catalog_models(&results, &sections, "land-rank:1:0", Image::default(), false));
        let section = sections.row_data(0).unwrap();
        assert!(!section.has_icon);
        assert_eq!(section.title.as_str(), "傭兵");
        assert_eq!(section.aside.as_str(), "expanded");
        assert!(!results.row_data(0).unwrap().has_icon);
        assert!(paint_catalog_models(&results, &sections, "land-rank:1:0", image.clone(), true));
        assert!(sections.row_data(0).unwrap().has_icon);
        assert!(paint_catalog_models(&results, &sections, "hero:U08", image, true));
        assert!(results.row_data(0).unwrap().has_icon);
        assert!(!paint_catalog_models(&results, &sections, "land-rank:10:0", Image::default(), false));
    }

    fn drop_model(count: usize) -> (ModelRc<Hit>, ModelRc<MonsterDrop>) {
        let drops = ModelRc::new(VecModel::from((0..count).map(|slot| MonsterDrop {
            key: format!("item:{slot}").into(),
            title: format!("Item {slot}").into(),
            drop_rate: "0.001%".into(),
            selected: slot == 1,
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
        assert_eq!(loaded.drop_rate.as_str(), "0.001%");
        assert!(loaded.selected);
        assert!(loaded.has_icon);
        assert_eq!(loaded.icon.size().width, 1);
        assert_eq!(hits.row_data(0).expect("monster").drop1.size().width, 0);

        assert!(!paint_monster_drop(&hits, "monster:test", 0, "item:1", image, true));
        assert!(!drops.row_data(0).expect("unchanged slot").has_icon);
        assert!(paint_monster_drop(&hits, "monster:test", 1, "item:1", Image::default(), false));
        let failed = hits.row_data(0).expect("updated monster").monster_drops.row_data(1).expect("failed image slot");
        assert_eq!(failed.key.as_str(), "item:1");
        assert_eq!(failed.title.as_str(), "Item 1");
        assert_eq!(failed.drop_rate.as_str(), "0.001%");
        assert!(failed.selected);
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
    fn enchant_equipment_loading_preserves_missing_slots_and_checks_item_identity() {
        let equipment = ModelRc::new(VecModel::from(vec![
            EnchantEquipment { key: "item:10".into(), title: "Missing icon".into(), ..Default::default() },
            EnchantEquipment {
                key: "item:20".into(), title: "Equipment".into(), alternate_name: "裝備".into(),
                ..Default::default()
            },
        ]));
        let hits = ModelRc::new(VecModel::from(vec![Hit {
            key: "item:1175".into(), enchant_equipment: equipment.clone(), enchant_scroll: true,
            ..Default::default()
        }]));
        let before = hits.row_data(0).unwrap();
        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        assert!(!paint_enchant_preview(&hits, EnchantPreviewKind::Equipment, "item:1175", 0, "item:20", image.clone(), true));
        assert!(!paint_enchant_preview(&hits, EnchantPreviewKind::Equipment, "item:1176", 1, "item:20", image.clone(), true));
        assert!(!paint_enchant_preview(&hits, EnchantPreviewKind::Equipment, "item:1175", 2, "item:20", image.clone(), true));
        assert_eq!(before.enchant_equipment, hits.row_data(0).unwrap().enchant_equipment);
        assert!(paint_enchant_preview(&hits, EnchantPreviewKind::Equipment, "item:1175", 1, "item:20", image, true));
        let after = hits.row_data(0).unwrap();
        assert_ne!(before.enchant_equipment, after.enchant_equipment);
        assert!(!equipment.row_data(1).unwrap().has_icon);
        assert_eq!(after.enchant_equipment.row_count(), 2);
        let missing = after.enchant_equipment.row_data(0).unwrap();
        assert_eq!(missing.key.as_str(), "item:10");
        assert!(!missing.has_icon);
        let loaded = after.enchant_equipment.row_data(1).unwrap();
        assert_eq!(loaded.key.as_str(), "item:20");
        assert_eq!(loaded.title.as_str(), "Equipment");
        assert_eq!(loaded.alternate_name.as_str(), "裝備");
        assert!(loaded.has_icon);
        assert_eq!(loaded.icon.size().width, 1);
        assert_eq!(after.monster_drops.row_count(), 0);

        assert!(paint_enchant_preview(&hits, EnchantPreviewKind::Equipment, "item:1175", 1, "item:20", Image::default(), false));
        let failed = hits.row_data(0).unwrap().enchant_equipment.row_data(1).unwrap();
        assert_eq!(failed.key.as_str(), "item:20");
        assert!(!failed.has_icon);
        let model = hits.as_any().downcast_ref::<VecModel<Hit>>().unwrap();
        let mut compact_row = model.row_data(0).unwrap();
        compact_row.enchant_scroll = false;
        model.set_row_data(0, compact_row);
        assert!(!paint_enchant_preview(&hits, EnchantPreviewKind::Equipment, "item:1175", 1, "item:20", Image::default(), false));
    }

    #[test]
    fn enchant_material_icons_do_not_paint_equipment_or_change_quantities() {
        let equipment = ModelRc::new(VecModel::from(vec![EnchantEquipment {
            key: "item:20".into(), title: "Equipment".into(), ..Default::default()
        }]));
        let materials = ModelRc::new(VecModel::from(vec![
            EnchantEquipment {
                key: "item:20".into(), title: "Material".into(), quantity: "× 30".into(),
                alternate_name: "材料".into(), ..Default::default()
            },
            EnchantEquipment {
                key: "item:21".into(), title: "Missing image".into(), quantity: "× 2".into(),
                ..Default::default()
            },
        ]));
        let hits = ModelRc::new(VecModel::from(vec![Hit {
            key: "item:1175".into(), enchant_equipment: equipment.clone(),
            enchant_materials: materials.clone(), enchant_scroll: true, ..Default::default()
        }]));
        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(2, 2));
        assert!(!paint_enchant_preview(&hits, EnchantPreviewKind::Material, "item:1175", 1, "item:20", image.clone(), true));
        assert!(!paint_enchant_preview(&hits, EnchantPreviewKind::Material, "item:1176", 0, "item:20", image.clone(), true));
        assert!(!paint_enchant_preview(&hits, EnchantPreviewKind::Material, "item:1175", 2, "item:20", image.clone(), true));
        assert!(paint_enchant_preview(&hits, EnchantPreviewKind::Material, "item:1175", 0, "item:20", image, true));
        let after = hits.row_data(0).unwrap();
        assert_eq!(equipment, after.enchant_equipment);
        assert!(!after.enchant_equipment.row_data(0).unwrap().has_icon);
        assert_ne!(materials, after.enchant_materials);
        let material = after.enchant_materials.row_data(0).unwrap();
        assert_eq!(material.key.as_str(), "item:20");
        assert_eq!(material.quantity.as_str(), "× 30");
        assert_eq!(material.alternate_name.as_str(), "材料");
        assert!(material.has_icon);
        assert_eq!(material.icon.size().width, 2);
        let missing = after.enchant_materials.row_data(1).unwrap();
        assert_eq!(missing.key.as_str(), "item:21");
        assert_eq!(missing.quantity.as_str(), "× 2");
        assert!(!missing.has_icon);

        let equipment_image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        assert!(paint_enchant_preview(&hits, EnchantPreviewKind::Equipment, "item:1175", 0, "item:20", equipment_image, true));
        let after_equipment = hits.row_data(0).unwrap();
        assert_eq!(after.enchant_materials, after_equipment.enchant_materials);
        assert_eq!(after_equipment.enchant_equipment.row_data(0).unwrap().icon.size().width, 1);
        assert!(paint_enchant_preview(&hits, EnchantPreviewKind::Material, "item:1175", 0, "item:20", Image::default(), false));
        let failed = hits.row_data(0).unwrap().enchant_materials.row_data(0).unwrap();
        assert_eq!(failed.quantity.as_str(), "× 30");
        assert!(!failed.has_icon);
        assert!(hits.row_data(0).unwrap().enchant_equipment.row_data(0).unwrap().has_icon);
    }

    fn quest_chips(items: &[(&str, &str)]) -> ModelRc<QuestItemChip> {
        ModelRc::new(VecModel::from(items.iter().map(|(key, quantity)| QuestItemChip {
            key: (*key).into(),
            title: format!("Name {key}").into(),
            alternate: format!("別名 {key}").into(),
            quantity: (*quantity).into(),
            ..Default::default()
        }).collect::<Vec<_>>()))
    }

    #[test]
    fn quest_item_loading_updates_all_occurrences_and_preserves_quantities() {
        let rows = ModelRc::new(VecModel::from(vec![
            QuestItemRow {
                required_items: quest_chips(&[("item:7", "30"), ("item:8", "2"), ("item:7", "5")]),
                reward_icons: quest_chips(&[("item:7", "1,000")]),
                other_objectives: "Defeat 4 enemies".into(),
            },
            QuestItemRow {
                required_items: quest_chips(&[("item:7", "9")]),
                ..Default::default()
            },
        ]));
        let before = rows.row_data(0).unwrap();
        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        assert!(paint_quest_item_rows(&rows, "item:7", image, true));
        let after = rows.row_data(0).unwrap();
        assert_ne!(before.required_items, after.required_items);
        assert_ne!(before.reward_icons, after.reward_icons);
        assert!(!before.required_items.row_data(0).unwrap().has_icon);
        assert_eq!(after.other_objectives, before.other_objectives);
        for (slot, quantity) in [(0, "30"), (2, "5")] {
            let chip = after.required_items.row_data(slot).unwrap();
            assert!(chip.has_icon);
            assert_eq!(chip.key.as_str(), "item:7");
            assert_eq!(chip.quantity.as_str(), quantity);
            assert_eq!(chip.title.as_str(), "Name item:7");
            assert_eq!(chip.alternate.as_str(), "別名 item:7");
            assert_eq!(chip.icon.size().width, 1);
        }
        assert!(!after.required_items.row_data(1).unwrap().has_icon);
        assert_eq!(after.required_items.row_data(1).unwrap().key.as_str(), "item:8");
        let reward = after.reward_icons.row_data(0).unwrap();
        assert!(reward.has_icon);
        assert_eq!(reward.quantity.as_str(), "1,000");
        let other_quest = rows.row_data(1).unwrap().required_items.row_data(0).unwrap();
        assert!(other_quest.has_icon);
        assert_eq!(other_quest.quantity.as_str(), "9");
    }

    #[test]
    fn missing_quest_item_keeps_its_slot_and_exact_item_identity() {
        let rows = ModelRc::new(VecModel::from(vec![QuestItemRow {
            required_items: quest_chips(&[("item:7", "30"), ("item:70", "8")]),
            ..Default::default()
        }]));
        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        assert!(paint_quest_item_rows(&rows, "item:70", image, true));
        assert!(paint_quest_item_rows(&rows, "item:7", Image::default(), false));
        let chips = rows.row_data(0).unwrap().required_items;
        assert_eq!(chips.row_count(), 2);
        let missing = chips.row_data(0).unwrap();
        assert_eq!(missing.key.as_str(), "item:7");
        assert_eq!(missing.quantity.as_str(), "30");
        assert!(!missing.has_icon);
        assert!(chips.row_data(1).unwrap().has_icon);
        assert!(!paint_quest_item_rows(&rows, "item:700", Image::default(), false));
        assert_eq!(chips, rows.row_data(0).unwrap().required_items);
    }

    #[test]
    fn mixed_quest_objective_icons_keep_monster_and_item_namespaces_separate() {
        let rows = ModelRc::new(VecModel::from(vec![QuestItemRow {
            required_items: quest_chips(&[("item:7", "30"), ("monster:7", "1,000"), ("monster:7", "5")]),
            reward_icons: quest_chips(&[("item:7", "2")]),
            ..Default::default()
        }]));
        let monster_icon = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(2, 2));
        assert!(paint_quest_item_rows(&rows, "monster:7", monster_icon, true));
        let after = rows.row_data(0).unwrap();
        assert!(!after.required_items.row_data(0).unwrap().has_icon);
        assert!(!after.reward_icons.row_data(0).unwrap().has_icon);
        for (slot, quantity) in [(1, "1,000"), (2, "5")] {
            let chip = after.required_items.row_data(slot).unwrap();
            assert_eq!(chip.key.as_str(), "monster:7");
            assert_eq!(chip.quantity.as_str(), quantity);
            assert_eq!(chip.icon.size().width, 2);
        }
        let item_icon = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        assert!(paint_quest_item_rows(&rows, "item:7", item_icon, true));
        let after = rows.row_data(0).unwrap();
        assert_eq!(after.required_items.row_data(0).unwrap().icon.size().width, 1);
        assert_eq!(after.reward_icons.row_data(0).unwrap().icon.size().width, 1);
        assert_eq!(after.required_items.row_data(1).unwrap().icon.size().width, 2);
    }

    #[test]
    fn quest_map_item_loading_replaces_models_without_changing_graph_metadata() {
        let rows = ModelRc::new(VecModel::from(vec![
            QuestMapNode {
                key: "quest:1".into(), x: 48.0, y: 288.0, state: 2,
                required_items: quest_chips(&[("item:7", "4")]),
                reward_icons: quest_chips(&[("item:8", "1")]),
                other_objectives: "Speak to the elder".into(),
                ..Default::default()
            },
            QuestMapNode {
                key: "quest:2".into(),
                reward_icons: quest_chips(&[("item:7", "2")]),
                ..Default::default()
            },
        ]));
        let before = rows.row_data(0).unwrap();
        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1));
        assert!(paint_quest_map_items(&rows, "item:7", image, true));
        let after = rows.row_data(0).unwrap();
        assert_ne!(before.required_items, after.required_items);
        assert_eq!(before.reward_icons, after.reward_icons);
        assert_eq!(after.key, before.key);
        assert_eq!(after.x, before.x);
        assert_eq!(after.y, before.y);
        assert_eq!(after.state, before.state);
        assert_eq!(after.other_objectives, before.other_objectives);
        assert!(after.required_items.row_data(0).unwrap().has_icon);
        assert!(rows.row_data(1).unwrap().reward_icons.row_data(0).unwrap().has_icon);
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
