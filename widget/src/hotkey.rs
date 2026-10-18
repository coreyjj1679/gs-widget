use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::thread;

const MOD_NOREPEAT: u32 = 0x4000;
const WM_HOTKEY: u32 = 0x0312;
const WM_APP: u32 = 0x8000;
const VK_F1: u32 = 0x70;

static ENABLED: AtomicBool = AtomicBool::new(true);
static THREAD: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct Msg {
    hwnd: isize,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    pt: Point,
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    let id = THREAD.load(Ordering::Relaxed);
    if id != 0 {
        unsafe {
            PostThreadMessageW(id, WM_APP, 0, 0);
        }
    }
}

pub fn spawn(tx: Sender<i32>, enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    thread::spawn(move || unsafe {
        THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
        let mut registered = false;
        loop {
            let want = ENABLED.load(Ordering::Relaxed);
            if want != registered {
                for id in 1..=9 {
                    if want {
                        RegisterHotKey(0, id, MOD_NOREPEAT, VK_F1 + id as u32 - 1);
                    } else {
                        UnregisterHotKey(0, id);
                    }
                }
                registered = want;
            }
            let mut msg = std::mem::zeroed::<Msg>();
            let status = GetMessageW(&mut msg, 0, 0, 0);
            if status == 0 || status == -1 {
                break;
            }
            if msg.message == WM_HOTKEY && ENABLED.load(Ordering::Relaxed) {
                let id = msg.wparam as i32;
                if (1..=9).contains(&id) {
                    raise_widget();
                    let _ = tx.send(id);
                }
            }
        }
    });
}

unsafe fn raise_widget() {
    let title: Vec<u16> = "巨商\0".encode_utf16().collect();
    let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
    if hwnd != 0 {
        let _ = SetForegroundWindow(hwnd);
    }
}

#[link(name = "user32")]
extern "system" {
    fn RegisterHotKey(hwnd: isize, id: i32, modifiers: u32, vk: u32) -> i32;
    fn UnregisterHotKey(hwnd: isize, id: i32) -> i32;
    fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
    fn PostThreadMessageW(id: u32, msg: u32, wparam: usize, lparam: isize) -> i32;
    fn GetCurrentThreadId() -> u32;
    fn FindWindowW(class: *const u16, window: *const u16) -> isize;
    fn SetForegroundWindow(hwnd: isize) -> i32;
}
