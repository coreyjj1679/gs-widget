use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSPasteboard, NSPasteboardTypeString, NSScreen, NSWindow};
use objc2_foundation::{NSRect, NSString};

fn main_thread() -> MainThreadMarker {
    MainThreadMarker::new().expect("macOS window operations must run on the UI thread")
}

fn widget_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .find(|window| window.title().to_string() == "巨商")
}

pub(crate) fn focus_window() {
    let mtm = main_thread();
    let app = NSApplication::sharedApplication(mtm);
    // Retain support for macOS versions before NSApplication::activate (14.0).
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    if let Some(window) = widget_window(mtm) {
        if window.isMiniaturized() {
            window.deminiaturize(None);
        }
        window.makeKeyAndOrderFront(None);
    }
}

pub(crate) fn copy_to_clipboard(text: &str) {
    if !try_copy_to_clipboard(text) { eprintln!("無法複製到 macOS 剪貼簿"); }
}

pub(crate) fn try_copy_to_clipboard(text: &str) -> bool {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // AppKit provides this immutable, process-lifetime string constant.
    pasteboard.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString })
}

pub(crate) fn open_source_repository() {
    // Wait for Launch Services on a worker so opening the browser cannot stall the UI.
    std::thread::spawn(|| {
        match std::process::Command::new("/usr/bin/open")
            .arg(super::SOURCE_REPOSITORY_URL)
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("無法開啟 GitHub 原始碼：{status}"),
            Err(error) => eprintln!("無法開啟 GitHub 原始碼：{error}"),
        }
    });
}

pub(crate) fn primary_screen() -> (i32, i32) {
    let mtm = main_thread();
    let screens = NSScreen::screens(mtm);
    let Some(primary) = screens.firstObject() else {
        return (1280, 800);
    };
    let scale = widget_window(mtm)
        .map(|window| window.backingScaleFactor())
        .unwrap_or_else(|| primary.backingScaleFactor());
    let size = primary.frame().size;
    (
        (size.width * scale).round() as i32,
        (size.height * scale).round() as i32,
    )
}

pub(crate) fn position_scale() -> f64 {
    let mtm = main_thread();
    widget_window(mtm)
        .map(|window| window.backingScaleFactor())
        .or_else(|| {
            NSScreen::screens(mtm)
                .firstObject()
                .map(|screen| screen.backingScaleFactor())
        })
        .unwrap_or(1.0)
}

/// AppKit has a bottom-left origin in points. Slint/winit uses a top-left
/// origin in pixels, scaled by the *window's* current backing factor even
/// when a neighbouring screen uses a different factor.
fn physical_rect(rect: NSRect, primary_height: f64, scale: f64) -> (i32, i32, i32, i32) {
    let left = rect.origin.x;
    let top = primary_height - rect.origin.y - rect.size.height;
    (
        (left * scale).round() as i32,
        (top * scale).round() as i32,
        ((left + rect.size.width) * scale).round() as i32,
        ((top + rect.size.height) * scale).round() as i32,
    )
}

fn distance_to_rect(x: f64, y: f64, rect: (i32, i32, i32, i32)) -> f64 {
    let (left, top, right, bottom) = rect;
    let dx = x - x.clamp(left as f64, right as f64);
    let dy = y - y.clamp(top as f64, bottom as f64);
    dx * dx + dy * dy
}

pub(crate) fn work_area_at(x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
    let mtm = main_thread();
    let screens = NSScreen::screens(mtm);
    let Some(primary) = screens.firstObject() else {
        return (0, 0, 1280, 800);
    };
    let primary_height = primary.frame().size.height;
    let scale = widget_window(mtm)
        .map(|window| window.backingScaleFactor())
        .unwrap_or_else(|| primary.backingScaleFactor());
    let cx = x as f64 + w as f64 / 2.0;
    let cy = y as f64 + h as f64 / 2.0;
    let screen = screens
        .iter()
        .min_by(|a, b| {
            let a = distance_to_rect(cx, cy, physical_rect(a.frame(), primary_height, scale));
            let b = distance_to_rect(cx, cy, physical_rect(b.frame(), primary_height, scale));
            a.total_cmp(&b)
        })
        .unwrap_or(primary);
    // visibleFrame excludes the menu bar and Dock.
    physical_rect(screen.visibleFrame(), primary_height, scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::{NSPoint, NSSize};

    #[test]
    fn retina_work_area_excludes_menu_bar_and_dock() {
        let visible = NSRect::new(NSPoint::new(0.0, 70.0), NSSize::new(1440.0, 805.0));
        assert_eq!(physical_rect(visible, 900.0, 2.0), (0, 50, 2880, 1660));
    }

    #[test]
    fn secondary_screens_keep_negative_coordinates_and_window_scale() {
        let left = NSRect::new(NSPoint::new(-1920.0, 0.0), NSSize::new(1920.0, 1080.0));
        assert_eq!(physical_rect(left, 900.0, 2.0), (-3840, -360, 0, 1800));
        let above = NSRect::new(NSPoint::new(0.0, 900.0), NSSize::new(1920.0, 1080.0));
        assert_eq!(physical_rect(above, 900.0, 1.0), (0, -1080, 1920, 0));
    }

    #[test]
    fn disconnected_screen_position_selects_nearest_screen() {
        assert_eq!(distance_to_rect(-100.0, 300.0, (-1920, 0, 0, 1080)), 0.0);
        assert_eq!(
            distance_to_rect(2000.0, 300.0, (0, 0, 1440, 900)),
            560.0 * 560.0
        );
    }
}
