use super::db;
use super::window_logical;

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
}

pub(crate) fn place_window(window: &slint::Window, width: f32, height: f32, at: Option<(i32, i32)>) {
    let scale = window.scale_factor().max(1.0);
    let (screen_w, screen_h) = primary_screen();
    let pixel_w = (width * scale).round() as i32;
    let pixel_h = (height * scale).round() as i32;
    let (x, y) = match at {
        Some((x, y)) if x > -80 && y > -40 && x + 120 < screen_w && y + 80 < screen_h => (x, y),
        _ => ((screen_w - pixel_w).max(0) / 2, (screen_h - pixel_h).max(0) / 2),
    };
    window.set_size(slint::LogicalSize::new(width, height));
    window.set_position(slint::PhysicalPosition::new(x, y));
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
    db::save_window(&db::WindowPref {
        width,
        height,
        x: pos.x,
        y: pos.y,
    });
}

pub(crate) fn focus_window() {
    let title: Vec<u16> = "巨商\0".encode_utf16().collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd != 0 {
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

pub(crate) fn primary_screen() -> (i32, i32) {
    unsafe { (GetSystemMetrics(0), GetSystemMetrics(1)) }
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

pub(crate) fn work_area_at(x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
    let point = WinPoint {
        x: x.saturating_add(w / 2),
        y: y.saturating_add(h / 2),
    };
    unsafe {
        let monitor = MonitorFromPoint(point, 2);
        let mut info = MonitorInfo {
            cb_size: std::mem::size_of::<MonitorInfo>() as u32,
            rc_monitor: WinRect::default(),
            rc_work: WinRect::default(),
            dw_flags: 0,
        };
        if monitor != 0 && GetMonitorInfoW(monitor, &mut info) != 0 {
            let area = info.rc_work;
            if area.right > area.left && area.bottom > area.top {
                return (area.left, area.top, area.right, area.bottom);
            }
        }
    }
    let (width, height) = primary_screen();
    (0, 0, width.max(1), height.max(1))
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WinPoint {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct MonitorInfo {
    cb_size: u32,
    rc_monitor: WinRect,
    rc_work: WinRect,
    dw_flags: u32,
}

#[link(name = "user32")]
extern "system" {
    fn GetSystemMetrics(index: i32) -> i32;
    fn MonitorFromPoint(point: WinPoint, flags: u32) -> isize;
    fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
    fn FindWindowW(class: *const u16, window: *const u16) -> isize;
    fn SetForegroundWindow(hwnd: isize) -> i32;
    fn OpenClipboard(hwnd: isize) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, mem: isize) -> isize;
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalLock(mem: isize) -> *mut u16;
    fn GlobalUnlock(mem: isize) -> i32;
    fn GlobalFree(mem: isize) -> isize;
}

pub(crate) fn copy_to_clipboard(text: &str) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let bytes = wide.len() * std::mem::size_of::<u16>();
    unsafe {
        if OpenClipboard(0) == 0 {
            return;
        }
        EmptyClipboard();
        let mem = GlobalAlloc(0x0002, bytes);
        if mem != 0 {
            let ptr = GlobalLock(mem);
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                GlobalUnlock(mem);
                if SetClipboardData(13, mem) == 0 {
                    GlobalFree(mem);
                }
            } else {
                GlobalFree(mem);
            }
        }
        CloseClipboard();
    }
}
