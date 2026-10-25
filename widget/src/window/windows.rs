fn widget_handle() -> isize {
    // Winit 0.30.13 (Cargo.lock) uses "Window Class" for application windows
    // (platform_impl/windows/mod.rs), while its event-loop helper uses
    // "Winit Thread Event Target" (event_loop.rs) and may also be visible.
    // Select the process's unowned application window, never its translated
    // caption. Do not cache the HWND: hiding/recreating a Slint window can
    // invalidate it. Minimized windows remain visible and can still be restored.
    let class: Vec<u16> = "Window Class\0".encode_utf16().collect();
    widget_handle_for_class(unsafe { GetCurrentProcessId() }, &class)
}

fn widget_handle_for_class(process: u32, class: &[u16]) -> isize {
    unsafe {
        let mut hwnd = 0;
        let mut found = 0;
        loop {
            hwnd = FindWindowExW(0, hwnd, class.as_ptr(), std::ptr::null());
            if hwnd == 0 {
                return found;
            }
            let mut owner = 0;
            GetWindowThreadProcessId(hwnd, &mut owner);
            if owner != process || IsWindowVisible(hwnd) == 0 || GetWindow(hwnd, 4) != 0 {
                continue; // GW_OWNER: exclude owned dialogs and popups.
            }
            let style = GetWindowLongW(hwnd, -16) as u32; // GWL_STYLE
            let extended_style = GetWindowLongW(hwnd, -20) as u32; // GWL_EXSTYLE
            if style & 0x4000_0000 != 0 || extended_style & 0x0000_0080 != 0 {
                continue; // WS_CHILD / WS_EX_TOOLWINDOW
            }
            if found != 0 {
                // An unexpected second application window is ambiguous. Do not
                // focus or attach clipboard ownership to an arbitrary window.
                return 0;
            }
            found = hwnd;
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
    fn IsWindowVisible(hwnd: isize) -> i32;
    fn GetWindow(hwnd: isize, command: u32) -> isize;
    fn GetWindowLongW(hwnd: isize, index: i32) -> i32;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    struct WindowClass {
        style: u32,
        window_proc: Option<unsafe extern "system" fn(isize, u32, usize, isize) -> isize>,
        class_extra: i32,
        window_extra: i32,
        instance: isize,
        icon: isize,
        cursor: isize,
        background: isize,
        menu_name: *const u16,
        class_name: *const u16,
    }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterClassW(class: *const WindowClass) -> u16;
        fn UnregisterClassW(class: *const u16, instance: isize) -> i32;
        fn DefWindowProcW(hwnd: isize, message: u32, wparam: usize, lparam: isize) -> isize;
        fn CreateWindowExW(
            extended_style: u32, class: *const u16, title: *const u16, style: u32,
            x: i32, y: i32, width: i32, height: i32, parent: isize,
            menu: isize, instance: isize, parameter: *mut std::ffi::c_void,
        ) -> isize;
        fn DestroyWindow(hwnd: isize) -> i32;
        fn SetWindowTextW(hwnd: isize, title: *const u16) -> i32;
        fn ShowWindow(hwnd: isize, command: i32) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(module: *const u16) -> isize;
    }

    struct NativeWindows {
        instance: isize,
        classes: Vec<Vec<u16>>,
        windows: Vec<isize>,
    }

    impl NativeWindows {
        fn new() -> Self {
            let mut fixture = Self {
                instance: unsafe { GetModuleHandleW(std::ptr::null()) },
                classes: Vec::new(), windows: Vec::new(),
            };
            // A private class avoids interfering with real Slint windows from
            // parallel tests or calling Winit's window procedure without its
            // required creation data. Both classes exercise the same HWND finder.
            for suffix in ["application", "event-loop"] {
                let name: Vec<u16> = format!("Gersang window lookup test {} {suffix}\0", unsafe { GetCurrentProcessId() })
                    .encode_utf16().collect();
                let class = WindowClass {
                    style: 0, window_proc: Some(DefWindowProcW), class_extra: 0,
                    window_extra: 0, instance: fixture.instance, icon: 0,
                    cursor: 0, background: 0, menu_name: std::ptr::null(),
                    class_name: name.as_ptr(),
                };
                assert_ne!(unsafe { RegisterClassW(&class) }, 0, "register native fixture class");
                fixture.classes.push(name);
            }
            fixture
        }

        fn create(&mut self, class: usize, title: &str, style: u32, extended_style: u32, owner: isize) -> isize {
            let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            let hwnd = unsafe {
                CreateWindowExW(extended_style, self.classes[class].as_ptr(), title.as_ptr(),
                    style, 8, 8, 160, 100, owner, 0, self.instance, std::ptr::null_mut())
            };
            assert_ne!(hwnd, 0, "create native fixture window");
            self.windows.push(hwnd);
            hwnd
        }

        fn find(&self) -> isize {
            widget_handle_for_class(unsafe { GetCurrentProcessId() }, &self.classes[0])
        }
    }

    impl Drop for NativeWindows {
        fn drop(&mut self) {
            unsafe {
                for hwnd in self.windows.iter().rev() { DestroyWindow(*hwnd); }
                for class in &self.classes { UnregisterClassW(class.as_ptr(), self.instance); }
            }
        }
    }

    #[test]
    fn native_lookup_ignores_translated_titles_and_excludes_auxiliary_windows() {
        const VISIBLE: u32 = 0x1000_0000;
        const OVERLAPPED: u32 = 0x00cf_0000;
        const CHILD: u32 = 0x4000_0000;
        const TOOL_WINDOW: u32 = 0x0000_0080;
        const NO_ACTIVATE: u32 = 0x0800_0000;
        let mut fixture = NativeWindows::new();
        fixture.create(0, "hidden main class", OVERLAPPED, NO_ACTIVATE, 0);
        fixture.create(0, "visible tool", VISIBLE | OVERLAPPED, TOOL_WINDOW | NO_ACTIVATE, 0);
        fixture.create(1, "", VISIBLE | OVERLAPPED, NO_ACTIVATE, 0);
        assert_eq!(fixture.find(), 0, "hidden, tool and event-loop windows are not the app");

        let main = fixture.create(0, "Gersang", VISIBLE | OVERLAPPED, NO_ACTIVATE, 0);
        fixture.create(0, "owned dialog", VISIBLE | OVERLAPPED, NO_ACTIVATE, main);
        fixture.create(0, "child", VISIBLE | CHILD, NO_ACTIVATE, main);
        for title in ["Gersang", "巨商", "거상", "", "A future translated title"] {
            let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            assert_ne!(unsafe { SetWindowTextW(main, wide.as_ptr()) }, 0);
            assert_eq!(fixture.find(), main, "title: {title}");
        }
        assert_eq!(widget_handle_for_class(0, &fixture.classes[0]), 0, "another process cannot match");
        unsafe { ShowWindow(main, 7); } // SW_SHOWMINNOACTIVE
        assert_ne!(unsafe { IsIconic(main) }, 0);
        assert_eq!(fixture.find(), main, "minimized app remains available for hotkey restore");

        fixture.create(0, "second app window", VISIBLE | OVERLAPPED, NO_ACTIVATE, 0);
        assert_eq!(fixture.find(), 0, "ambiguous application windows must not be chosen by Z order");
        let second = fixture.windows.pop().unwrap();
        assert_ne!(unsafe { DestroyWindow(second) }, 0);
        assert_eq!(fixture.find(), main, "no cached handle survives the removed window");
    }
}
