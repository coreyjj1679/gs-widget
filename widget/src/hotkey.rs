use std::sync::mpsc::Sender;
use std::thread;

const MOD_NOREPEAT: u32 = 0x4000;
const WM_HOTKEY: u32 = 0x0312;
const VK_F1: u32 = 0x70;

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

pub fn spawn(tx: Sender<i32>) {
    thread::spawn(move || unsafe {
        for id in 1..=9 {
            RegisterHotKey(0, id, MOD_NOREPEAT, VK_F1 + id as u32 - 1);
        }
        loop {
            let mut msg = std::mem::zeroed::<Msg>();
            let status = GetMessageW(&mut msg, 0, 0, 0);
            if status == 0 || status == -1 {
                break;
            }
            if msg.message == WM_HOTKEY {
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
    fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
    fn FindWindowW(class: *const u16, window: *const u16) -> isize;
    fn SetForegroundWindow(hwnd: isize) -> i32;
}
