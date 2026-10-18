use global_hotkey::{
    hotkey::{Code, HotKey},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use objc2::MainThreadMarker;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::mpsc::Sender;

static ENABLED: AtomicBool = AtomicBool::new(false);
static REGISTERED: AtomicU16 = AtomicU16::new(0);
static PRESSED: AtomicU16 = AtomicU16::new(0);

const KEYS: [Code; 9] = [
    Code::F1,
    Code::F2,
    Code::F3,
    Code::F4,
    Code::F5,
    Code::F6,
    Code::F7,
    Code::F8,
    Code::F9,
];

struct Hotkeys {
    manager: GlobalHotKeyManager,
    keys: [HotKey; 9],
    registered: [bool; 9],
}

thread_local! {
    // Carbon hotkeys use the main thread's existing Slint/AppKit event loop.
    // Retain the manager for that thread's lifetime so registrations stay alive.
    static HOTKEYS: RefCell<Option<Hotkeys>> = const { RefCell::new(None) };
}

impl Hotkeys {
    fn set_enabled(&mut self, enabled: bool) {
        ENABLED.store(enabled, Ordering::Relaxed);
        PRESSED.store(0, Ordering::Relaxed);
        for (index, key) in self.keys.iter().copied().enumerate() {
            if self.registered[index] == enabled {
                continue;
            }
            let bit = 1 << index;
            if enabled {
                match self.manager.register(key) {
                    Ok(()) => {
                        self.registered[index] = true;
                        REGISTERED.fetch_or(bit, Ordering::Relaxed);
                    }
                    Err(error) => eprintln!(
                        "Unable to register F{} on macOS: {error}. Check system keyboard shortcuts or other applications.",
                        index + 1
                    ),
                }
            } else {
                // Stop dispatching immediately, even if the OS unregister fails.
                REGISTERED.fetch_and(!bit, Ordering::Relaxed);
                if let Err(error) = self.manager.unregister(key) {
                    eprintln!("Unable to unregister F{} on macOS: {error}", index + 1);
                }
                self.registered[index] = false;
            }
        }
    }
}

/// Called by the settings callback on the main/UI thread.
pub fn set_enabled(enabled: bool) {
    if MainThreadMarker::new().is_none() {
        eprintln!("macOS global hotkeys must be changed on the main thread");
        return;
    }
    HOTKEYS.with_borrow_mut(|hotkeys| {
        if let Some(hotkeys) = hotkeys {
            hotkeys.set_enabled(enabled);
        }
    });
}

/// Initialize on the main thread before entering Slint's event loop.
/// The name matches the Windows backend; macOS must not spawn a worker loop.
pub fn spawn(tx: Sender<i32>, enabled: bool) {
    if MainThreadMarker::new().is_none() {
        eprintln!("macOS global hotkeys must be initialized on the main thread");
        return;
    }
    HOTKEYS.with_borrow_mut(|hotkeys| {
        if hotkeys.is_some() {
            return;
        }
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => manager,
            Err(error) => {
                eprintln!("Unable to initialize macOS global hotkeys: {error}");
                return;
            }
        };
        let keys = KEYS.map(|code| HotKey::new(None, code));
        let ids = keys.map(|key| key.id());
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            let Some(index) = ids.iter().position(|id| *id == event.id) else {
                return;
            };
            let bit = 1 << index;
            if event.state == HotKeyState::Released {
                PRESSED.fetch_and(!bit, Ordering::Relaxed);
                return;
            }
            if !ENABLED.load(Ordering::Relaxed)
                || REGISTERED.load(Ordering::Relaxed) & bit == 0
                || PRESSED.fetch_or(bit, Ordering::Relaxed) & bit != 0
            {
                return;
            }
            let tx = tx.clone();
            // AppKit window activation and UI updates stay on Slint's UI thread.
            let _ = slint::invoke_from_event_loop(move || {
                if ENABLED.load(Ordering::Relaxed) && REGISTERED.load(Ordering::Relaxed) & bit != 0
                {
                    crate::window::focus_window();
                    let _ = tx.send(index as i32 + 1);
                }
            });
        }));
        let mut state = Hotkeys {
            manager,
            keys,
            registered: [false; 9],
        };
        state.set_enabled(enabled);
        *hotkeys = Some(state);
    });
}
