//! Keeps networking, archive verification and the helper handshake off Slint's UI thread.
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode};

use crate::{update, App};

// Values are the presentation states documented by App.update-state.
const CHECKING: i32 = 1;
const CURRENT: i32 = 2;
const AVAILABLE: i32 = 3;
const DOWNLOADING: i32 = 4;
const READY: i32 = 5;
const ERROR: i32 = 6;
const RESTARTING: i32 = 7;

enum Event {
    Checked(Result<Option<update::Release>, String>),
    Progress(i32),
    Downloaded(Result<update::PreparedUpdate, String>),
    Installer(Result<(), String>, update::PreparedUpdate),
}

#[derive(Default)]
struct Session {
    release: Option<update::Release>,
    prepared: Option<update::PreparedUpdate>,
}

/// The returned timer must remain alive for the lifetime of the window.
pub fn install(app: &App) -> Timer {
    app.set_app_version(env!("CARGO_PKG_VERSION").into());
    app.set_updates_supported(update::supported());
    let session = Rc::new(RefCell::new(Session::default()));
    let (tx, rx) = mpsc::channel::<Event>();
    let weak = app.as_weak();
    let actions = Rc::clone(&session);
    app.on_request_update(move || {
        let Some(app) = weak.upgrade() else { return };
        if !update::supported() || matches!(app.get_update_state(), CHECKING | DOWNLOADING | RESTARTING) {
            return;
        }
        app.set_update_error("".into());
        let mut session = actions.borrow_mut();
        if session.prepared.is_some() && !app.invoke_prepare_update() {
            return;
        }
        if let Some(prepared) = session.prepared.take() {
            app.set_update_state(RESTARTING);
            let tx = tx.clone();
            std::thread::spawn(move || {
                let result = update::launch_installer(&prepared);
                let _ = tx.send(Event::Installer(result, prepared));
            });
        } else if let Some(release) = session.release.take() {
            app.set_update_state(DOWNLOADING);
            app.set_update_progress(0);
            let tx = tx.clone();
            std::thread::spawn(move || {
                let progress = tx.clone();
                let result = update::download(&release, move |received, total| {
                    let percent = if total > 0 { ((received as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as i32 } else { 0 };
                    let _ = progress.send(Event::Progress(percent));
                });
                let _ = tx.send(Event::Downloaded(result));
            });
        } else {
            app.set_update_state(CHECKING);
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Event::Checked(update::check()));
            });
        }
    });

    let timer = Timer::default();
    let weak = app.as_weak();
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(app) = weak.upgrade() else { return };
        // Coalesce progress into one repaint, while preserving completion ordering.
        for event in rx.try_iter() {
            match event {
                Event::Checked(Ok(Some(release))) => {
                    app.set_update_version(release.version.clone().into());
                    session.borrow_mut().release = Some(release);
                    app.set_update_state(AVAILABLE);
                }
                Event::Checked(Ok(None)) => app.set_update_state(CURRENT),
                Event::Checked(Err(message)) | Event::Downloaded(Err(message)) => {
                    app.set_update_error(message.into());
                    app.set_update_state(ERROR);
                }
                Event::Progress(percent) => app.set_update_progress(percent),
                Event::Downloaded(Ok(prepared)) => {
                    session.borrow_mut().prepared = Some(prepared);
                    app.set_update_state(READY);
                }
                Event::Installer(Ok(()), prepared) => {
                    // Keep staging alive until the helper has taken over. Use the regular
                    // close callback so the window layout is saved before exiting.
                    session.borrow_mut().prepared = Some(prepared);
                    app.invoke_request_close();
                }
                Event::Installer(Err(message), prepared) => {
                    session.borrow_mut().prepared = Some(prepared);
                    app.set_update_error(message.into());
                    app.set_update_state(READY);
                }
            }
        }
    });
    timer
}
