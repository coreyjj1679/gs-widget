//! Full-mode quick search: groups of matching records with category headings.
use std::rc::Rc;
use std::sync::atomic::Ordering;

use slint::{ComponentHandle, ModelRc, VecModel};

use super::db;
use super::sheet::link_rows;
use super::{App, Session};

/// Categories whose own search page can list every match.
const LISTABLE: &[&str] = &["item", "monster", "hero", "quest", "npc"];

fn row(title: String, key: String, aside: String, chip: &str) -> db::Hit {
    db::Hit { title, subtitle: String::new(), key, icon: None, solo: false, badge: None, aside, chip: chip.into() }
}

pub(crate) fn install(app: &App, catalog: Rc<db::Catalog>, session: Session) {
    let weak = app.as_weak();
    app.on_quick_search(move |query| {
        let Some(app) = weak.upgrade() else { return };
        let lang = app.get_lang().to_string();
        let mut hits = Vec::new();
        for group in catalog.quick_search(&query, &lang) {
            hits.push(row(group.title, String::new(), group.total.to_string(), "label"));
            hits.extend(group.hits);
            if group.total > db::QUICK_ROWS && LISTABLE.contains(&group.kind) {
                let title = format!(
                    "{}{}{}",
                    db::ui_text(&lang, "查看全部 ", "Show all "),
                    group.total,
                    db::ui_text(&lang, " 筆", " results"),
                );
                hits.push(row(title, format!("quick-all:{}", group.kind), "→".into(), ""));
            }
        }
        // Use the current generation so an open record's pending icons stay valid.
        let gen = session.link_generation.load(Ordering::Relaxed);
        let (rows, jobs) = link_rows(&session.cache, hits, gen, &catalog, &lang);
        let selectable: Vec<i32> = rows.iter().enumerate()
            .filter(|(_, row)| row.chip.as_str() != "label")
            .map(|(index, _)| i32::try_from(index).unwrap_or(i32::MAX))
            .collect();
        app.set_quick_results(ModelRc::new(VecModel::from(rows)));
        app.set_quick_selectable(ModelRc::new(VecModel::from(selectable)));
        app.set_quick_pick(0);
        for job in jobs {
            let _ = session.jobs.send(job);
        }
    });
}
