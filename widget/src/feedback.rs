//! User-visible feedback shared by the native app's otherwise independent flows.
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode};

use crate::{db, App};

pub(crate) fn text(lang: &str, zh: &str, en: &str, ko: &str) -> String {
    match db::primary_lang(lang) {
        "en" => en,
        "ko" => ko,
        _ => zh,
    }.into()
}

pub(crate) fn error_for(app: &App, source: &str, zh: &str, en: &str, ko: &str) {
    app.set_feedback_source(source.into());
    app.set_feedback_message(text(&app.get_lang(), zh, en, ko).into());
    app.set_feedback_error(true);
    app.set_feedback_retry(false);
}

pub(crate) fn success(app: &App, zh: &str, en: &str, ko: &str) {
    // A successful unrelated action must not erase an unresolved error.
    if app.get_feedback_error() && !app.get_feedback_message().is_empty() { return; }
    app.set_feedback_message(text(&app.get_lang(), zh, en, ko).into());
    app.set_feedback_error(false);
    app.set_feedback_retry(false);
}

pub(crate) fn success_for(app: &App, source: &str, zh: &str, en: &str, ko: &str) {
    if app.get_feedback_source() == source {
        app.set_feedback_error(false);
    }
    success(app, zh, en, ko);
    if !db::user_data::pending_preference_failures().is_empty() {
        preference_error(app);
    }
}

fn preference_error(app: &App) {
    error_for(app, "preferences",
        "設定未能儲存，關閉程式後可能遺失。請確認資料夾可寫入及磁碟空間，再按「重試儲存」。",
        "Settings could not be saved and may be lost when you close the app. Check folder permissions and free disk space, then retry saving.",
        "설정을 저장하지 못했습니다. 앱을 닫으면 변경 사항이 사라질 수 있습니다. 폴더 권한과 디스크 공간을 확인한 후 다시 저장하세요.");
    app.set_feedback_retry(true);
}

/// Poll only the small in-memory failure queue, never the filesystem.
pub(crate) fn install(app: &App) -> Timer {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let weak = app.as_weak();
    let retry_seen = Rc::clone(&seen);
    app.on_retry_preferences(move || {
        let Some(app) = weak.upgrade() else { return };
        let failures = db::user_data::retry_failed_preferences();
        *retry_seen.borrow_mut() = failures.clone();
        if failures.is_empty() {
            app.set_feedback_message("".into());
            app.set_feedback_error(false);
            success(&app, "設定已儲存。", "Settings saved.", "설정을 저장했습니다.");
        } else {
            preference_error(&app);
        }
    });
    let weak = app.as_weak();
    app.on_dismiss_feedback(move || {
        if let Some(app) = weak.upgrade() {
            if app.get_feedback_source() != "preferences"
                && !db::user_data::pending_preference_failures().is_empty() {
                preference_error(&app);
                return;
            }
            app.set_feedback_message("".into());
            app.set_feedback_error(false);
            app.set_feedback_retry(false);
        }
    });
    let timer = Timer::default();
    let weak = app.as_weak();
    timer.start(TimerMode::Repeated, Duration::from_millis(300), move || {
        let Some(app) = weak.upgrade() else { return };
        let failures = db::user_data::pending_preference_failures();
        if *seen.borrow() != failures {
            *seen.borrow_mut() = failures.clone();
            if !failures.is_empty() {
                preference_error(&app);
            } else if app.get_feedback_retry() {
                app.set_feedback_message("".into());
                app.set_feedback_error(false);
                app.set_feedback_retry(false);
            }
        }
    });
    timer
}
