fn widget_handle() -> isize {
    let title: Vec<u16> = "巨商\0".encode_utf16().collect();
    unsafe {
        let process = GetCurrentProcessId();
        let mut hwnd = 0;
        loop {
            hwnd = FindWindowExW(0, hwnd, std::ptr::null(), title.as_ptr());
            if hwnd == 0 {
                return 0;
            }
            let mut owner = 0;
            GetWindowThreadProcessId(hwnd, &mut owner);
            if owner == process {
                return hwnd;
            }
        }
    }
}

pub(crate) fn focus_window() {
    unsafe {
        let hwnd = widget_handle();
        if hwnd != 0 {
            if IsIconic(hwnd) != 0 {
                // This can be called by the hotkey thread; do not block that
                // thread while the UI thread handles restoring the window.
                ShowWindowAsync(hwnd, 9); // SW_RESTORE
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

pub(crate) fn open_source_repository() {
    let url: Vec<u16> = super::SOURCE_REPOSITORY_URL.encode_utf16().chain(Some(0)).collect();
    let operation: Vec<u16> = "open\0".encode_utf16().collect();
    let result = unsafe {
        ShellExecuteW(
            widget_handle(),
            operation.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1, // SW_SHOWNORMAL
        )
    };
    if result <= 32 {
        eprintln!("無法開啟 GitHub 原始碼：ShellExecuteW 錯誤 {result}");
    }
}

pub(super) fn current_work_area() -> (i32, i32, i32, i32) {
    let hwnd = widget_handle();
    let mut rect = WinRect::default();
    if hwnd != 0 && unsafe { GetWindowRect(hwnd, &mut rect) } != 0 {
        return work_area_at(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);
    }
    work_area_at(0, 0, 1, 1)
}

/// Fit after a move/resize, using the native size after any synchronous
/// WM_DPICHANGED processing rather than the previous display's scale.
pub(super) fn fit_current_window() -> Option<(i32, i32)> {
    let hwnd = widget_handle();
    let mut rect = WinRect::default();
    if hwnd == 0 || unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let work = work_area_at(rect.left, rect.top, width, height);
    let (x, y) = super::fit_on_screen(rect.left, rect.top, width, height, width, height, work);
    if (x, y) != (rect.left, rect.top) {
        // SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE: preserve window level
        // and keyboard focus while bringing the frame into the work area.
        if unsafe { SetWindowPos(hwnd, 0, x, y, 0, 0, 0x0015) } == 0 {
            return Some((rect.left, rect.top));
        }
    }
    Some((x, y))
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
    fn FindWindowExW(parent: isize, after: isize, class: *const u16, window: *const u16) -> isize;
    fn GetWindowThreadProcessId(hwnd: isize, process_id: *mut u32) -> u32;
    fn GetWindowRect(hwnd: isize, rect: *mut WinRect) -> i32;
    fn SetWindowPos(hwnd: isize, after: isize, x: i32, y: i32, width: i32, height: i32, flags: u32) -> i32;
    fn IsIconic(hwnd: isize) -> i32;
    fn ShowWindowAsync(hwnd: isize, command: i32) -> i32;
    fn SetForegroundWindow(hwnd: isize) -> i32;
    fn OpenClipboard(hwnd: isize) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, mem: isize) -> isize;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalLock(mem: isize) -> *mut u16;
    fn GlobalUnlock(mem: isize) -> i32;
    fn GlobalFree(mem: isize) -> isize;
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(
        hwnd: isize,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show_command: i32,
    ) -> isize;
}

pub(crate) fn try_copy_to_clipboard(text: &str) -> bool {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let bytes = wide.len() * std::mem::size_of::<u16>();
    unsafe {
        // EmptyClipboard needs a real owner; opening with NULL makes the
        // subsequent SetClipboardData fail according to the Win32 contract.
        let hwnd = widget_handle();
        if hwnd == 0 || OpenClipboard(hwnd) == 0 {
            return false;
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return false;
        }
        let mem = GlobalAlloc(0x0002, bytes);
        let mut copied = false;
        if mem != 0 {
            let ptr = GlobalLock(mem);
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                GlobalUnlock(mem);
                if SetClipboardData(13, mem) == 0 {
                    GlobalFree(mem);
                } else {
                    copied = true;
                }
            } else {
                GlobalFree(mem);
            }
        }
        CloseClipboard();
        copied
    }
}
