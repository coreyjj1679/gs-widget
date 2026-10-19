use super::db;
use super::window_logical;

const SOURCE_REPOSITORY_URL: &str = "https://github.com/coreyjj1679/gs-widget";

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub(crate) use macos::{copy_to_clipboard, try_copy_to_clipboard, focus_window, open_source_repository, primary_screen, work_area_at};
#[cfg(target_os = "macos")]
pub(crate) use macos::position_scale;
#[cfg(windows)]
pub(crate) use windows::{copy_to_clipboard, try_copy_to_clipboard, focus_window, open_source_repository, primary_screen, work_area_at};

#[cfg(test)]
mod tests {
    use super::fit_on_screen;

    #[test]
    fn vertical_dock_on_the_right_grows_left() {
        let (x, y) = fit_on_screen(1820, 80, 100, 640, 560, 740, (0, 0, 1920, 1080));
        assert_eq!((x, y), (1360, 80));
    }

    #[test]
    fn dock_on_the_left_stays() {
        let (x, y) = fit_on_screen(12, 40, 580, 84, 560, 740, (0, 0, 1920, 1080));
        assert_eq!((x, y), (12, 40));
    }

    #[test]
    fn dock_on_the_bottom_grows_up() {
        let (x, y) = fit_on_screen(40, 996, 580, 84, 560, 740, (0, 0, 1920, 1080));
        assert_eq!((x, y), (40, 340));
    }

    #[test]
    fn oversized_window_pins_to_the_origin() {
        let (x, y) = fit_on_screen(100, 100, 80, 80, 2000, 2000, (0, 0, 1920, 1080));
        assert_eq!((x, y), (0, 0));
    }

    #[test]
    fn saved_window_on_a_left_hand_monitor_keeps_negative_coordinates() {
        let position = fit_on_screen(-1800, 120, 560, 740, 560, 740, (-1920, 0, 0, 1040));
        assert_eq!(position, (-1800, 120));
    }

    #[test]
    fn restored_window_avoids_the_taskbar_and_a_disconnected_monitor() {
        assert_eq!(
            fit_on_screen(1400, 500, 560, 740, 560, 740, (0, 0, 1920, 1040)),
            (1360, 300)
        );
        assert_eq!(
            fit_on_screen(-1800, -800, 560, 740, 560, 740, (0, 40, 1920, 1080)),
            (0, 40)
        );
    }

    #[test]
    fn rescaled_window_is_fitted_using_its_actual_physical_size() {
        assert_eq!(
            fit_on_screen(3280, 600, 1120, 1480, 1120, 1480, (1920, 0, 4480, 1400)),
            (3280, 0)
        );
    }

    #[test]
    fn full_span_is_1280_by_800_unless_the_work_area_is_smaller() {
        assert_eq!(super::full_span(1920.0, 1080.0), (1280.0, 800.0));
        assert_eq!(super::full_span(1100.0, 700.0), (1100.0, 700.0));
        assert_eq!(super::full_span(900.0, 600.0), (900.0, 600.0));
    }

    #[test]
    fn resizing_crosses_between_widget_and_full_with_a_gap() {
        assert!(!super::enters_full(900.0, 800.0));
        assert!(!super::enters_full(1200.0, 680.0));
        assert!(super::enters_full(1080.0, 700.0));
        assert!(!super::leaves_full(1080.0, 700.0));
        assert!(!super::leaves_full(1000.0, 660.0));
        assert!(super::leaves_full(959.0, 800.0));
        assert!(super::leaves_full(1200.0, 639.0));
    }
}

#[cfg(windows)]
pub(crate) fn place_window(window: &slint::Window, width: f32, height: f32, at: Option<(i32, i32)>) {
    let scale = window.scale_factor().max(1.0);
    let pixel_w = (width * scale).round() as i32;
    let pixel_h = (height * scale).round() as i32;
    let work = at
        .map(|(x, y)| work_area_at(x, y, pixel_w, pixel_h))
        .unwrap_or_else(|| work_area_at(0, 0, 1, 1));
    let (left, top, right, bottom) = work;
    let (x, y) = at.unwrap_or_else(|| {
        (left + (right - left - pixel_w).max(0) / 2, top + (bottom - top - pixel_h).max(0) / 2)
    });
    let (x, y) = fit_on_screen(x, y, pixel_w, pixel_h, pixel_w, pixel_h, work);
    window.set_size(slint::LogicalSize::new(width, height));
    window.set_position(slint::PhysicalPosition::new(x, y));
    // Crossing to another-DPI monitor can resize the native window immediately.
    let _ = windows::fit_current_window();
}

pub(crate) fn default_main_size(scale: f32) -> (f32, f32) {
    let scale = scale.max(1.0);
    let (screen_w, screen_h) = primary_screen();
    let logical_w = screen_w as f32 / scale;
    let logical_h = screen_h as f32 / scale;
    let width = (logical_w * 0.34).clamp(480.0, 780.0).min((logical_w - 48.0).max(420.0));
    let height = (logical_h * 0.74).clamp(640.0, 1000.0).min((logical_h - 64.0).max(560.0));
    (width, height)
}

pub(crate) fn default_icon_size() -> i32 {
    1
}

pub(crate) fn remember_window(window: &slint::Window) {
    let (width, height) = window_logical(window);
    if width < 280.0 || height < 360.0 {
        return;
    }
    let pos = window.position();
    let mut pref = db::load_window().unwrap_or(db::WindowPref::widget(width, height, pos.x, pos.y));
    pref.width = width;
    pref.height = height;
    pref.x = pos.x;
    pref.y = pos.y;
    db::save_window(&pref);
}

pub(crate) fn full_span(avail_w: f32, avail_h: f32) -> (f32, f32) {
    (1280.0_f32.min(avail_w.max(1.0)), 800.0_f32.min(avail_h.max(1.0)))
}

/// A widget this large should open as the full window.
pub(crate) fn enters_full(width: f32, height: f32) -> bool {
    width >= 1080.0 && height >= 700.0
}

/// A full window this small should drop back to the widget.
pub(crate) fn leaves_full(width: f32, height: f32) -> bool {
    width < 960.0 || height < 640.0
}

pub(crate) fn full_window_size(scale: f32) -> (f32, f32) {
    let scale = scale.max(1.0);
    #[cfg(target_os = "macos")]
    let (left, top, right, bottom) = work_area_at(0, 0, 1, 1);
    #[cfg(windows)]
    let (left, top, right, bottom) = windows::current_work_area();
    let logical_w = (right - left).max(1) as f32 / scale;
    let logical_h = (bottom - top).max(1) as f32 / scale;
    full_span((logical_w - 48.0).max(1.0), (logical_h - 48.0).max(1.0))
}

pub(crate) fn place_fitted(window: &slint::Window, width: f32, height: f32, prefer: Option<(i32, i32)>) -> (i32, i32) {
    let pos = window.position();
    let size = window.size();
    let scale = window.scale_factor().max(1.0);
    let new_w = (width * scale).round() as i32;
    let new_h = (height * scale).round() as i32;
    let old_w = (size.width as i32).max(1);
    let old_h = (size.height as i32).max(1);
    let work = work_area_at(pos.x, pos.y, old_w, old_h);
    let work = prefer.map(|(x, y)| work_area_at(x, y, new_w, new_h)).unwrap_or(work);
    let (x, y) = match prefer {
        Some((px, py)) => fit_on_screen(px, py, new_w, new_h, new_w, new_h, work),
        None => fit_on_screen(pos.x, pos.y, old_w, old_h, new_w, new_h, work),
    };
    window.set_position(slint::PhysicalPosition::new(x, y));
    window.set_size(slint::LogicalSize::new(width, height));
    #[cfg(target_os = "macos")]
    {
        // Moving to another display can change the backing scale immediately.
        // Return coordinates in that new scale for the preference writer.
        let position = window.position();
        return (position.x, position.y);
    }
    #[cfg(windows)]
    {
        windows::fit_current_window().unwrap_or((x, y))
    }
}

/// Shift an expanded window back inside `work` (left, top, right, bottom).
/// A dock parked on the right or bottom keeps that edge and grows inward.
pub(crate) fn fit_on_screen(
    x: i32,
    y: i32,
    old_w: i32,
    old_h: i32,
    new_w: i32,
    new_h: i32,
    work: (i32, i32, i32, i32),
) -> (i32, i32) {
    let (left, top, right, bottom) = work;
    let old_w = old_w.max(1);
    let old_h = old_h.max(1);
    let new_w = new_w.max(1);
    let new_h = new_h.max(1);
    let mut nx = x;
    let mut ny = y;
    if nx + new_w > right {
        nx = x + old_w - new_w;
    }
    if ny + new_h > bottom {
        ny = y + old_h - new_h;
    }
    let max_x = (right - new_w).max(left);
    let max_y = (bottom - new_h).max(top);
    (nx.clamp(left, max_x), ny.clamp(top, max_y))
}


#[cfg(target_os = "macos")]
pub(crate) fn place_window(window: &slint::Window, width: f32, height: f32, at: Option<(i32, i32)>) {
    let scale = window.scale_factor().max(1.0);
    let pixel_w = (width * scale).round() as i32;
    let pixel_h = (height * scale).round() as i32;
    let (x, y) = at.unwrap_or_else(|| {
        let (left, top, right, bottom) = work_area_at(0, 0, 1, 1);
        (left + (right - left - pixel_w).max(0) / 2, top + (bottom - top - pixel_h).max(0) / 2)
    });
    let work = work_area_at(x, y, pixel_w, pixel_h);
    let (x, y) = fit_on_screen(x, y, pixel_w, pixel_h, pixel_w, pixel_h, work);
    window.set_size(slint::LogicalSize::new(width, height));
    window.set_position(slint::PhysicalPosition::new(x, y));
}
