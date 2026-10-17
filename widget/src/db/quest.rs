use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rusqlite::Connection;

use super::{Catalog, Hit};
use super::names::haystack;
use super::npc::npc_names;
use super::rows::{QuestLine, QuestRow, QuestStep};
use super::store::column_exists;
use super::text::{caption, commas_text, game_breaks, kind_name, label, lead, named_lead, numbered, phrase, reward_amount, role_phrase, stack, step_hint, step_value, trail, with_count};

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
        let mut parts = vec![title.clone(), detail.clone()];
        for line in quest_lines {
            for text in line.texts.values() {
                parts.push(text.clone());
            }
        }
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        let names = haystack(&borrowed);
        quests.push(QuestRow {
            title_en: quest_heading("en", &code, quest_lines),
            title_zh: quest_heading("zh", &code, quest_lines),
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

pub(in crate::db) fn quest_heading(lang: &str, code: &str, lines: &[QuestLine]) -> String {
    if let Some(text) = quest_line(lines, QUEST_TITLE_ROLES, lang) {
        return text;
    }
    let other = if lang == "en" { "zh" } else { "en" };
    quest_line(lines, QUEST_TITLE_ROLES, other).unwrap_or_else(|| code.to_string())
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

impl Catalog {
    pub(in crate::db) fn item_quest_uses(&self, code: i64, lang: &str) -> Vec<Hit> {
        self.item_quests(code, lang, &["requirement", "objective"], "use-quest")
    }

    pub(in crate::db) fn item_quest_gains(&self, code: i64, lang: &str) -> Vec<Hit> {
        self.item_quests(code, lang, &["reward"], "")
    }

    pub(in crate::db) fn item_quests(&self, code: i64, lang: &str, roles: &[&str], tag: &str) -> Vec<Hit> {
        let mut stmt = match self.conn.prepare(
            "SELECT quest_id, role FROM quest_steps
             WHERE lower(kind) = 'item' AND code = ?1
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
        let (en, zh) = self.quest_names(id);
        let shown = stack(lang, &en, &zh);
        if shown.is_empty() { code.to_string() } else { shown }
    }

    pub fn quest_sheet(&self, id: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String, String)>, String)> {
        let (source, code, title, detail, kind, repeat) = self.quest_row(id)?;
        let (en, zh) = self.quest_names(id);
        let mut name = lead(lang, &en, &zh);
        if name.is_empty() {
            name = if title.is_empty() { code.clone() } else { title };
        }
        Some((
            name,
            trail(lang, &en, &zh),
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
        let Some((source, code, _title, _detail, _kind, _repeat, prev, next)) = self.quest_chain_row(id) else {
            return (None, Vec::new());
        };
        let prev_hit = self
            .quest_id_by_code(&source, &prev)
            .and_then(|prev_id| self.quest_link_hit(prev_id, lang, ""));
        let mut nexts = Vec::new();
        let mut seen = HashSet::new();
        if let Some(next_id) = self.quest_id_by_code(&source, &next) {
            if next_id != id {
                if let Some(hit) = self.quest_link_hit(next_id, lang, "") {
                    seen.insert(next_id);
                    nexts.push(hit);
                }
            }
        }
        for follow_id in self.follower_ids(&source, &code, id) {
            if follow_id != id && seen.insert(follow_id) {
                if let Some(hit) = self.quest_link_hit(follow_id, lang, "") {
                    nexts.push(hit);
                }
            }
        }
        (prev_hit, nexts)
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
                    title: lead(lang, &quest.title_en, &quest.title_zh),
                    subtitle: caption(
                        &trail(lang, &quest.title_en, &quest.title_zh),
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
        if !kind.is_empty() && !kind.chars().all(|ch| ch.is_ascii_digit()) {
            facts.push((phrase("kind", lang), kind.to_string(), String::new()));
        }
        if !repeat.is_empty() && repeat != "0" {
            facts.push((phrase("repeat", lang), commas_text(repeat), String::new()));
        }
        let source = self.quest_source(id);
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
                let quest_id = self.quest_id_by_code(source, &step.code)?;
                let hit = self.quest_link_hit(quest_id, lang, "")?;
                Some((hit.title, format!("quest:{quest_id}")))
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
        let (en, zh) = self.item_pair(code);
        let title = lead(lang, &en, &zh);
        if !title.is_empty() {
            title
        } else {
            let hint = step_hint(fallback);
            if hint.is_empty() { code.to_string() } else { hint }
        }
    }

    pub(in crate::db) fn named_monster(&self, code: &str, lang: &str, fallback: &str) -> String {
        let title = self
            .monster_pair(code)
            .map(|(en, zh)| lead(lang, &en, &zh))
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
                let (en, zh) = self.item_pair(code);
                let (title, alt) = named_lead(lang, &en, &zh, &step.label, &step.code);
                Some(self.entity_hit(title, alt, &step.qty, format!("item:{code}"), self.item_icon(code), aside))
            }
            "monster" => {
                if step.code.is_empty() || step.code == "0" {
                    return None;
                }
                let (en, zh) = self.monster_pair(&step.code)?;
                let (title, alt) = named_lead(lang, &en, &zh, &step.label, &step.code);
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
        if code.is_empty() || code == "0" {
            return None;
        }
        self.conn
            .query_row(
                "SELECT id FROM quests WHERE source = ?1 AND code = ?2",
                (source, code),
                |row| row.get(0),
            )
            .ok()
    }

    pub(in crate::db) fn follower_ids(&self, source: &str, code: &str, self_id: i64) -> Vec<i64> {
        let mut stmt = match self.conn.prepare(
            "SELECT id FROM quests WHERE source = ?1 AND prev_code = ?2 AND id != ?3 ORDER BY CAST(code AS INTEGER), code",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map((source, code, self_id), |row| row.get(0))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
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

    pub(in crate::db) fn quest_body(&self, id: i64, _stored: &str, lang: &str) -> String {
        let zh = self.line_text(id, &["detail", "explain", "info"], "zh").unwrap_or_default();
        let en = self.line_text(id, &["detail", "explain", "info"], "en").unwrap_or_default();
        let main = lead(lang, &en, &zh);
        let alt = trail(lang, &en, &zh);
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
