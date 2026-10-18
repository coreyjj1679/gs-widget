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

// Match the Windows SDK's MSG layout; its lPrivate field is _MAC-only.
const _: () = assert!(std::mem::size_of::<Msg>() == if cfg!(target_pointer_width = "64") { 48 } else { 28 });

pub fn set_enabled(on: bool) {
    // Pair with the worker's THREAD store followed by ENABLED load. A single
    // total order prevents both sides from observing the pre-startup value.
    ENABLED.store(on, Ordering::SeqCst);
    let id = THREAD.load(Ordering::SeqCst);
    if id != 0 {
        unsafe {
            PostThreadMessageW(id, WM_APP, 0, 0);
        }
    }
}

pub fn spawn(tx: Sender<i32>, enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
    thread::spawn(move || unsafe {
        // PostThreadMessage fails until this thread has a message queue.
        // Create it before publishing THREAD so a settings change cannot be lost.
        let mut msg = std::mem::zeroed::<Msg>();
        PeekMessageW(&mut msg, 0, WM_APP, WM_APP, 0); // PM_NOREMOVE
        THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
        let mut registered = false;
        loop {
            let want = ENABLED.load(Ordering::SeqCst);
            if want != registered {
                for id in 1..=9 {
                    if want {
                        if RegisterHotKey(0, id, MOD_NOREPEAT, VK_F1 + id as u32 - 1) == 0 {
                            eprintln!("Unable to register F{id} on Windows: {}", std::io::Error::last_os_error());
                        }
                    } else {
                        UnregisterHotKey(0, id);
                    }
                }
                registered = want;
            }
            let status = GetMessageW(&mut msg, 0, 0, 0);
            if status == 0 || status == -1 {
                break;
            }
            if msg.message == WM_HOTKEY && ENABLED.load(Ordering::SeqCst) {
                let id = msg.wparam as i32;
                if (1..=9).contains(&id) {
                    crate::window::focus_window();
                    let _ = tx.send(id);
                }
            }
        }
        THREAD.store(0, Ordering::SeqCst);
    });
}

#[link(name = "user32")]
extern "system" {
    fn RegisterHotKey(hwnd: isize, id: i32, modifiers: u32, vk: u32) -> i32;
    fn UnregisterHotKey(hwnd: isize, id: i32) -> i32;
    fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
    fn PeekMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32, remove: u32) -> i32;
    fn PostThreadMessageW(id: u32, msg: u32, wparam: usize, lparam: isize) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
}
