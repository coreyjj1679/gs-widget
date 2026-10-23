//! Full-mode quick search across categories. It reuses the ranked category searches
//! and the reviewed nickname glossary; nicknames resolve to their catalog records.
use std::collections::HashSet;

use super::{terminology, ui_text, Catalog, Hit};

/// Records shown per category; the category search lists the rest.
pub const QUICK_ROWS: usize = 5;
const NICKNAME_ROWS: usize = 8;

pub struct QuickGroup {
    /// `nickname`, or the category whose search lists every match.
    pub kind: &'static str,
    pub title: String,
    pub total: usize,
    pub hits: Vec<Hit>,
}

impl Catalog {
    pub fn quick_search(&self, query: &str, lang: &str) -> Vec<QuickGroup> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        let mut shown = HashSet::new();
        let mut groups = Vec::new();
        let nicknames = self.nickname_hits(query, lang);
        if !nicknames.is_empty() {
            shown.extend(nicknames.iter().map(|hit| hit.key.clone()));
            groups.push(QuickGroup {
                kind: "nickname",
                title: ui_text(lang, "俗稱", "Nicknames"),
                total: nicknames.len(),
                hits: nicknames,
            });
        }
        let searches = [
            ("item", ui_text(lang, "道具", "Items"), self.search_scoped("item", query, lang, "", "", "", "")),
            ("monster", ui_text(lang, "怪物", "Monsters"), self.monster_records(query, lang, "", "", "")),
            ("hero", ui_text(lang, "角色", "Roster"), self.search_scoped("hero", query, lang, "", "", "", "")),
            ("quest", ui_text(lang, "任務", "Quests"), self.search_scoped("quest", query, lang, "", "", "", "")),
            ("npc", ui_text(lang, "人物", "NPCs"), self.search_scoped("npc", query, lang, "", "", "", "")),
            ("set", ui_text(lang, "套裝", "Sets"), self.search_scoped("item", query, lang, "set", "", "", "")),
        ];
        for (kind, title, hits) in searches {
            let total = hits.len();
            let hits: Vec<Hit> = hits.into_iter().filter(|hit| !shown.contains(&hit.key)).take(QUICK_ROWS).collect();
            if hits.is_empty() {
                continue;
            }
            shown.extend(hits.iter().map(|hit| hit.key.clone()));
            groups.push(QuickGroup { kind, title, total, hits });
        }
        groups
    }

    /// Reviewed community nicknames, shown with the official record they name.
    fn nickname_hits(&self, query: &str, lang: &str) -> Vec<Hit> {
        let book = self.mcp_terminology.as_ref().unwrap_or_else(|| terminology::glossary());
        let mut seen = HashSet::new();
        let mut hits = Vec::new();
        for found in terminology::resolve_terms_with(book, query).entities {
            for key in &found.keys {
                if hits.len() == NICKNAME_ROWS || !seen.insert(key.clone()) {
                    continue;
                }
                if let Some(mut hit) = self.record_hit(key, lang) {
                    hit.aside = format!("「{}」", found.matched_alias);
                    hits.push(hit);
                }
            }
        }
        hits
    }

    /// A record formatted as in its own category list, or None when it is not catalogued.
    fn record_hit(&self, key: &str, lang: &str) -> Option<Hit> {
        let (kind, id) = key.split_once(':')?;
        let folded = id.to_lowercase();
        let all = usize::MAX;
        let rows = match kind {
            "item" => self.index.item_hits(id, &folded, lang, "", "", all),
            "monster" => self.index.monster_hits(id, &folded, lang, &[], None),
            "hero" => self.index.hero_hits(id, &folded, lang, "", "", all),
            "npc" => self.index.npc_hits(id, &folded, lang, all),
            "set" => self.index.set_hits(id, &folded, lang, ""),
            "quest" => {
                let code = &self.index.quests.iter().find(|row| row.id.to_string() == id)?.code;
                self.index.quest_hits(code, &code.to_lowercase(), lang, "", "", all)
            }
            _ => return None,
        };
        rows.into_iter().find(|hit| hit.key == key)
    }
}
