use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rusqlite::Connection;

use super::{Catalog, Hit};
use super::names::haystack;
use super::npc::npc_names;
use super::rows::{QuestLine, QuestRow, QuestStep};
use super::store::column_exists;
use super::text::{caption, chrome_text, commas_text, game_breaks, kind_name, label, lead3, named_lead, numbered, phrase, reward_amount, role_phrase, stack3, step_hint, step_value, trail3, with_count};

pub(in crate::db) const QUEST_TITLE_ROLES: &[&str] = &["title", "subject", "start-subject"];

pub(in crate::db) fn quest_source_rank(source: &str) -> usize {
    ["main", "letter", "intro", "mercenary", "dispatch"]
        .iter()
        .position(|key| *key == source)
        .unwrap_or(9)
}

pub(in crate::db) fn main_type_name(kind: &str, lang: &str) -> Option<String> {
    let key = match kind {
        "0" => "starter",
        "2" => "combat",
        "5" => "repeat-type",
        "7" => "timed",
        _ => return None,
    };
    Some(phrase(key, lang))
}

pub(in crate::db) fn load_quests(conn: &Connection) -> Result<Vec<QuestRow>, String> {
    let lines = load_quest_lines(conn)?;
    let npc_column = column_exists(conn, "quests", "npc_code");
    let sql = if npc_column {
        "SELECT id, source, code, title, detail, kind, npc_code FROM quests ORDER BY id"
    } else {
        "SELECT id, source, code, title, detail, kind, 0 FROM quests ORDER BY id"
    };
    let mut stmt = conn.prepare(sql).map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut quests = Vec::new();
    for row in rows {
        let (id, source, code, title, detail, kind, npc) = row.map_err(|err| err.to_string())?;
        let empty = Vec::new();
        let quest_lines = lines.get(&id).unwrap_or(&empty);
        let (mut title_en, mut title_zh, title_ko) = complete_quest_title(
            conn, &source, &title,
            quest_line(quest_lines, QUEST_TITLE_ROLES, "en").unwrap_or_default(),
            quest_line(quest_lines, QUEST_TITLE_ROLES, "zh").unwrap_or_default(),
        );
        if title_en.is_empty() && title_zh.is_empty() && title_ko.is_empty() {
            title_en = code.clone();
            title_zh = code.clone();
        }
        let mut parts = vec![title.clone(), detail.clone(), title_ko.clone(), title_en.clone(), title_zh.clone()];
        for line in quest_lines {
            for text in line.texts.values() {
                parts.push(text.clone());
            }
        }
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        let names = haystack(&borrowed);
        quests.push(QuestRow {
            title_ko,
            title_en,
            title_zh,
            id,
            source,
            code,
            kind,
            npc,
            names,
        });
    }
    Ok(quests)
}

fn complete_quest_title(conn: &Connection, source: &str, stored: &str, mut en: String, mut zh: String) -> (String, String, String) {
    // Mercenary source titles contain character IDs, not localized names.
    let ko = if source == "mercenary" {
        let (character_en, character_zh, ko): (String, String, String) = conn.query_row(
            "SELECT IFNULL(se.text, ''), IFNULL(sz.text, ''), c.name_ko
             FROM characters c
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND c.name_gts > 0 AND se.id = printf('%08d', c.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND c.name_gts > 0 AND sz.id = printf('%08d', c.name_gts)
             WHERE c.code = ?1",
            [stored], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap_or_else(|_| {
            let ko = conn.query_row("SELECT name_ko FROM characters WHERE code = ?1", [stored], |row| row.get(0)).unwrap_or_default();
            (String::new(), String::new(), ko)
        });
        // A recorded quest translation takes precedence over the character name.
        if en.trim().is_empty() { en = character_en; }
        if zh.trim().is_empty() { zh = character_zh; }
        ko
    } else if stored.trim() == "0" {
        String::new()
    } else {
        game_breaks(stored.to_string())
    };
    (en, zh, ko)
}

pub(in crate::db) fn load_quest_lines(conn: &Connection) -> Result<HashMap<i64, Vec<QuestLine>>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT ql.quest_id, ql.role, ql.slot, IFNULL(s.lang, ''), IFNULL(s.text, '')
             FROM quest_lines ql
             LEFT JOIN strings s ON s.object = ql.text_object AND s.id = printf('%08d', ql.gts_id)
             ORDER BY ql.quest_id, ql.slot",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut lines: HashMap<i64, Vec<QuestLine>> = HashMap::new();
    for row in rows {
        let (quest_id, role, slot, lang, text) = row.map_err(|err| err.to_string())?;
        if lang.is_empty() || text.is_empty() {
            continue;
        }
        let quest = lines.entry(quest_id).or_default();
        if let Some(line) = quest.iter_mut().find(|line| line.slot == slot && line.role == role) {
            line.texts.insert(lang, game_breaks(text));
        } else {
            let mut texts = HashMap::new();
            texts.insert(lang, game_breaks(text));
            quest.push(QuestLine { role, slot, texts });
        }
    }
    Ok(lines)
}

pub(in crate::db) fn quest_line(lines: &[QuestLine], roles: &[&str], lang: &str) -> Option<String> {
    for role in roles {
        let Some(line) = lines.iter().find(|line| line.role == *role) else {
            continue;
        };
        if let Some(text) = line.texts.get(lang).filter(|text| !text.is_empty()) {
            return Some(text.clone());
        }
    }
    None
}

fn named_lead3(lang: &str, en: &str, zh: &str, ko: &str, hint: &str, code: &str) -> (String, String) {
    let title = lead3(lang, en, zh, ko);
    if title.is_empty() { named_lead(lang, en, zh, hint, code) } else { (title, trail3(lang, en, zh, ko)) }
}

impl Catalog {
    pub(in crate::db) fn item_quest_uses(&self, code: i64, lang: &str) -> Vec<Hit> {
        self.item_quests(code, lang, &["requirement", "objective"], "use-quest")
    }

    pub(in crate::db) fn item_quest_gains(&self, code: i64, lang: &str) -> Vec<Hit> {
        self.item_quests(code, lang, &["reward"], "")
    }

    pub(in crate::db) fn item_quests(&self, code: i64, lang: &str, roles: &[&str], tag: &str) -> Vec<Hit> {
        if code <= 0 {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT quest_id, role FROM quest_steps
             WHERE lower(kind) IN ('item', 'book', 'wearitem', 'useitem') AND code = ?1
             ORDER BY quest_id",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let wanted: Vec<String> = roles.iter().map(|role| role.to_string()).collect();
        let rows = stmt
            .query_map([code.to_string()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut seen = HashSet::new();
        let mut hits = Vec::new();
        for (id, role) in rows {
            if !wanted.iter().any(|wanted| wanted.eq_ignore_ascii_case(&role)) || !seen.insert(id) {
                continue;
            }
            let tag = if tag.is_empty() { String::new() } else { phrase(tag, lang) };
            if let Some(hit) = self.quest_link_hit(id, lang, &tag) {
                hits.push(hit);
            }
        }
        hits
    }

    pub(in crate::db) fn quest_text(&self, id: i64, lang: &str) -> String {
        let row = self.conn.query_row(
            "SELECT source, code, title, detail, prev_code FROM quests WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        );
        let Ok((source, code, title, detail, prev)) = row else {
            return phrase("missing-quest", lang);
        };
        let heading = self.quest_title(id, &title, &code, lang);
        let body = self.quest_body(id, &detail, lang);
        let mut text = format!("{heading}\n{}  {code}", label(&source, lang));
        if !body.is_empty() {
            text.push_str(&format!("\n{body}"));
        }
        if !prev.is_empty() && prev != "0" {
            text.push_str(&format!("\n{} {prev}", phrase("prev", lang)));
        }
        let mut stmt = self
            .conn
            .prepare("SELECT role, kind, code, qty, label FROM quest_steps WHERE quest_id = ?1 ORDER BY slot")
            .expect("steps");
        let steps: Vec<(String, String, String, String, String)> = stmt
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
            .expect("step rows")
            .filter_map(Result::ok)
            .collect();
        for (role, heading) in [
            ("requirement", phrase("req", lang)),
            ("objective", phrase("done", lang)),
            ("reward", phrase("reward", lang)),
        ] {
            let mine: Vec<_> = steps.iter().filter(|step| step.0 == role).collect();
            if mine.is_empty() {
                continue;
            }
            text.push_str(&format!("\n\n{heading}"));
            for (_, kind, code, qty, name) in mine {
                let shown = if code.chars().all(|ch| ch.is_ascii_digit()) && !code.is_empty() {
                    self.item_name(code.parse().unwrap_or(0), lang)
                } else if !name.is_empty() {
                    format!("{name} ({code})")
                } else {
                    code.clone()
                };
                text.push_str(&format!("\n{} {shown} × {}", kind_name(kind, lang), commas_text(qty)));
            }
        }
        text
    }

    pub(in crate::db) fn quest_title(&self, id: i64, _stored: &str, code: &str, lang: &str) -> String {
        let (en, zh, ko) = self.quest_names3(id);
        let shown = stack3(lang, &en, &zh, &ko);
        if shown.is_empty() { code.to_string() } else { shown }
    }

    pub fn quest_sheet(&self, id: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String, String)>, String)> {
        let (source, code, _title, detail, kind, repeat) = self.quest_row(id)?;
        let (en, zh, ko) = self.quest_names3(id);
        let mut name = lead3(lang, &en, &zh, &ko);
        if name.is_empty() {
            name = code.clone();
        }
        Some((
            name,
            trail3(lang, &en, &zh, &ko),
            format!("{}  {code}", label(&source, lang)),
            self.quest_facts(id, lang, &kind, &repeat),
            self.quest_body(id, &detail, lang),
        ))
    }

    pub fn quest_reward_lines(&self, id: i64, lang: &str) -> Vec<(String, String)> {
        let source = self.quest_source(id);
        let mut lines = Vec::new();
        for step in self.quest_steps(id).into_iter().filter(|step| step.role == "reward") {
            if self.step_link(&source, &step, lang, &HashSet::new()).is_some() {
                continue;
            }
            let Some(value) = reward_amount(&step) else { continue };
            let label = kind_name(&step.kind, lang);
            if label.is_empty() {
                continue;
            }
            lines.push((label, value));
        }
        lines
    }

    pub fn quest_chain(&self, id: i64, lang: &str) -> (Option<Hit>, Vec<Hit>) {
        let (parents, children) = self.quest_immediate_dependencies(id);
        // Preserve this older single-parent API for existing callers. The full
        // dependency view exposes all prerequisites and branching relationships.
        let explicit_previous = self.quest_chain_row(id)
            .and_then(|row| self.quest_id_by_code(&row.0, &row.6));
        let previous = explicit_previous.or_else(|| parents.into_iter().find(|parent| *parent != id));
        (
            previous.filter(|parent| *parent != id).and_then(|parent| self.quest_link_hit(parent, lang, "")),
            children.into_iter().filter(|child| *child != id)
                .filter_map(|child| self.quest_link_hit(child, lang, "")).collect(),
        )
    }

    pub fn quest_giver(&self, id: i64) -> String {
        self.index
            .quests
            .iter()
            .find(|quest| quest.id == id && quest.npc > 0)
            .map(|quest| format!("npc:{}", quest.npc))
            .unwrap_or_default()
    }

    pub(in crate::db) fn quest_links(&self, id: i64, lang: &str) -> Vec<Hit> {
        let source = self.quest_source(id);
        let mut hits = Vec::new();
        for step in self.quest_steps(id).into_iter().filter(|step| step.role == "reward") {
            if let Some(mut hit) = self.step_link(&source, &step, lang, &HashSet::new()) {
                hit.aside.clear();
                hits.push(hit);
            }
        }
        hits
    }

    pub(in crate::db) fn quest_npc_icon(&self, id: i64) -> Option<(PathBuf, i32)> {
        self.index
            .quests
            .iter()
            .find(|quest| quest.id == id)
            .and_then(|quest| self.index.npc_face(quest.npc))
    }

    pub(in crate::db) fn quests_for_npc(&self, code: i64, lang: &str) -> Vec<Hit> {
        self.index
            .quests
            .iter()
            .filter(|quest| quest.npc == code)
            .map(|quest| {
                Hit {
                    title: lead3(lang, &quest.title_en, &quest.title_zh, &quest.title_ko),
                    subtitle: caption(
                        &trail3(lang, &quest.title_en, &quest.title_zh, &quest.title_ko),
                        format!("{}  {}", label(&quest.source, lang), quest.code),
                    ),
                    key: format!("quest:{}", quest.id),
                    icon: None,
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn quest_facts(&self, id: i64, lang: &str, kind: &str, repeat: &str) -> Vec<(String, String, String)> {
        let mut facts = Vec::new();
        let source = self.quest_source(id);
        if !kind.is_empty() && !kind.chars().all(|ch| ch.is_ascii_digit()) {
            facts.push((phrase("kind", lang), kind.to_string(), String::new()));
        }
        if source == "main" && matches!(repeat, "1" | "2") {
            facts.push((
                chrome_text(lang, "重複週期", "Repeat cycle"),
                if repeat == "1" { chrome_text(lang, "每日", "Daily") } else { chrome_text(lang, "每週", "Weekly") },
                String::new(),
            ));
        } else if !repeat.is_empty() && repeat != "0" {
            facts.push((phrase("repeat", lang), commas_text(repeat), String::new()));
        }
        let prev = self
            .quest_chain_row(id)
            .map(|row| row.6)
            .unwrap_or_default();
        let prev_known = self.quest_id_by_code(&source, &prev).is_some();
        for step in self.quest_steps(id) {
            if step.role != "requirement" && step.role != "objective" {
                continue;
            }
            if prev_known && step.kind.eq_ignore_ascii_case("quest") && step.code == prev {
                continue;
            }
            if let Some(fact) = self.task_fact(&source, &step, lang) {
                facts.push(fact);
            }
        }
        facts
    }

    pub(in crate::db) fn task_fact(&self, source: &str, step: &QuestStep, lang: &str) -> Option<(String, String, String)> {
        let kind = step.kind.to_ascii_lowercase();
        if kind.is_empty() || kind == "0" {
            return None;
        }
        let bucket = match kind.as_str() {
            "monster" => "task-kill",
            "item" | "wearitem" | "useitem" | "book" => "task-item",
            "npc" => "task-talk",
            _ => "task-other",
        };
        let (value, key) = self.task_detail(source, step, lang, bucket)?;
        if value.is_empty() {
            return None;
        }
        Some((phrase(bucket, lang), value, key))
    }

    pub(in crate::db) fn task_detail(&self, source: &str, step: &QuestStep, lang: &str, bucket: &str) -> Option<(String, String)> {
        let kind = step.kind.to_ascii_lowercase();
        match bucket {
            "task-kill" => {
                if step.code.is_empty() || step.code == "0" {
                    return None;
                }
                let name = self.named_monster(&step.code, lang, &step.label);
                Some((with_count(name, &step.qty), format!("monster:{}", step.code)))
            }
            "task-item" => {
                let code = step.code.parse::<i64>().ok().filter(|code| *code > 0);
                let name = match code {
                    Some(code) => self.named_item(code, lang, &step.label),
                    None => step_hint(&step.label),
                };
                if name.is_empty() {
                    return None;
                }
                let key = code.map(|code| format!("item:{code}")).unwrap_or_default();
                Some((with_count(name, &step.qty), key))
            }
            "task-talk" => {
                let code = step.code.parse::<i64>().ok().filter(|code| *code > 0)?;
                Some((self.named_npc(code, lang), format!("npc:{code}")))
            }
            _ if kind.starts_with("level") => numbered(kind_name(&step.kind, lang), &step.qty),
            _ if kind.starts_with("credit") => numbered(kind_name(&step.kind, lang), &step.qty),
            _ if kind == "quest" => {
                if let Some(quest_id) = self.quest_id_by_code(source, &step.code) {
                    let hit = self.quest_link_hit(quest_id, lang, "")?;
                    Some((hit.title, format!("quest:{quest_id}")))
                } else if !step.code.is_empty() && step.code != "0" {
                    // Retain missing requirements as visible text instead of
                    // silently presenting an incomplete set of conditions.
                    Some((format!("{} {}", kind_name(&step.kind, lang), step.code), String::new()))
                } else {
                    None
                }
            }
            _ => {
                let named = kind_name(&step.kind, lang);
                let amount = step_value(&step.code, &step.qty);
                let hint = step_hint(&step.label);
                let value = match (amount.is_empty(), hint.is_empty()) {
                    (true, true) => return None,
                    (false, true) => format!("{named} {amount}"),
                    (true, false) => format!("{named} {hint}"),
                    (false, false) => format!("{named} {hint} {amount}"),
                };
                Some((value, String::new()))
            }
        }
    }

    pub(in crate::db) fn named_item(&self, code: i64, lang: &str, fallback: &str) -> String {
        let (en, zh, ko) = self.item_names3(code);
        let title = lead3(lang, &en, &zh, &ko);
        if !title.is_empty() {
            title
        } else {
            let hint = step_hint(fallback);
            if hint.is_empty() { code.to_string() } else { hint }
        }
    }

    pub(in crate::db) fn named_monster(&self, code: &str, lang: &str, fallback: &str) -> String {
        let title = self
            .monster_names3(code)
            .map(|(en, zh, ko)| lead3(lang, &en, &zh, &ko))
            .unwrap_or_default();
        if !title.is_empty() {
            title
        } else {
            let hint = step_hint(fallback);
            if hint.is_empty() { code.to_string() } else { hint }
        }
    }

    pub(in crate::db) fn named_npc(&self, code: i64, lang: &str) -> String {
        self.index
            .npc(code)
            .map(|npc| npc_names(lang, npc).0)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| code.to_string())
    }

    pub(in crate::db) fn step_link(&self, source: &str, step: &QuestStep, lang: &str, seen: &HashSet<i64>) -> Option<Hit> {
        let aside = role_phrase(&step.role, lang);
        match step.kind.to_ascii_lowercase().as_str() {
            "item" => {
                let code = step.code.parse::<i64>().ok().filter(|code| *code > 0)?;
                let (en, zh, ko) = self.item_names3(code);
                let (title, alt) = named_lead3(lang, &en, &zh, &ko, &step.label, &step.code);
                Some(self.entity_hit(title, alt, &step.qty, format!("item:{code}"), self.item_icon(code), aside))
            }
            "monster" => {
                if step.code.is_empty() || step.code == "0" {
                    return None;
                }
                let (en, zh, ko) = self.monster_names3(&step.code)?;
                let (title, alt) = named_lead3(lang, &en, &zh, &ko, &step.label, &step.code);
                Some(self.entity_hit(title, alt, &step.qty, format!("monster:{}", step.code), self.sprite(&step.code), aside))
            }
            "quest" => {
                let quest_id = self.quest_id_by_code(source, &step.code)?;
                if seen.contains(&quest_id) {
                    return None;
                }
                self.quest_link_hit(quest_id, lang, &aside)
            }
            _ => None,
        }
    }

    pub(in crate::db) fn entity_hit(
        &self,
        title: String,
        alt: String,
        qty: &str,
        key: String,
        icon: Option<(PathBuf, i32)>,
        aside: String,
    ) -> Hit {
        let amount = if qty.is_empty() || qty == "0" {
            String::new()
        } else {
            format!("× {}", commas_text(qty))
        };
        Hit {
            title,
            subtitle: caption(&alt, amount),
            key,
            icon,
            solo: false,
            aside,
            badge: None,
            chip: String::new(),
        }
    }

    pub(in crate::db) fn quest_link_hit(&self, id: i64, lang: &str, aside: &str) -> Option<Hit> {
        let (code, stored) = self.conn.query_row(
            "SELECT code, title FROM quests WHERE id = ?1",
            [id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).ok()?;
        let heading = self.quest_title(id, &stored, &code, lang);
        let mut lines = heading.lines();
        let title = lines.next().unwrap_or(&code).to_string();
        let alt = lines.next().unwrap_or("").to_string();
        Some(Hit {
            title,
            subtitle: caption(&alt, code),
            key: format!("quest:{id}"),
            icon: None,
            solo: false,
            aside: aside.to_string(),
            badge: None,
            chip: String::new(),
        })
    }

    pub(in crate::db) fn quest_names(&self, id: i64) -> (String, String) {
        (
            self.line_text(id, QUEST_TITLE_ROLES, "en").unwrap_or_default(),
            self.line_text(id, QUEST_TITLE_ROLES, "zh").unwrap_or_default(),
        )
    }

    pub(in crate::db) fn quest_names3(&self, id: i64) -> (String, String, String) {
        if let Some(quest) = self.index.quests.iter().find(|quest| quest.id == id) {
            return (quest.title_en.clone(), quest.title_zh.clone(), quest.title_ko.clone());
        }
        let (en, zh) = self.quest_names(id);
        if let Ok((source, title)) = self.conn.query_row("SELECT source, title FROM quests WHERE id = ?1", [id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))) {
            complete_quest_title(&self.conn, &source, &title, en, zh)
        } else {
            (en, zh, String::new())
        }
    }

    pub(in crate::db) fn quest_row(&self, id: i64) -> Option<(String, String, String, String, String, String)> {
        self.conn
            .query_row(
                "SELECT source, code, title, detail, kind, repeat_count FROM quests WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .ok()
    }

    pub(in crate::db) fn quest_chain_row(&self, id: i64) -> Option<(String, String, String, String, String, String, String, String)> {
        self.conn
            .query_row(
                "SELECT source, code, title, detail, kind, repeat_count, prev_code, next_code FROM quests WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .ok()
    }

    pub(in crate::db) fn quest_source(&self, id: i64) -> String {
        self.conn
            .query_row("SELECT source FROM quests WHERE id = ?1", [id], |row| row.get(0))
            .unwrap_or_default()
    }

    pub(in crate::db) fn quest_id_by_code(&self, source: &str, code: &str) -> Option<i64> {
        super::quest_graph::resolve_quest_id(&self.conn, source, code)
    }

    pub(in crate::db) fn quest_steps(&self, id: i64) -> Vec<QuestStep> {
        let mut stmt = match self
            .conn
            .prepare("SELECT role, kind, code, qty, label FROM quest_steps WHERE quest_id = ?1 ORDER BY slot")
        {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([id], |row| {
            Ok(QuestStep {
                role: row.get(0)?,
                kind: row.get(1)?,
                code: row.get(2)?,
                qty: row.get(3)?,
                label: row.get(4)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    pub(in crate::db) fn quest_body(&self, id: i64, stored: &str, lang: &str) -> String {
        let zh = self.line_text(id, &["detail", "explain", "info"], "zh").unwrap_or_default();
        let en = self.line_text(id, &["detail", "explain", "info"], "en").unwrap_or_default();
        let ko = if stored.trim() == "0" { String::new() } else { game_breaks(stored.to_string()) };
        let main = lead3(lang, &en, &zh, &ko);
        let alt = trail3(lang, &en, &zh, &ko);
        if alt.is_empty() { main } else { format!("{main}\n\n{alt}") }
    }

    pub(in crate::db) fn line_text(&self, quest_id: i64, roles: &[&str], lang: &str) -> Option<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT role, gts_id, text_object FROM quest_lines WHERE quest_id = ?1 ORDER BY slot")
            .ok()?;
        let rows: Vec<(String, i64, String)> = stmt
            .query_map([quest_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .ok()?
            .filter_map(Result::ok)
            .collect();
        for role in roles {
            let Some((_, gts, object)) = rows.iter().find(|(name, _, _)| name == role) else {
                continue;
            };
            if let Some(text) = self.gts_text(object, *gts, lang) {
                return Some(text);
            }
        }
        None
    }

    pub(in crate::db) fn gts_text(&self, object: &str, gts: i64, lang: &str) -> Option<String> {
        if gts <= 0 {
            return None;
        }
        let id = format!("{gts:08}");
        self.conn
            .query_row(
                "SELECT text FROM strings WHERE object = ?1 AND lang = ?2 AND id = ?3",
                (object, lang, id),
                |row| row.get(0),
            )
            .ok()
            .filter(|text: &String| !text.is_empty())
            .map(game_breaks)
    }
}

#[cfg(test)]
mod item_relation_tests {
    use super::*;

    #[test]
    fn worn_and_used_quest_items_have_reverse_links_without_invented_quantities() {
        let catalog = Catalog::open().expect("catalog");
        for (code, quests, reward_count) in [
            (44, [829, 843, 857, 871], 4),
            (143, [834, 848, 862, 876], 20),
        ] {
            let expected: Vec<_> = quests.iter().map(|id| format!("quest:{id}")).collect();
            let uses = catalog.item_quest_uses(code, "zh-en");
            assert_eq!(uses.iter().map(|hit| &hit.key).collect::<Vec<_>>(), expected.iter().collect::<Vec<_>>());
            let relations = catalog.item_relations(code, "zh-en");
            for key in &expected {
                let matching: Vec<_> = relations.iter().filter(|hit| &hit.key == key).collect();
                assert_eq!(matching.len(), 1, "{key}");
                assert!(!matching[0].subtitle.contains("× 0"), "{key}");
            }
            let gains = catalog.item_quest_gains(code, "zh-en");
            assert_eq!(gains.len(), reward_count);
            assert!(gains.iter().all(|hit| !expected.contains(&hit.key)));
            let previews = catalog.quest_item_previews(&quests, "zh-en");
            for id in quests {
                let required = &previews[&id].required;
                assert_eq!(required.len(), 1);
                assert_eq!(required[0].key, format!("item:{code}"));
                assert!(required[0].quantity.is_empty());
            }
        }
        // The source also contains a Book step with code 0, not a real item.
        assert!(catalog.item_quest_uses(0, "zh-en").is_empty());
    }
}

#[cfg(test)]
mod language_tests {
    use super::*;

    #[test]
    fn korean_quests_use_raw_titles_and_body_and_resolve_mercenary_character_ids() {
        let catalog = Catalog::open().expect("catalog");
        let (id, title, detail): (i64, String, String) = catalog.conn.query_row(
            "SELECT id, title, detail FROM quests WHERE source = 'main' AND title != '' AND detail != '' LIMIT 1", [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        let (name, alternate, _, _, body) = catalog.quest_sheet(id, "ko").unwrap();
        assert_eq!(name, game_breaks(title).trim());
        assert!(alternate.is_empty());
        assert_eq!(body, game_breaks(detail).trim());
        let (mercenary, expected): (i64, String) = catalog.conn.query_row(
            "SELECT q.id, c.name_ko FROM quests q JOIN characters c ON c.code = q.title WHERE q.source = 'mercenary' LIMIT 1", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(catalog.quest_sheet(mercenary, "ko").unwrap().0, expected.trim());
    }

    #[test]
    fn absent_korean_quest_translation_falls_back_without_a_duplicate_secondary() {
        let catalog = Catalog::open().expect("catalog");
        let quest = catalog.index.quests.iter().find(|quest| quest.source == "dispatch" && !quest.title_en.is_empty()).expect("dispatch quest");
        assert!(quest.title_ko.is_empty());
        let (title, alternate, _, _, _) = catalog.quest_sheet(quest.id, "ko-en").unwrap();
        assert_eq!(title, quest.title_en.trim());
        assert!(alternate.is_empty());
        assert_eq!(catalog.copyable_name(&format!("quest:{}", quest.id), "ko-en", true), None);
    }
}
