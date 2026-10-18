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
