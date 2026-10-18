//! Opt-in local timings. No queries, record names, or user preferences are logged.
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use slint::{ComponentHandle, Timer, TimerMode};

use crate::{db, App};

#[derive(Serialize)]
struct Event {
    process_id: u32,
    since_start_ms: f64,
    stage: String,
    elapsed_ms: f64,
    count: usize,
}

fn sink() -> Option<&'static mpsc::SyncSender<Event>> {
    static SINK: OnceLock<Option<mpsc::SyncSender<Event>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var_os("GERSANG_PROFILE").filter(|path| !path.is_empty())?;
        let mut file = match OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => file,
            Err(error) => { eprintln!("Cannot open performance log: {error}"); return None; }
        };
        let (tx, rx) = mpsc::sync_channel::<Event>(4096);
        std::thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                if serde_json::to_writer(&mut file, &event).is_err() || file.write_all(b"\n").is_err() {
                    break;
                }
            }
        });
        Some(tx)
    }).as_ref()
}

pub(crate) fn record(stage: &str, elapsed: Duration, count: usize) {
    if let Some(tx) = sink() {
        // Diagnostics must never stall the UI if the disk cannot keep up.
        static START: OnceLock<Instant> = OnceLock::new();
        let since_start_ms = START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0;
        let _ = tx.try_send(Event { process_id: std::process::id(), since_start_ms, stage: stage.into(), elapsed_ms: elapsed.as_secs_f64() * 1000.0, count });
    }
}

pub(crate) fn navigation(category: &str) {
    // Only application-owned category identifiers are recorded, never a query.
    if sink().is_some() && ["item", "monster", "recipe", "quest", "npc", "hero", "works", "atlas", "patch"].contains(&category) {
        record(&format!("navigation.{category}"), Duration::ZERO, 0);
    }
}

pub(crate) struct Span {
    start: Option<Instant>,
    stage: &'static str,
    count: usize,
}

impl Span {
    pub(crate) fn new(stage: &'static str, count: usize) -> Self {
        Self { start: sink().map(|_| Instant::now()), stage, count }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(start) = self.start { record(self.stage, start.elapsed(), self.count); }
    }
}

fn pending_frame() -> &'static Mutex<Option<Instant>> {
    static PENDING: Mutex<Option<Instant>> = Mutex::new(None);
    &PENDING
}

pub(crate) fn begin_update() {
    if sink().is_some() {
        // Keep the earliest request when several updates coalesce into one frame.
        pending_frame().lock().unwrap().get_or_insert_with(Instant::now);
    }
}

/// The returned timer must stay alive for the duration of the GUI session.
pub(crate) fn install(app: &App) -> Option<Timer> {
    sink()?;
    let mut frame_start = None;
    let supported = app.window().set_rendering_notifier(move |state, _| {
        match state {
            slint::RenderingState::BeforeRendering => frame_start = Some(Instant::now()),
            slint::RenderingState::AfterRendering => {
                if let Some(start) = frame_start.take() { record("ui.render_span", start.elapsed(), 1); }
                if let Some(start) = pending_frame().lock().unwrap().take() {
                    record("ui.update_to_render", start.elapsed(), 1);
                }
            }
            _ => {}
        }
    }).is_ok();
    record(if supported { "ui.render_notifier_supported" } else { "ui.render_notifier_unavailable" }, Duration::ZERO, 0);
    let timer = Timer::default();
    let mut last = Instant::now();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let elapsed = last.elapsed();
        last = Instant::now();
        let delay = elapsed.saturating_sub(Duration::from_millis(50));
        if delay >= Duration::from_millis(20) { record("ui.event_loop_delay", delay, 1); }
    });
    Some(timer)
}

#[derive(Serialize)]
struct Measurement {
    scenario: String,
    count: usize,
    samples: usize,
    min_ms: f64,
    median_ms: f64,
    max_ms: f64,
}

fn measure(name: impl Into<String>, samples: usize, mut action: impl FnMut() -> usize) -> Measurement {
    let mut times = Vec::with_capacity(samples);
    let mut count = 0;
    for _ in 0..samples {
        let start = Instant::now();
        count = std::hint::black_box(action());
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    Measurement { scenario: name.into(), count, samples, min_ms: times[0], median_ms: times[samples / 2], max_ms: times[samples - 1] }
}

/// A fresh process measures the bundled catalog without creating a window.
/// Normal first-launch preference initialization applies, as in the application.
pub(crate) fn benchmark(path: &std::path::Path) -> Result<(), String> {
    let _running_guard = crate::update::running_guard()?;
    db::initialize_user_data()?;
    let start = Instant::now();
    let catalog = db::Catalog::open()?;
    let open_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut results = vec![Measurement { scenario: "catalog.open".into(), count: 1, samples: 1, min_ms: open_ms, median_ms: open_ms, max_ms: open_ms }];
    let mut sections = Vec::new();
    for category in ["item", "monster", "recipe", "quest", "npc", "hero", "works", "atlas", "patch"] {
        for query in ["", "a", "龍"] {
            results.push(measure(format!("search.{category}.{}", if query.is_empty() { "root" } else if query == "a" { "en" } else { "zh" }), 5,
                || catalog.search(category, query, "zh-en", "", "").len()));
        }
        sections.extend(catalog.search(category, "", "zh-en", "", "").into_iter().map(|hit| hit.key));
    }
    results.push(measure("search.item.fuzzy", 5, || catalog.search("item", "essense of stars", "en", "", "").len()));
    results.push(measure("search.works.no_match", 5, || catalog.search("works", "zzzz-not-a-real-item", "zh-en", "", "").len()));
    let mut pictures = std::collections::HashSet::new();
    for key in sections {
        let hits = catalog.section_records(&key, "zh-en");
        if hits.is_empty() { continue; }
        pictures.extend(hits.into_iter().filter_map(|hit| hit.icon));
        results.push(measure(format!("section.{key}"), 5, || catalog.section_records(&key, "zh-en").len()));
    }
    results.push(measure("regions.all", 5, || catalog.region_rows("", 0, "zh-en").len()));
    results.push(measure("regions.search", 5, || catalog.region_rows("a", 0, "zh-en").len()));
    results.push(measure("monsters.all", 5, || catalog.monster_records("", "zh-en", "", "", "").len()));
    let monsters = catalog.monster_records("", "zh-en", "", "", "");
    let codes: Vec<_> = monsters.iter().filter_map(|hit| hit.key.strip_prefix("monster:").map(str::to_string)).collect();
    results.push(measure("monsters.cards", 5, || catalog.monster_cards(&codes, "zh-en").len()));
    results.push(measure("monsters.filter", 5, || catalog.filter_monsters(monsters.clone(), -1, -1, -1, "a").len()));
    let mut pictures: Vec<_> = pictures.into_iter().collect();
    pictures.sort();
    let total_pictures = pictures.len();
    pictures.truncate(1000);
    let mut failures = 0;
    for (name, samples) in [("icons.cold", 1), ("icons.warm", 5)] {
        results.push(measure(name, samples, || {
            let mut decoded = 0;
            for (path, frame) in &pictures {
                match crate::agf::frame_rgba(path, *frame) {
                    Ok(icon) => { std::hint::black_box(icon); decoded += 1; }
                    Err(_) => failures += 1,
                }
            }
            decoded
        }));
    }
    results.sort_by(|a, b| b.median_ms.total_cmp(&a.median_ms));
    let report = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "debug_assertions": cfg!(debug_assertions), "scope": "catalog and image decoding only; excludes GUI layout, rendering, event loop, and image upload",
        "image_candidates": total_pictures, "image_sample": pictures.len(), "image_decode_failures_across_samples": failures,
        "measurements": results
    });
    let bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn measurements_execute_every_sample_and_keep_result_count() {
        let mut calls = 0;
        let result = super::measure("test", 5, || { calls += 1; 42 });
        assert_eq!(calls, 5);
        assert_eq!(result.count, 42);
        assert_eq!(result.samples, 5);
        assert!(result.min_ms <= result.median_ms && result.median_ms <= result.max_ms);
    }
}

/// Render the actual Slint app with explicitly synthetic rows. This isolates
/// model/layout/raster work from catalog, disk and asynchronous image loading.
pub(crate) fn benchmark_ui(path: &std::path::Path) -> Result<(), String> {
    use slint::{ModelRc, VecModel};
    // FemtoVG does not support take_snapshot. Pin the diagnostic renderer so
    // before/after results are comparable and normal application defaults stay intact.
    slint::BackendSelector::new().backend_name("winit".into()).renderer_name("software".into())
        .select().map_err(|e| e.to_string())?;
    let app = App::new().map_err(|e| e.to_string())?;
    app.set_full_mode(true);
    app.set_collapsed(false);
    app.set_lang("zh-en".into());
    #[cfg(target_os = "macos")]
    app.set_ui_font("PingFang TC".into());
    app.set_category("item".into());
    app.show().map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    let mut dimensions = Vec::new();
    for (width, height) in [(1280., 800.), (960., 640.)] {
        app.window().set_size(slint::LogicalSize::new(width, height));
        for count in [50usize, 500, 5000] {
            let mut times = Vec::new();
            for _ in 0..5 {
                app.set_hits(ModelRc::new(VecModel::<crate::Hit>::default()));
                app.set_list_offset(0.0);
                app.window().take_snapshot().map_err(|e| e.to_string())?;
                let rows: Vec<_> = (0..count).map(|index| crate::Hit {
                    key: format!("item:{index}").into(),
                    title: format!("效能測試道具 {index} / Synthetic benchmark item").into(),
                    subtitle: "Synthetic fixture — layout benchmark".into(),
                    no_image: true,
                    ..Default::default()
                }).collect();
                let start = Instant::now();
                app.set_hits(ModelRc::new(VecModel::from(rows)));
                let snapshot = app.window().take_snapshot().map_err(|e| e.to_string())?;
                std::hint::black_box(snapshot);
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            results.push(Measurement { scenario: format!("ui.catalog.{width}x{height}.{count}"), count, samples: times.len(), min_ms: times[0], median_ms: times[2], max_ms: times[4] });
            if count == 5000 {
                for (name, fraction) in [("top", 0.0f32), ("middle", 0.5), ("end", 1.0)] {
                    app.set_list_offset(-((count as f32 * 60.0 - height + 220.0).max(0.0) * fraction));
                    let start = Instant::now();
                    let snapshot = app.window().take_snapshot().map_err(|e| e.to_string())?;
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                    dimensions.push(serde_json::json!({ "logical_width": width, "logical_height": height, "position": name, "snapshot_width": snapshot.width(), "snapshot_height": snapshot.height(), "snapshot_ms": elapsed }));
                    // PPM avoids another runtime dependency and keeps this tool offline.
                    let screenshot = path.with_extension(format!("{width}x{height}.{name}.ppm"));
                    let mut file = std::fs::File::create(screenshot).map_err(|e| e.to_string())?;
                    write!(file, "P6\n{} {}\n255\n", snapshot.width(), snapshot.height()).map_err(|e| e.to_string())?;
                    let rgb: Vec<_> = snapshot.as_bytes().chunks_exact(4).flat_map(|pixel| pixel[..3].iter().copied()).collect();
                    file.write_all(&rgb).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    app.hide().map_err(|e| e.to_string())?;
    let report = serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "debug_assertions": cfg!(debug_assertions),
        "renderer": "winit-software", "scope": "synthetic rows in real Slint App; model replacement + layout + CPU snapshot raster; excludes native presentation, catalog, icons, event loop, fixture creation", "measurements": results, "snapshots": dimensions });
    std::fs::write(path, serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
