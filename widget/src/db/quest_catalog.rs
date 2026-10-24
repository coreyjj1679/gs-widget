//! Quest browse categories derived from the local data, without editorial labels.
use super::text::{lead3, trail3};
use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use super::names::text_hit;
use super::rows::{QuestRow, QuestStep};
use super::text::{
    chrome_text, game_breaks, kind_name, label, lead, reward_amount, step_hint, step_value, trail,
    with_count,
};
use super::{Catalog, Hit};

const CATEGORIES: &[&str] = &[
    "daily",
    "weekly",
    "scheduled",
    "starter",
    "standard",
    "letter",
    "intro",
    "mercenary",
    "dispatch",
    "other",
];
const STANDARD_CATEGORIES: &[&str] = &["items", "combat", "mixed", "other", "unspecified"];

/// Plain text for roadmap nodes and their lightweight preview, without opening
/// a quest sheet or changing the selected quest.
#[derive(Clone, Default)]
pub struct QuestMapPreview {
    pub summary: String,
    pub npc: String,
    pub npc_key: String,
    pub npc_location: String,
    pub objectives: String,
    pub rewards: String,
    pub reward_items: String,
    pub reward_stats: String,
    pub brief: String,
}

#[derive(Clone, Default)]
pub struct QuestListSummary {
    pub summary: String,
    pub rewards: String,
    pub reward_items: String,
    pub reward_stats: String,
}

struct PreviewTargetNames<'a> {
    items: HashMap<&'a str, PreviewEntityName>,
    monsters: HashMap<&'a str, PreviewEntityName>,
}

struct PreviewEntityName {
    primary: String,
    bilingual: String,
}

fn preview_name(lang: &str, en: &str, zh: &str, ko: &str) -> String {
    let primary = lead3(lang, en, zh, ko);
    let alternate = trail3(lang, en, zh, ko);
    if alternate.is_empty() {
        primary
    } else {
        format!("{primary} / {alternate}")
    }
}

fn preview_brief(primary: &str, alternate: &str) -> String {
    if alternate.trim().is_empty() {
        preview_excerpt(primary, 240)
    } else {
        format!(
            "{}\n\n{}",
            preview_excerpt(primary, 199),
            preview_excerpt(alternate, 199)
        )
    }
}

fn preview_reward_label(kind: &str, lang: &str) -> String {
    match kind.to_ascii_lowercase().as_str() {
        "exp" => chrome_text(lang, "經驗", "XP"),
        "expall" => chrome_text(lang, "全體經驗", "Party XP"),
        "credit" => chrome_text(lang, "信用", "Credit"),
        "money" => chrome_text(lang, "金錢", "Money"),
        _ => kind_name(kind, lang),
    }
}

fn preview_named_reward(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "item" | "book" | "wearitem" | "useitem" | "monster"
    )
}

fn preview_compact_amount(amount: &str) -> String {
    let Ok(value) = amount.replace(',', "").parse::<i128>() else {
        return amount.to_string();
    };
    let magnitude = value.unsigned_abs();
    let (unit, scale, digits) = if magnitude >= 1_000_000 {
        ("M", 1_000_000, 6)
    } else if magnitude >= 1_000 {
        ("K", 1_000, 3)
    } else {
        return amount.to_string();
    };
    let sign = if value < 0 { "-" } else { "" };
    let whole = magnitude / scale;
    let fraction = magnitude % scale;
    if fraction == 0 {
        format!("{sign}{whole}{unit}")
    } else {
        // Preserve exact quantities; the full hover also retains grouped digits.
        let fraction = format!("{fraction:0digits$}");
        format!("{sign}{whole}.{}{unit}", fraction.trim_end_matches('0'))
    }
}

fn preview_reward_stat(step: &QuestStep, lang: &str) -> Option<String> {
    let kind = step.kind.to_ascii_lowercase();
    if kind.is_empty() || kind == "0" || preview_named_reward(&kind) {
        return None;
    }
    let amount = preview_compact_amount(&reward_amount(step)?);
    let label = match kind.as_str() {
        "exp" => chrome_text(lang, "經驗", "XP"),
        "expall" => chrome_text(lang, "全體經驗", "Party XP"),
        "credit" => chrome_text(lang, "信用", "Cr"),
        "money" => chrome_text(lang, "錢", "$"),
        _ => preview_reward_label(&kind, lang),
    };
    Some(format!("{label} {amount}"))
}

fn preview_reward_items(items: &[String], lang: &str) -> String {
    if items.is_empty() {
        return String::new();
    }
    let joined = items.join(" · ");
    if joined.chars().count() <= 36 {
        return joined;
    }
    let more = if items.len() > 1 {
        format!(" +{}{}", items.len() - 1, chrome_text(lang, "項", ""))
    } else {
        String::new()
    };
    format!(
        "{}{}",
        preview_excerpt(&items[0], 36 - more.chars().count()),
        more
    )
}

fn preview_excerpt(text: &str, limit: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= limit {
        return text;
    }
    let mut excerpt: String = text.chars().take(limit.saturating_sub(1)).collect();
    // Avoid chopping an English word when a useful word boundary is available.
    if let Some((prefix, _)) = excerpt.rsplit_once(' ') {
        if prefix.chars().count() >= limit / 2 {
            excerpt = prefix.to_string();
        }
    }
    format!("{}…", excerpt.trim_end())
}

fn preview_target_excerpt(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    // Quantities are always kept even when a long item/monster name is shortened.
    if let Some((name, quantity)) = text.rsplit_once(" × ") {
        let suffix = format!(" × {quantity}");
        if suffix.chars().count() + 2 < limit {
            return format!(
                "{}{}",
                preview_excerpt(name, limit - suffix.chars().count()),
                suffix
            );
        }
    }
    preview_excerpt(text, limit)
}

fn preview_summary(targets: &[String], fallback: &str) -> String {
    if targets.is_empty() {
        return preview_excerpt(fallback, 60);
    }
    let joined = targets.join(" · ");
    if joined.chars().count() <= 60 {
        joined
    } else if targets.len() > 1 {
        format!("{} …", preview_target_excerpt(&targets[0], 58))
    } else {
        preview_target_excerpt(&targets[0], 60)
    }
}

fn preview_objective_summary(targets: &[String], fallback: &str) -> String {
    let compact_targets: Vec<_> = targets
        .iter()
        .map(|target| {
            let Some((name, quantity)) = target.rsplit_once(" × ") else {
                return target.clone();
            };
            if let Some((heading, name)) = name.split_once(": ") {
                format!("{heading}: × {quantity} {name}")
            } else {
                format!("× {quantity} {name}")
            }
        })
        .collect();
    preview_summary(&compact_targets, fallback)
}

fn preview_reward_summary(rewards: &[String], lang: &str) -> String {
    if rewards.is_empty() {
        return chrome_text(lang, "未提供獎勵資料", "No rewards recorded");
    }
    let scalar_labels: HashSet<_> = ["exp", "expall", "credit", "money"]
        .iter()
        .map(|kind| preview_reward_label(kind, lang))
        .collect();
    let mut ordered: Vec<_> = rewards.iter().collect();
    ordered.sort_by_key(|line| {
        !line
            .split_once(": ")
            .is_some_and(|(label, _)| scalar_labels.contains(label))
    });
    let more = |count: usize| {
        if count == 0 {
            String::new()
        } else {
            format!(" · +{count} {}", chrome_text(lang, "項", "more"))
        }
    };
    let mut shown = String::new();
    let mut consumed = 0;
    for (index, line) in ordered.iter().enumerate() {
        let separator = if shown.is_empty() { "" } else { " · " };
        let suffix = more(ordered.len() - index - 1);
        let available = 100usize.saturating_sub(
            shown.chars().count() + separator.chars().count() + suffix.chars().count(),
        );
        if line.chars().count() <= available {
            shown.push_str(separator);
            shown.push_str(line);
            consumed += 1;
        } else {
            if available >= 18 {
                shown.push_str(separator);
                shown.push_str(&preview_target_excerpt(line, available));
                consumed += 1;
            }
            break;
        }
    }
    shown.push_str(&more(ordered.len() - consumed));
    shown
}

fn list_preview_steps(conn: &Connection, wanted: &HashSet<i64>) -> HashMap<i64, Vec<QuestStep>> {
    let mut found: HashMap<i64, Vec<QuestStep>> = HashMap::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT quest_id, role, kind, code, qty, label FROM quest_steps
         WHERE lower(role) IN ('objective', 'requirement', 'reward') ORDER BY quest_id, slot",
    ) else {
        return found;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            QuestStep {
                role: row.get(1)?,
                kind: row.get(2)?,
                code: row.get(3)?,
                qty: row.get(4)?,
                label: row.get(5)?,
            },
        ))
    }) else {
        return found;
    };
    for (id, step) in rows.filter_map(Result::ok) {
        if wanted.contains(&id) {
            found.entry(id).or_default().push(step);
        }
    }
    found
}

fn list_preview_missions(
    conn: &Connection,
    wanted: &HashSet<i64>,
    lang: &str,
) -> HashMap<i64, String> {
    if wanted.is_empty() {
        return HashMap::new();
    }
    let Ok(mut stmt) = conn.prepare(
        "SELECT ql.quest_id, s.lang, s.text FROM quest_lines ql
         JOIN strings s ON s.object = ql.text_object AND s.id = printf('%08d', ql.gts_id)
         WHERE ql.role = 'mission' AND s.lang IN ('en', 'zh') ORDER BY ql.quest_id, ql.slot",
    ) else {
        return HashMap::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) else {
        return HashMap::new();
    };
    let mut texts: HashMap<i64, (String, String)> = HashMap::new();
    for (id, language, text) in rows.filter_map(Result::ok) {
        if !wanted.contains(&id) || text.trim().is_empty() {
            continue;
        }
        let pair = texts.entry(id).or_default();
        let target = if language == "en" {
            &mut pair.0
        } else {
            &mut pair.1
        };
        if target.is_empty() {
            *target = game_breaks(text);
        }
    }
    texts
        .into_iter()
        .map(|(id, (en, zh))| (id, lead(lang, &en, &zh)))
        .collect()
}

fn preview_texts(conn: &Connection, id: i64, lang: &str) -> (String, String, String, String) {
    let mut stmt = match conn.prepare(
        "SELECT ql.role, s.lang, s.text FROM quest_lines ql
         JOIN strings s ON s.object = ql.text_object AND s.id = printf('%08d', ql.gts_id)
         WHERE ql.quest_id = ?1 AND ql.role IN ('detail', 'explain', 'info', 'mission')
            AND s.lang IN ('en', 'zh') ORDER BY ql.slot",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Default::default(),
    };
    let lines = stmt
        .query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
        .unwrap_or_default();
    let pick = |roles: &[&str], language: &str| {
        for role in roles {
            if let Some((_, _, text)) = lines.iter().find(|(have_role, have_lang, text)| {
                have_role == role && have_lang == language && !text.trim().is_empty()
            }) {
                return game_breaks(text.clone());
            }
        }
        String::new()
    };
    let mission_en = pick(&["mission"], "en");
    let mission_zh = pick(&["mission"], "zh");
    let note_en = pick(&["detail", "explain", "info"], "en");
    let note_zh = pick(&["detail", "explain", "info"], "zh");
    let note_ko: String = conn.query_row("SELECT IFNULL(detail, '') FROM quests WHERE id = ?1", [id], |row| row.get(0)).unwrap_or_default();
    let note_ko = game_breaks(note_ko);
    (
        lead(lang, &mission_en, &mission_zh),
        trail(lang, &mission_en, &mission_zh),
        lead3(lang, &note_en, &note_zh, &note_ko),
        trail3(lang, &note_en, &note_zh, &note_ko),
    )
}

#[derive(Default)]
struct Metadata {
    repeat: String,
    level: Option<i64>,
    objectives: HashSet<&'static str>,
}

fn primary_category<'a>(source: &'a str, kind: &str, repeat: &str) -> &'a str {
    // The same repeat_count column has unrelated meanings in the other sources.
    // Test cadence before kind: two starter quests are daily quests.
    match (source, repeat, kind) {
        ("main", "1", _) => "daily",
        ("main", "2", _) => "weekly",
        ("main", _, "7") => "scheduled",
        ("main", _, "0") => "starter",
        ("main", _, _) => "standard",
        ("letter" | "intro" | "mercenary" | "dispatch", _, _) => source,
        _ => "other",
    }
}

fn standard_category(meta: &Metadata) -> &'static str {
    if meta.objectives.is_empty() {
        "unspecified"
    } else if meta.objectives.contains("item") && meta.objectives.contains("combat") {
        "mixed"
    } else if meta.objectives.len() == 1 && meta.objectives.contains("item") {
        "items"
    } else if meta.objectives.len() == 1 && meta.objectives.contains("combat") {
        "combat"
    } else {
        "other"
    }
}

fn category_matches(selected: &str, primary: &str, meta: &Metadata) -> bool {
    if selected.is_empty() || selected == "all" {
        true
    } else if let Some(secondary) = selected.strip_prefix("standard:") {
        primary == "standard" && secondary == standard_category(meta)
    } else {
        selected == primary
    }
}

fn category_name(category: &str, lang: &str) -> String {
    let (zh, en) = match category {
        "all" | "" => ("全部任務", "All quests"),
        "daily" => ("每日任務", "Daily quests"),
        "weekly" => ("每週任務", "Weekly quests"),
        "scheduled" => ("定期任務", "Timed quests"),
        "starter" => ("新手任務", "Starter quests"),
        "standard" => ("一般任務", "Standard quests"),
        "standard:items" => ("物品目標", "Item objectives"),
        "standard:combat" => ("怪物討伐", "Monster combat"),
        "standard:mixed" => ("混合目標", "Mixed objectives"),
        "standard:other" => ("其他條件", "Other conditions"),
        "standard:unspecified" => ("目標未註明", "Unspecified objectives"),
        "intro" => ("引導任務", "Guides"),
        "other" => ("其他來源", "Other sources"),
        source => return label(source, lang),
    };
    chrome_text(lang, zh, en)
}

fn category_description(category: &str, lang: &str) -> String {
    let (zh, en) = match category {
        "all" => ("按任務週期與來源分類", "Browse by cadence and source"),
        "daily" => ("每日可重複的任務", "Quests with a daily repeat cycle"),
        "weekly" => ("每週可重複的任務", "Quests with a weekly repeat cycle"),
        "scheduled" => (
            "有時間限制；未提供完整重置規則",
            "Time restricted; full reset rules unavailable",
        ),
        "starter" => ("新手入門任務", "Getting started quests"),
        "standard" => ("一般任務與委託", "General quests and commissions"),
        "standard:items" => ("以物品為目標的任務", "Quests with item objectives"),
        "standard:combat" => ("以討伐怪物為目標的任務", "Quests with monster objectives"),
        "standard:mixed" => (
            "同時包含討伐與物品目標",
            "Quests with both combat and item objectives",
        ),
        "standard:other" => (
            "挑戰、戰鬥階段及狀態等目標",
            "Challenges, battle ranks and other objectives",
        ),
        "standard:unspecified" => (
            "未提供目標類型，請查看任務內容",
            "Objective type unavailable; see quest details",
        ),
        "letter" => ("書信任務系列", "Letter quest series"),
        "intro" => ("成長引導與系統教學", "Progression and game-system guides"),
        "mercenary" => ("傭兵劇情任務系列", "Mercenary quest series"),
        "dispatch" => ("客棧派遣任務", "Inn dispatch assignments"),
        _ => ("其他資料來源的任務", "Quests from other sources"),
    };
    chrome_text(lang, zh, en)
}

fn objective_category(kind: &str) -> &'static str {
    match kind.to_ascii_lowercase().as_str() {
        "monster" => "combat",
        "item" | "wearitem" | "useitem" | "book" => "item",
        "npc" => "talk",
        "work" => "work",
        _ => "other",
    }
}

fn objective_name(category: &str, lang: &str) -> String {
    let (zh, en) = match category {
        "combat" => ("討伐", "Combat"),
        "item" => ("物品", "Items"),
        "talk" => ("對話", "Talk"),
        "work" => ("工作", "Work"),
        _ => ("其他目標", "Other objectives"),
    };
    chrome_text(lang, zh, en)
}

fn objective_summary(metadata: &Metadata, lang: &str) -> String {
    ["combat", "item", "talk", "work", "other"]
        .iter()
        .filter(|kind| metadata.objectives.contains(**kind))
        .map(|kind| objective_name(kind, lang))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn load_metadata(conn: &Connection) -> HashMap<i64, Metadata> {
    let mut metadata: HashMap<i64, Metadata> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT id, repeat_count FROM quests") {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        }) {
            for (id, repeat) in rows.filter_map(Result::ok) {
                metadata.entry(id).or_default().repeat = repeat;
            }
        }
    }
    if let Ok(mut stmt) = conn.prepare(
        "SELECT quest_id, role, kind, qty FROM quest_steps
         WHERE role = 'objective' OR (role = 'requirement' AND lower(kind) = 'level')",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        }) {
            for (id, role, kind, qty) in rows.filter_map(Result::ok) {
                let meta = metadata.entry(id).or_default();
                if role == "objective" {
                    if !kind.is_empty() && kind != "0" {
                        meta.objectives.insert(objective_category(&kind));
                    }
                } else if let Some(level) = qty.parse::<i64>().ok().filter(|level| *level > 0) {
                    // Level1..Level5 concern specific mercenaries, not character level.
                    meta.level = Some(meta.level.unwrap_or(0).max(level));
                }
            }
        }
    }
    metadata
}

impl Catalog {
    /// Builds every visible list row with one steps read and, when needed, one
    /// mission-text read. No per-row SQL or repeated linear entity-name lookup.
    pub fn quest_list_summaries(&self, ids: &[i64], lang: &str) -> HashMap<i64, QuestListSummary> {
        let wanted: HashSet<_> = ids.iter().copied().collect();
        if wanted.is_empty() {
            return HashMap::new();
        }
        let steps = list_preview_steps(&self.conn, &wanted);
        let codes: HashSet<_> = steps
            .values()
            .flatten()
            .map(|step| step.code.as_str())
            .collect();
        let names = PreviewTargetNames {
            items: self
                .index
                .items
                .iter()
                .filter(|item| codes.contains(item.code_str.as_str()))
                .map(|item| {
                    (
                        item.code_str.as_str(),
                        PreviewEntityName {
                            primary: lead3(lang, &item.en, &item.zh, &item.ko),
                            bilingual: preview_name(lang, &item.en, &item.zh, &item.ko),
                        },
                    )
                })
                .collect(),
            monsters: self
                .index
                .monsters
                .iter()
                .filter(|monster| codes.contains(monster.code.as_str()))
                .map(|monster| {
                    (
                        monster.code.as_str(),
                        PreviewEntityName {
                            primary: lead3(lang, &monster.en, &monster.zh, &monster.ko),
                            bilingual: preview_name(lang, &monster.en, &monster.zh, &monster.ko),
                        },
                    )
                })
                .collect(),
        };
        let mut found = HashMap::new();
        let mut needs_mission = HashSet::new();
        for quest in self
            .index
            .quests
            .iter()
            .filter(|quest| wanted.contains(&quest.id))
        {
            let mut targets = Vec::new();
            let mut required_targets = Vec::new();
            let mut rewards = Vec::new();
            let mut reward_items = Vec::new();
            let mut reward_stats = Vec::new();
            for step in steps.get(&quest.id).into_iter().flatten() {
                if step.role.eq_ignore_ascii_case("reward") {
                    if let Some(line) = self.quest_preview_reward(step, lang, Some(&names)) {
                        if preview_named_reward(&step.kind) {
                            reward_items.push(self.quest_preview_reward_item(
                                step,
                                lang,
                                Some(&names),
                            ));
                        } else if let Some(stat) = preview_reward_stat(step, lang) {
                            reward_stats.push(stat);
                        }
                        rewards.push(line);
                    }
                } else if step.role.eq_ignore_ascii_case("objective") {
                    if let Some(line) =
                        self.quest_preview_step(&quest.source, step, lang, Some(&names))
                    {
                        targets.push(line);
                    }
                } else if step.role.eq_ignore_ascii_case("requirement")
                    && matches!(
                        step.kind.to_ascii_lowercase().as_str(),
                        "item" | "wearitem" | "useitem" | "book" | "monster"
                    )
                {
                    if let Some(line) =
                        self.quest_preview_step(&quest.source, step, lang, Some(&names))
                    {
                        required_targets
                            .push(format!("{} {line}", chrome_text(lang, "需要", "Requires")));
                    }
                }
            }
            let targets = if targets.is_empty() {
                &required_targets
            } else {
                &targets
            };
            if targets.is_empty() {
                needs_mission.insert(quest.id);
            }
            found.insert(
                quest.id,
                QuestListSummary {
                    summary: preview_objective_summary(targets, ""),
                    rewards: preview_reward_summary(&rewards, lang),
                    reward_items: preview_reward_items(&reward_items, lang),
                    reward_stats: reward_stats.join(" · "),
                },
            );
        }
        let missions = list_preview_missions(&self.conn, &needs_mission, lang);
        let unknown = chrome_text(lang, "目標未註明", "Objective not specified");
        for id in needs_mission {
            if let Some(row) = found.get_mut(&id) {
                row.summary = preview_summary(
                    &[],
                    missions
                        .get(&id)
                        .filter(|text| !text.trim().is_empty())
                        .unwrap_or(&unknown),
                );
            }
        }
        found
    }

    pub fn quest_alternate_title(&self, id: i64, lang: &str) -> String {
        self.index
            .quests
            .iter()
            .find(|quest| quest.id == id)
            .map(|quest| trail3(lang, &quest.title_en, &quest.title_zh, &quest.title_ko))
            .unwrap_or_default()
    }

    /// Two indexed reads per node; entity and prerequisite names use the loaded
    /// catalog so precomputing previews for a whole component stays inexpensive.
    pub fn quest_map_preview(&self, id: i64, lang: &str) -> QuestMapPreview {
        let Some(quest) = self.index.quests.iter().find(|quest| quest.id == id) else {
            return QuestMapPreview {
                summary: chrome_text(lang, "找不到這個任務", "Quest not found"),
                ..Default::default()
            };
        };
        let steps = self.quest_steps(id);
        let (mission, mission_alt, note, note_alt) = preview_texts(&self.conn, id, lang);
        let mut targets = Vec::new();
        let mut requirements = Vec::new();
        let mut required_targets = Vec::new();
        let mut rewards = Vec::new();
        let mut reward_items = Vec::new();
        let mut reward_stats = Vec::new();
        for step in &steps {
            if step.role.eq_ignore_ascii_case("reward") {
                if let Some(reward) = self.quest_preview_reward(step, lang, None) {
                    if preview_named_reward(&step.kind) {
                        reward_items.push(self.quest_preview_reward_item(step, lang, None));
                    } else if let Some(stat) = preview_reward_stat(step, lang) {
                        reward_stats.push(stat);
                    }
                    rewards.push(reward);
                }
                continue;
            }
            if !step.role.eq_ignore_ascii_case("objective")
                && !step.role.eq_ignore_ascii_case("requirement")
            {
                continue;
            }
            let Some(line) = self.quest_preview_step(&quest.source, step, lang, None) else {
                continue;
            };
            if step.role.eq_ignore_ascii_case("objective") {
                targets.push(line);
            } else {
                if matches!(
                    step.kind.to_ascii_lowercase().as_str(),
                    "item" | "wearitem" | "useitem" | "book" | "monster"
                ) {
                    required_targets
                        .push(format!("{} {line}", chrome_text(lang, "需要", "Requires")));
                }
                requirements.push(line);
            }
        }
        let unknown = chrome_text(lang, "目標未註明", "Objective not specified");
        let summary = preview_objective_summary(
            if targets.is_empty() {
                &required_targets
            } else {
                &targets
            },
            if mission.trim().is_empty() {
                &unknown
            } else {
                &mission
            },
        );
        let mut objectives = Vec::new();
        if !targets.is_empty() {
            objectives.push(format!(
                "{}\n{}",
                chrome_text(lang, "任務目標", "Objectives"),
                targets.join("\n")
            ));
        } else if !mission.trim().is_empty() {
            objectives.push(format!(
                "{}\n{}",
                chrome_text(lang, "任務目標", "Objectives"),
                if mission_alt.is_empty() {
                    mission.trim().to_string()
                } else {
                    format!("{}\n{}", mission.trim(), mission_alt.trim())
                }
            ));
        } else {
            objectives.push(unknown);
        }
        if !requirements.is_empty() {
            objectives.push(format!(
                "{}\n{}",
                chrome_text(lang, "接取條件", "Requirements"),
                requirements.join("\n")
            ));
        }
        let (npc, npc_key, npc_location) = if quest.npc > 0 {
            let known = self.index.npc(quest.npc);
            let key = known.map(|npc| format!("npc:{}", npc.code)).unwrap_or_default();
            let name = known
                .map(|npc| preview_name(lang, &npc.en, &npc.zh, &npc.ko))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| self.named_npc(quest.npc, lang));
            let location = known
                .map(|npc| {
                    let place = preview_name(lang, &npc.place_en, &npc.place_zh, "");
                    if place.is_empty() {
                        preview_name(lang, &npc.map_en, &npc.map_zh, &npc.map_ko)
                    } else {
                        place
                    }
                })
                .unwrap_or_default();
            (name, key, location)
        } else {
            (String::new(), String::new(), String::new())
        };
        QuestMapPreview {
            summary,
            npc,
            npc_key,
            npc_location,
            objectives: objectives.join("\n\n"),
            rewards: if rewards.is_empty() {
                chrome_text(lang, "未提供獎勵資料", "No rewards recorded")
            } else {
                rewards.join("\n")
            },
            reward_items: preview_reward_items(&reward_items, lang),
            reward_stats: reward_stats.join(" · "),
            brief: preview_brief(&note, &note_alt),
        }
    }

    fn quest_preview_target(
        &self,
        step: &QuestStep,
        lang: &str,
        names: Option<&PreviewTargetNames<'_>>,
    ) -> String {
        with_count(
            self.quest_preview_target_name(step, lang, names, true),
            &step.qty,
        )
    }

    fn quest_preview_reward_item(
        &self,
        step: &QuestStep,
        lang: &str,
        names: Option<&PreviewTargetNames<'_>>,
    ) -> String {
        let name = self.quest_preview_target_name(step, lang, names, false);
        if step.qty.is_empty() || step.qty == "0" {
            name
        } else {
            // Keep the quantity visible even when a narrow list/node elides the name.
            let quantity = with_count(String::new(), &step.qty);
            format!("{} {name}", quantity.trim())
        }
    }

    fn quest_preview_target_name(
        &self,
        step: &QuestStep,
        lang: &str,
        names: Option<&PreviewTargetNames<'_>>,
        bilingual: bool,
    ) -> String {
        let kind = step.kind.to_ascii_lowercase();
        if let Some(names) = names {
            let name = if kind == "monster" {
                names.monsters.get(step.code.as_str())
            } else {
                names.items.get(step.code.as_str())
            };
            name.map(|name| {
                if bilingual {
                    name.bilingual.clone()
                } else {
                    name.primary.clone()
                }
            })
        } else if kind == "monster" {
            self.index
                .monsters
                .iter()
                .find(|monster| monster.code == step.code)
                .map(|monster| {
                    if bilingual {
                        preview_name(lang, &monster.en, &monster.zh, &monster.ko)
                    } else {
                        lead3(lang, &monster.en, &monster.zh, &monster.ko)
                    }
                })
        } else {
            self.index
                .items
                .iter()
                .find(|item| item.code_str == step.code)
                .map(|item| {
                    if bilingual {
                        preview_name(lang, &item.en, &item.zh, &item.ko)
                    } else {
                        lead3(lang, &item.en, &item.zh, &item.ko)
                    }
                })
        }
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            let hint = step_hint(&step.label);
            if hint.is_empty() {
                step.code.clone()
            } else {
                hint
            }
        })
    }

    fn quest_preview_step(
        &self,
        source: &str,
        step: &QuestStep,
        lang: &str,
        names: Option<&PreviewTargetNames<'_>>,
    ) -> Option<String> {
        let kind = step.kind.to_ascii_lowercase();
        if kind.is_empty() || kind == "0" {
            return None;
        }
        let (heading, value) = match kind.as_str() {
            "item" | "book" | "wearitem" | "useitem" | "monster" => {
                let heading = if kind == "monster" {
                    chrome_text(lang, "討伐", "Defeat")
                } else {
                    kind_name(&step.kind, lang)
                };
                (heading, self.quest_preview_target(step, lang, names))
            }
            "npc" => {
                let code = step.code.parse::<i64>().ok().filter(|code| *code > 0)?;
                (
                    chrome_text(lang, "對話", "Talk to"),
                    self.index
                        .npc(code)
                        .map(|npc| preview_name(lang, &npc.en, &npc.zh, &npc.ko))
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| self.named_npc(code, lang)),
                )
            }
            "quest" => {
                if step.code.is_empty() || step.code == "0" {
                    return None;
                }
                let candidates: Vec<_> = self
                    .index
                    .quests
                    .iter()
                    .filter(|quest| quest.source == source && quest.code == step.code)
                    .collect();
                let target = if candidates.len() == 1 {
                    candidates.first().copied()
                } else if candidates.is_empty() && source == "intro" {
                    let mut main = self
                        .index
                        .quests
                        .iter()
                        .filter(|quest| quest.source == "main" && quest.code == step.code);
                    let first = main.next();
                    if main.next().is_none() {
                        first
                    } else {
                        None
                    }
                } else {
                    None
                };
                let name = target
                    .map(|quest| preview_name(lang, &quest.title_en, &quest.title_zh, &quest.title_ko))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("#{}", step.code));
                (chrome_text(lang, "前置任務", "Prerequisite"), name)
            }
            _ => {
                let heading = if (kind.starts_with("level") || kind.starts_with("credit"))
                    && kind.chars().last().is_some_and(|ch| ch.is_ascii_digit())
                {
                    // Keep numbered conditions distinct instead of mislabelling them as character level.
                    format!(
                        "{} ({})",
                        kind_name(&step.kind, lang),
                        kind.chars().last().unwrap()
                    )
                } else {
                    kind_name(&step.kind, lang)
                };
                let amount = step_value(&step.code, &step.qty);
                let hint = step_hint(&step.label);
                let value = [hint, amount]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                (heading, value)
            }
        };
        (!value.is_empty()).then(|| format!("{heading}: {value}"))
    }

    fn quest_preview_reward(
        &self,
        step: &QuestStep,
        lang: &str,
        names: Option<&PreviewTargetNames<'_>>,
    ) -> Option<String> {
        let kind = step.kind.to_ascii_lowercase();
        if kind.is_empty() || kind == "0" {
            return None;
        }
        if matches!(
            kind.as_str(),
            "item" | "book" | "wearitem" | "useitem" | "monster"
        ) {
            return Some(self.quest_preview_target(step, lang, names));
        }
        let amount = reward_amount(step)?;
        let heading = preview_reward_label(&step.kind, lang);
        Some(format!("{heading}: {amount}"))
    }

    pub fn quest_categories(&self, lang: &str) -> Vec<Hit> {
        let metadata = load_metadata(&self.conn);
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for quest in &self.index.quests {
            let repeat = metadata
                .get(&quest.id)
                .map(|meta| meta.repeat.as_str())
                .unwrap_or("");
            *counts
                .entry(primary_category(&quest.source, &quest.kind, repeat))
                .or_default() += 1;
        }
        std::iter::once(("all", self.index.quests.len()))
            .chain(
                CATEGORIES
                    .iter()
                    .filter_map(|category| counts.get(category).map(|count| (*category, *count))),
            )
            .map(|(category, count)| Hit {
                title: category_name(category, lang),
                subtitle: category_description(category, lang),
                key: format!("qcat:{category}"),
                icon: None,
                solo: false,
                badge: None,
                aside: count.to_string(),
                chip: String::new(),
            })
            .collect()
    }

    /// Standard quests have one secondary category each. The aggregate remains
    /// first so the same model can drive a "view all" action and the selector.
    pub fn quest_standard_categories(&self, lang: &str) -> Vec<Hit> {
        let metadata = load_metadata(&self.conn);
        let empty = Metadata::default();
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for quest in &self.index.quests {
            let meta = metadata.get(&quest.id).unwrap_or(&empty);
            if primary_category(&quest.source, &quest.kind, &meta.repeat) == "standard" {
                *counts.entry(standard_category(meta)).or_default() += 1;
            }
        }
        std::iter::once(("standard".to_string(), counts.values().sum::<usize>()))
            .chain(STANDARD_CATEGORIES.iter().map(|secondary| {
                (
                    format!("standard:{secondary}"),
                    counts.get(secondary).copied().unwrap_or(0),
                )
            }))
            .map(|(category, count)| Hit {
                title: category_name(&category, lang),
                subtitle: category_description(&category, lang),
                key: format!("qcat:{category}"),
                icon: None,
                solo: false,
                badge: None,
                aside: count.to_string(),
                chip: String::new(),
            })
            .collect()
    }

    /// Returns the entire matching category, including more than 300 quests.
    pub fn quest_category_records(&self, category: &str, query: &str, lang: &str) -> Vec<Hit> {
        let category = category.strip_prefix("qcat:").unwrap_or(category);
        let query = query.trim().trim_start_matches('#');
        let folded = query.to_lowercase();
        self.index.prepare_aliases(&folded);
        let metadata = load_metadata(&self.conn);
        let step_matches = self.quest_step_search_matches(query, &folded);
        let empty = Metadata::default();
        let mut picked = Vec::new();
        for quest in &self.index.quests {
            let meta = metadata.get(&quest.id).unwrap_or(&empty);
            let group = primary_category(&quest.source, &quest.kind, &meta.repeat);
            if !category_matches(category, group, meta) {
                continue;
            }
            let exact = self
                .index
                .name_hit(query, &folded, &quest.code, &quest.names)
                || self.quest_metadata_matches(quest, meta, group, &folded)
                || step_matches.contains(&quest.id);
            if exact || text_hit(&folded, &quest.names) {
                picked.push((!exact, quest, meta, group));
            }
        }
        // Preserve index order, with exact matches ahead of fuzzy ones.
        picked.sort_by_key(|(fuzzy, _, _, _)| *fuzzy);
        picked
            .into_iter()
            .map(|(_, quest, meta, group)| {
                // The detail pane carries bilingual titles; list metadata stays compact.
                let mut parts = Vec::new();
                if quest.npc > 0 {
                    parts.push(self.named_npc(quest.npc, lang));
                }
                parts.push(objective_summary(meta, lang));
                parts.push(if quest.source == "main" {
                    format!("#{}", quest.code)
                } else {
                    format!(
                        "{} · #{}",
                        if quest.source == "intro" {
                            category_name("intro", lang)
                        } else {
                            label(&quest.source, lang)
                        },
                        quest.code
                    )
                });
                Hit {
                    title: lead3(lang, &quest.title_en, &quest.title_zh, &quest.title_ko),
                    subtitle: parts
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · "),
                    key: format!("quest:{}", quest.id),
                    icon: self.index.npc_face(quest.npc),
                    solo: false,
                    badge: None,
                    aside: meta
                        .level
                        .map(|level| format!("Lv. {level}+"))
                        .unwrap_or_default(),
                    chip: if group == "standard" {
                        category_name(&format!("standard:{}", standard_category(meta)), lang)
                    } else {
                        category_name(group, lang)
                    },
                }
            })
            .collect()
    }

    fn quest_step_search_matches(&self, query: &str, folded: &str) -> HashSet<i64> {
        if folded.is_empty() {
            return HashSet::new();
        }
        // Resolve translated item names once, then join those codes to every
        // matching quest in one read. Never query item names per quest/step.
        let item_codes: HashSet<&str> = self
            .index
            .items
            .iter()
            .filter(|item| {
                self.index
                    .name_hit(query, folded, &item.code_str, &item.names)
            })
            .map(|item| item.code_str.as_str())
            .collect();
        let mut stmt = match self.conn.prepare(
            "SELECT quest_id, role, kind, code, qty, label FROM quest_steps
             WHERE lower(role) IN ('requirement', 'objective', 'reward')
               AND (lower(kind) IN ('item', 'wearitem', 'useitem', 'book') OR lower(role) = 'reward')",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return HashSet::new(),
        };
        let rows = match stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                QuestStep {
                    role: row.get(1)?,
                    kind: row.get(2)?,
                    code: row.get(3)?,
                    qty: row.get(4)?,
                    label: row.get(5)?,
                },
            ))
        }) {
            Ok(rows) => rows,
            Err(_) => return HashSet::new(),
        };
        let mut found = HashSet::new();
        let mut reward_labels: HashMap<String, Vec<String>> = HashMap::new();
        for (id, step) in rows.filter_map(Result::ok) {
            let kind = step.kind.to_ascii_lowercase();
            if matches!(kind.as_str(), "item" | "wearitem" | "useitem" | "book") {
                if item_codes.contains(step.code.as_str())
                    || step.code.eq_ignore_ascii_case(query)
                    || (!step.label.is_empty() && step.label.to_lowercase().contains(folded))
                {
                    found.insert(id);
                }
            } else if step.role.eq_ignore_ascii_case("reward") && !kind.is_empty() && kind != "0" {
                let Some(amount) = reward_amount(&step) else {
                    continue;
                };
                let labels = reward_labels.entry(kind.clone()).or_insert_with(|| {
                    let mut labels = vec![
                        kind.clone(),
                        preview_reward_label(&kind, "en").to_lowercase(),
                        preview_reward_label(&kind, "zh"),
                        kind_name(&kind, "en").to_lowercase(),
                        kind_name(&kind, "zh"),
                    ];
                    if matches!(kind.as_str(), "exp" | "expall") {
                        labels.extend(["experience".to_string(), "經驗值".to_string()]);
                    }
                    labels.sort();
                    labels.dedup();
                    labels
                });
                if amount.contains(folded)
                    || step.qty.to_lowercase().contains(folded)
                    || labels.iter().any(|label| {
                        label.contains(folded)
                            || format!("{label}: {amount}").contains(folded)
                            || format!("{label} {amount}").contains(folded)
                            || format!("{label} {}", step.qty).contains(folded)
                    })
                {
                    found.insert(id);
                }
            }
        }
        found
    }

    fn quest_metadata_matches(
        &self,
        quest: &QuestRow,
        meta: &Metadata,
        category: &str,
        folded: &str,
    ) -> bool {
        if folded.is_empty() {
            return true;
        }
        ["zh", "en"].iter().any(|lang| {
            category_name(category, lang)
                .to_lowercase()
                .contains(folded)
                || (category == "standard"
                    && category_name(&format!("standard:{}", standard_category(meta)), lang)
                        .to_lowercase()
                        .contains(folded))
                || label(&quest.source, lang).to_lowercase().contains(folded)
                || objective_summary(meta, lang)
                    .to_lowercase()
                    .contains(folded)
                || (quest.npc > 0
                    && self
                        .named_npc(quest.npc, lang)
                        .to_lowercase()
                        .contains(folded))
        })
    }

    pub fn quest_classification_facts(&self, id: i64, lang: &str) -> Vec<(String, String, String)> {
        let Some(quest) = self.index.quests.iter().find(|quest| quest.id == id) else {
            return Vec::new();
        };
        let metadata = load_metadata(&self.conn);
        let empty = Metadata::default();
        let meta = metadata.get(&id).unwrap_or(&empty);
        let category = primary_category(&quest.source, &quest.kind, &meta.repeat);
        let mut facts = vec![(
            chrome_text(lang, "分類", "Category"),
            category_name(category, lang),
            format!("qcat:{category}"),
        )];
        if category == "standard" {
            let secondary = format!("standard:{}", standard_category(meta));
            facts.push((
                chrome_text(lang, "目標分類", "Objective"),
                category_name(&secondary, lang),
                String::new(),
            ));
        }
        if let Some(npc) = self.index.npcs.iter().find(|npc| npc.code == quest.npc) {
            let name = preview_name(lang, &npc.en, &npc.zh, &npc.ko);
            facts.push((
                "NPC".into(),
                if name.is_empty() {
                    self.named_npc(quest.npc, lang)
                } else {
                    name
                },
                format!("npc:{}", quest.npc),
            ));
            let place = lead(lang, &npc.place_en, &npc.place_zh);
            if !place.is_empty() {
                facts.push((
                    chrome_text(lang, "所在地", "Location"),
                    place,
                    String::new(),
                ));
            }
        }
        let objectives = objective_summary(meta, lang);
        if category != "standard" && !objectives.is_empty() {
            facts.push((
                chrome_text(lang, "目標類型", "Objective types"),
                objectives,
                String::new(),
            ));
        }
        facts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roadmap_summary_keeps_quantities_and_clips_unicode_safely() {
        let target = format!(
            "Item: {} × 12,345",
            "Very long localized quest item name ".repeat(4)
        );
        let summary = preview_summary(&[target], "");
        assert!(summary.chars().count() <= 60);
        assert!(summary.ends_with(" × 12,345"));
        assert!(summary.contains('…'));
        let many = preview_summary(
            &[
                "Defeat: Wolf × 20".into(),
                "Item: Very long second item objective that cannot fit here × 100".into(),
            ],
            "",
        );
        assert_eq!(many, "Defeat: Wolf × 20 …");
        let brief = preview_excerpt(&"任務說明與精確資料".repeat(50), 240);
        assert_eq!(brief.chars().count(), 240);
        assert!(brief.ends_with('…'));
        assert_eq!(preview_name("zh-en", "Pork", "豬肉", "돼지고기"), "豬肉 / Pork");
        assert_eq!(preview_name("en-zh", "Pork", "豬肉", "돼지고기"), "Pork / 豬肉");
        assert_eq!(preview_name("en-zh", "Same", "Same", ""), "Same");
        assert!(
            preview_brief(&"English note ".repeat(50), &"任務說明".repeat(100))
                .chars()
                .count()
                <= 400
        );
    }

    #[test]
    fn compact_rewards_keep_scalar_types_and_item_quantities() {
        let rewards = vec![
            format!(
                "{} × 12,345",
                "Long bilingual reward item 道具名稱 ".repeat(6)
            ),
            "Credit: 100".into(),
            "XP: 150,000".into(),
            "Second item × 2".into(),
        ];
        let summary = preview_reward_summary(&rewards, "en-zh");
        assert!(summary.chars().count() <= 100);
        assert!(summary.contains("Credit: 100"));
        assert!(summary.contains("XP: 150,000"));
        assert!(summary.contains(" × 12,345"));
        assert_eq!(summary.matches(" × 12,345").count(), 1);
        assert!(summary.ends_with(" · +1 more"));
        assert_eq!(preview_reward_summary(&[], "zh-en"), "未提供獎勵資料");
        // Omitted-count digit boundaries must still respect the row budget.
        for count in 1..=25 {
            let rewards: Vec<_> = (1..=count)
                .map(|quantity| format!("{} × {quantity}", "獎勵道具 ".repeat(15)))
                .collect();
            for lang in ["en-zh", "zh-en"] {
                assert!(preview_reward_summary(&rewards, lang).chars().count() <= 100);
            }
        }
    }

    #[test]
    fn reward_stats_keep_exact_compact_amounts_and_distinct_scalar_types() {
        assert_eq!(preview_compact_amount("150,000"), "150K");
        assert_eq!(preview_compact_amount("7,500,000"), "7.5M");
        assert_eq!(preview_compact_amount("1,234,567"), "1.234567M");
        assert_eq!(preview_compact_amount("1,001"), "1.001K");
        assert_eq!(preview_compact_amount("-1,250"), "-1.25K");
        assert_eq!(preview_compact_amount("100"), "100");
        assert_eq!(preview_compact_amount("unknown"), "unknown");
        let step = |kind: &str, code: &str, qty: &str| QuestStep {
            role: "reward".into(),
            kind: kind.into(),
            code: code.into(),
            qty: qty.into(),
            label: String::new(),
        };
        assert_eq!(
            preview_reward_stat(&step("Exp", "", "150000"), "en-zh").as_deref(),
            Some("XP 150K")
        );
        assert_eq!(
            preview_reward_stat(&step("ExpAll", "", "7500000"), "en-zh").as_deref(),
            Some("Party XP 7.5M")
        );
        assert_eq!(
            preview_reward_stat(&step("Credit", "", "100"), "en-zh").as_deref(),
            Some("Cr 100")
        );
        assert_eq!(
            preview_reward_stat(&step("Money", "1000", "0"), "en-zh").as_deref(),
            Some("$ 1K")
        );
        assert_eq!(
            preview_reward_stat(&step("ExpAll", "", "7500000"), "zh-en").as_deref(),
            Some("全體經驗 7.5M")
        );
        assert_eq!(
            preview_reward_stat(&step("Credit", "", "100"), "zh-en").as_deref(),
            Some("信用 100")
        );
        assert!(preview_reward_stat(&step("Item", "140", "2"), "en-zh").is_none());
        assert!(preview_reward_stat(&step("0", "", "2"), "en-zh").is_none());
        assert!(preview_reward_stat(&step("Exp", "0", "0"), "en-zh").is_none());
        assert!(preview_reward_items(&[], "en-zh").is_empty());
        assert_eq!(
            preview_reward_items(&["× 2 Pork".into(), "× 1 Flour".into()], "en-zh"),
            "× 2 Pork · × 1 Flour"
        );
        let item_summary = preview_reward_items(
            &[
                format!("× 20 {}", "Long item name ".repeat(10)),
                "× 2 Flour".into(),
            ],
            "en-zh",
        );
        assert!(item_summary.starts_with("× 20 "));
        assert!(item_summary.ends_with(" +1"));
        assert!(item_summary.chars().count() <= 36);
        let objective = preview_objective_summary(
            &[format!(
                "Defeat: {} × 100",
                "Long monster name 怪物 ".repeat(10)
            )],
            "",
        );
        assert!(objective.starts_with("Defeat: × 100 "));
        assert_eq!(objective.matches("× 100").count(), 1);
        assert!(objective.chars().count() <= 60);
    }

    #[test]
    fn list_summaries_batch_matches_individual_previews_in_both_languages() {
        let catalog = Catalog::open().expect("db");
        let fixtures = [
            ("main", "50000"),   // mission fallback and money
            ("main", "50001"),   // item plus XP, credit and item rewards
            ("main", "50002"),   // monster target
            ("main", "50016"),   // party XP
            ("main", "53020"),   // objective name absent from quest text
            ("main", "71624"),   // recurring quest with item reward
            ("letter", "10103"), // equipment requirement
            ("letter", "10108"), // use-item objective
        ];
        let ids: Vec<_> = fixtures
            .iter()
            .map(|(source, code)| {
                catalog
                    .index
                    .quests
                    .iter()
                    .find(|quest| quest.source == *source && quest.code == *code)
                    .unwrap()
                    .id
            })
            .collect();
        for lang in ["en-zh", "zh-en"] {
            let batch = catalog.quest_list_summaries(&ids, lang);
            assert_eq!(batch.len(), ids.len());
            for id in &ids {
                let individual = catalog.quest_map_preview(*id, lang);
                let row = &batch[id];
                assert_eq!(row.summary, individual.summary, "quest {id} / {lang}");
                let reward_lines: Vec<_> = individual.rewards.lines().map(str::to_string).collect();
                assert_eq!(
                    row.rewards,
                    preview_reward_summary(&reward_lines, lang),
                    "quest {id} / {lang}"
                );
                assert_eq!(
                    row.reward_items, individual.reward_items,
                    "quest {id} / {lang}"
                );
                assert_eq!(
                    row.reward_stats, individual.reward_stats,
                    "quest {id} / {lang}"
                );
            }
            let item_quest = &batch[&ids[1]];
            assert!(item_quest.summary.contains("× 1"));
            assert!(item_quest
                .rewards
                .contains(&format!("{}: 150,000", preview_reward_label("exp", lang))));
            assert!(item_quest
                .rewards
                .contains(&format!("{}: 100", preview_reward_label("credit", lang))));
        }
    }

    #[test]
    fn list_summaries_cover_all_quests_and_deduplicate_requested_ids() {
        let catalog = Catalog::open().expect("db");
        let mut ids: Vec<_> = catalog.index.quests.iter().map(|quest| quest.id).collect();
        let expected: HashSet<_> = ids.iter().copied().collect();
        ids.extend([ids[0], i64::MIN]);
        let rows = catalog.quest_list_summaries(&ids, "zh-en");
        assert_eq!(rows.len(), catalog.index.quests.len());
        assert_eq!(rows.keys().copied().collect::<HashSet<_>>(), expected);
        for (id, row) in rows {
            assert!(!row.summary.is_empty(), "quest {id}");
            assert!(!row.rewards.is_empty(), "quest {id}");
            assert!(row.summary.chars().count() <= 60, "quest {id}");
            assert!(row.rewards.chars().count() <= 100, "quest {id}");
            assert!(row.reward_items.chars().count() <= 36, "quest {id}");
        }
        assert!(catalog.quest_list_summaries(&[], "en-zh").is_empty());
        assert!(catalog
            .quest_list_summaries(&[i64::MIN], "en-zh")
            .is_empty());
    }

    #[test]
    fn roadmap_preview_preserves_item_and_monster_counts_and_separate_reward_types() {
        let catalog = Catalog::open().expect("db");
        let id = |code: &str| {
            catalog
                .index
                .quests
                .iter()
                .find(|quest| quest.source == "main" && quest.code == code)
                .unwrap()
                .id
        };
        let item_name = |code: &str, lang: &str| {
            let item = catalog
                .index
                .items
                .iter()
                .find(|item| item.code_str == code)
                .unwrap();
            preview_name(lang, &item.en, &item.zh, &item.ko)
        };
        let item_quest = catalog.quest_map_preview(id("50001"), "en-zh");
        assert!(item_quest.summary.contains("× 1"));
        assert!(item_quest
            .objectives
            .contains(&format!("Item: {} × 1", item_name("140", "en-zh"))));
        assert!(item_quest.objectives.contains("Level: 2"));
        assert!(item_quest.objectives.contains("Prerequisite:"));
        assert!(item_quest.rewards.lines().any(|line| line == "XP: 150,000"));
        assert!(item_quest.rewards.lines().any(|line| line == "Credit: 100"));
        assert!(item_quest
            .rewards
            .contains(&format!("{} × 1", item_name("8762", "en-zh"))));
        assert!(item_quest
            .rewards
            .contains(&format!("{} × 2", item_name("11385", "en-zh"))));
        assert!(item_quest.reward_items.starts_with("× 1 "));
        assert!(!item_quest.reward_items.contains(" / "));
        assert!(!item_quest.reward_items.contains("XP:"));
        assert!(!item_quest.reward_items.contains("Credit:"));
        assert!(item_quest.reward_stats.contains("XP 150K"));
        assert!(item_quest.reward_stats.contains("Cr 100"));
        assert!(!item_quest.reward_stats.contains('×'));
        assert!(item_quest.brief.chars().count() <= 400);
        let zh = catalog.quest_map_preview(id("50001"), "zh-en");
        assert!(zh
            .objectives
            .contains(&format!("道具: {} × 1", item_name("140", "zh-en"))));
        let en_paragraphs: Vec<_> = item_quest.brief.split("\n\n").collect();
        let zh_paragraphs: Vec<_> = zh.brief.split("\n\n").collect();
        assert_eq!(en_paragraphs.len(), 2);
        assert_eq!(en_paragraphs[0], zh_paragraphs[1]);
        assert_eq!(en_paragraphs[1], zh_paragraphs[0]);
        assert!(zh.rewards.lines().any(|line| line == "經驗: 150,000"));
        assert!(zh.rewards.lines().any(|line| line == "信用: 100"));
        assert!(zh.reward_stats.contains("經驗 150K"));
        assert!(zh.reward_stats.contains("信用 100"));
        let monster_quest = catalog.quest_map_preview(id("50002"), "en");
        assert!(monster_quest.summary.contains("× 20"));
        let monster = catalog
            .index
            .monsters
            .iter()
            .find(|monster| monster.code == "M25")
            .unwrap();
        assert!(monster_quest.objectives.contains(&format!(
            "Defeat: {} × 20",
            preview_name("en", &monster.en, &monster.zh, &monster.ko)
        )));
        assert!(catalog
            .quest_map_preview(id("50000"), "en")
            .rewards
            .lines()
            .any(|line| line == "Money: 1,000"));
        assert!(catalog
            .quest_map_preview(id("50016"), "en")
            .rewards
            .lines()
            .any(|line| line == "Party XP: 7,000,000"));
        assert!(catalog
            .quest_map_preview(id("50016"), "en")
            .reward_stats
            .contains("Party XP 7M"));
        let missing = catalog.quest_map_preview(i64::MIN, "en");
        assert_eq!(missing.summary, "Quest not found");
        assert!(missing.rewards.is_empty());
        assert!(missing.reward_items.is_empty());
        assert!(missing.reward_stats.is_empty());
        assert!(QuestMapPreview::default().summary.is_empty());
        let quest = catalog
            .index
            .quests
            .iter()
            .find(|quest| quest.id == id("50001"))
            .unwrap();
        assert_eq!(
            catalog.quest_alternate_title(quest.id, "zh-en"),
            quest.title_en
        );
        assert_eq!(
            catalog.quest_alternate_title(quest.id, "en-zh"),
            quest.title_zh
        );
        let npc = catalog.index.npc(quest.npc).unwrap();
        for lang in ["en-zh", "zh-en"] {
            let facts = catalog.quest_classification_facts(quest.id, lang);
            assert!(facts.iter().any(|(label, value, key)| {
                label == "NPC"
                    && value == &preview_name(lang, &npc.en, &npc.zh, &npc.ko)
                    && key == &format!("npc:{}", npc.code)
            }));
        }
    }

    #[test]
    fn roadmap_preview_includes_localized_giver_across_quest_sources() {
        let catalog = Catalog::open().expect("db");
        let mut sources = HashSet::new();
        for quest in &catalog.index.quests {
            let Some(npc) = catalog.index.npc(quest.npc) else {
                continue;
            };
            if !sources.insert(quest.source.as_str()) {
                continue;
            }
            for lang in ["en", "zh", "ko", "en-zh", "zh-en", "en-ko", "zh-ko", "ko-en", "ko-zh"] {
                let preview = catalog.quest_map_preview(quest.id, lang);
                assert_eq!(
                    preview.npc,
                    preview_name(lang, &npc.en, &npc.zh, &npc.ko),
                    "{} / {lang}",
                    quest.source,
                );
                assert_eq!(preview.npc_key, format!("npc:{}", npc.code));
                let place = preview_name(lang, &npc.place_en, &npc.place_zh, "");
                let location = if place.is_empty() {
                    preview_name(lang, &npc.map_en, &npc.map_zh, &npc.map_ko)
                } else {
                    place
                };
                assert_eq!(preview.npc_location, location, "{} / {lang}", quest.source);
            }
        }
        assert!(sources.contains("main"));
        assert!(sources.len() > 1, "exercise more than the main quest source");
    }

    #[test]
    fn roadmap_preview_keeps_missing_givers_and_locations_honest() {
        let mut catalog = Catalog::open().expect("db");
        let quest_index = catalog
            .index
            .quests
            .iter()
            .position(|quest| catalog.index.npc(quest.npc).is_some())
            .expect("quest with a known NPC");
        let id = catalog.index.quests[quest_index].id;
        let npc_code = catalog.index.quests[quest_index].npc;
        let npc = catalog
            .index
            .npcs
            .iter_mut()
            .find(|npc| npc.code == npc_code)
            .unwrap();
        npc.place_en.clear();
        npc.place_zh.clear();
        npc.map_en = "Map name".into();
        npc.map_zh = "地圖名稱".into();
        npc.map_ko = "지도 이름".into();
        assert_eq!(
            catalog.quest_map_preview(id, "zh-en").npc_location,
            "地圖名稱 / Map name"
        );
        assert_eq!(
            catalog.quest_map_preview(id, "ko").npc_location,
            "지도 이름"
        );

        catalog.index.quests[quest_index].npc = i64::MAX;
        let unknown = catalog.quest_map_preview(id, "en");
        assert_eq!(unknown.npc, i64::MAX.to_string());
        assert!(unknown.npc_key.is_empty());
        assert!(unknown.npc_location.is_empty());

        catalog.index.quests[quest_index].npc = 0;
        let absent = catalog.quest_map_preview(id, "en");
        assert!(absent.npc.is_empty());
        assert!(absent.npc_key.is_empty());
        assert!(absent.npc_location.is_empty());
        let missing = catalog.quest_map_preview(i64::MIN, "en");
        assert!(missing.npc.is_empty());
        assert!(missing.npc_key.is_empty());
        assert!(missing.npc_location.is_empty());
    }

    #[test]
    fn quest_search_finds_translated_required_and_reward_items_outside_quest_text() {
        let catalog = Catalog::open().expect("db");
        let target = catalog
            .index
            .quests
            .iter()
            .find(|quest| quest.source == "main" && quest.code == "53020")
            .unwrap();
        let item = catalog
            .index
            .items
            .iter()
            .find(|item| item.code_str == "2565")
            .unwrap();
        for query in [&item.en, &item.zh] {
            assert!(!query.is_empty());
            assert!(
                !target.names.contains(&query.to_lowercase()),
                "fixture must exercise new item search, not existing quest text"
            );
            for lang in ["zh-en", "en-zh"] {
                assert!(
                    catalog
                        .quest_category_records("standard:items", query, lang)
                        .iter()
                        .any(|hit| hit.key == format!("quest:{}", target.id)),
                    "{query} / {lang}"
                );
            }
        }
        assert!(catalog
            .quest_category_records("standard:items", "#2565", "en")
            .iter()
            .any(|hit| hit.key == format!("quest:{}", target.id)));
        let reward = catalog
            .index
            .items
            .iter()
            .find(|item| item.code_str == "4469")
            .unwrap();
        for query in [&reward.en, &reward.zh] {
            let all: HashSet<_> = catalog
                .quest_category_records("all", query, "zh-en")
                .into_iter()
                .map(|hit| hit.key)
                .collect();
            for (category, code) in [("daily", "71624"), ("weekly", "71693")] {
                let quest = catalog
                    .index
                    .quests
                    .iter()
                    .find(|quest| quest.source == "main" && quest.code == code)
                    .unwrap();
                assert!(!quest.names.contains(&query.to_lowercase()));
                let bucket: HashSet<_> = catalog
                    .quest_category_records(category, "", "en")
                    .into_iter()
                    .map(|hit| hit.key)
                    .collect();
                let actual: HashSet<_> = catalog
                    .quest_category_records(category, query, "en-zh")
                    .into_iter()
                    .map(|hit| hit.key)
                    .collect();
                assert!(actual.contains(&format!("quest:{}", quest.id)));
                assert_eq!(
                    actual,
                    all.intersection(&bucket).cloned().collect(),
                    "{category} / {query}"
                );
            }
        }
        let starter = catalog
            .index
            .quests
            .iter()
            .find(|quest| quest.source == "main" && quest.code == "50001")
            .unwrap();
        for query in ["XP 150,000", "Credit 100", "經驗 150000", "信用 100"] {
            assert!(
                catalog
                    .quest_category_records("starter", query, "en")
                    .iter()
                    .any(|hit| hit.key == format!("quest:{}", starter.id)),
                "{query}"
            );
        }
    }

    #[test]
    fn repeat_semantics_are_scoped_to_main_and_take_precedence_over_starter() {
        assert_eq!(primary_category("main", "0", "1"), "daily");
        assert_eq!(primary_category("main", "5", "2"), "weekly");
        assert_eq!(primary_category("main", "7", "0"), "scheduled");
        assert_eq!(primary_category("main", "0", "0"), "starter");
        assert_eq!(primary_category("intro", "重複", "3"), "intro");
        assert_eq!(primary_category("dispatch", "0", "1"), "dispatch");
        assert_eq!(primary_category("unknown-source", "0", "1"), "other");
    }

    #[test]
    fn standard_subcategories_require_explicit_objectives_and_stay_disjoint() {
        let classify = |objectives: &[&'static str]| {
            standard_category(&Metadata {
                objectives: objectives.iter().copied().collect(),
                ..Default::default()
            })
        };
        assert_eq!(classify(&[]), "unspecified");
        assert_eq!(classify(&["item"]), "items");
        assert_eq!(classify(&["combat"]), "combat");
        assert_eq!(classify(&["item", "combat"]), "mixed");
        assert_eq!(classify(&["other"]), "other");
        assert_eq!(classify(&["talk"]), "other");
        assert_eq!(classify(&["combat", "talk"]), "other");
        let meta = Metadata {
            objectives: HashSet::from(["combat"]),
            ..Default::default()
        };
        assert!(category_matches("standard:combat", "standard", &meta));
        assert!(!category_matches("standard:combat", "daily", &meta));
        assert!(!category_matches("standard:items", "standard", &meta));
        assert!(!category_matches("standard:missing", "standard", &meta));
    }

    #[test]
    fn objective_classification_does_not_invent_talk_or_character_level() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE quests (id INTEGER, repeat_count TEXT);
             CREATE TABLE quest_steps (quest_id INTEGER, role TEXT, kind TEXT, qty TEXT);
             INSERT INTO quests VALUES (1, '0'), (2, '0'), (3, '0');
             INSERT INTO quest_steps VALUES (1, 'requirement', 'Level1', '20'),
                (2, 'objective', 'NPC', '1'), (2, 'requirement', 'Level', '30'),
                (3, 'objective', 'Monster', '5'), (3, 'objective', 'ITEM', '10');",
        )
        .unwrap();
        let metadata = load_metadata(&conn);
        assert!(metadata[&1].objectives.is_empty());
        assert_eq!(metadata[&1].level, None);
        assert_eq!(metadata[&2].level, Some(30));
        assert_eq!(objective_summary(&metadata[&2], "en"), "Talk");
        assert_eq!(objective_summary(&metadata[&3], "zh"), "討伐 / 物品");
    }

    #[test]
    fn live_categories_partition_all_quests_and_counts_match_records() {
        let catalog = Catalog::open().expect("db");
        let categories = catalog.quest_categories("en");
        let mut seen = HashSet::new();
        let all = catalog.quest_category_records("all", "", "en");
        assert_eq!(all.len(), catalog.index.quests.len());
        assert_eq!(categories[0].aside.parse::<usize>().unwrap(), all.len());
        for category in categories.iter().skip(1) {
            let rows = catalog.quest_category_records(&category.key, "", "en");
            assert_eq!(category.aside.parse::<usize>().unwrap(), rows.len());
            for row in rows {
                assert!(
                    seen.insert(row.key),
                    "a quest must belong to only one primary category"
                );
            }
        }
        assert_eq!(seen, all.into_iter().map(|row| row.key).collect());
        assert!(catalog.quest_category_records("standard", "", "en").len() > 300);
        let daily = catalog.quest_category_records("daily", "", "en");
        let expected_daily: usize = catalog
            .conn
            .query_row(
                "SELECT count(*) FROM quests WHERE source = 'main' AND repeat_count = '1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(daily.len(), expected_daily);
        for quest in catalog
            .index
            .quests
            .iter()
            .filter(|quest| quest.source == "main" && quest.kind == "0")
        {
            let repeat: String = catalog
                .conn
                .query_row(
                    "SELECT repeat_count FROM quests WHERE id = ?1",
                    [quest.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                daily
                    .iter()
                    .any(|hit| hit.key == format!("quest:{}", quest.id)),
                repeat == "1"
            );
        }
    }

    #[test]
    fn categories_search_localized_names_codes_and_metadata() {
        let catalog = Catalog::open().expect("db");
        let quest = catalog
            .index
            .quests
            .iter()
            .find(|quest| {
                quest.source == "main" && !quest.title_en.is_empty() && !quest.title_zh.is_empty()
            })
            .expect("bilingual quest");
        let key = format!("quest:{}", quest.id);
        for query in [
            &quest.title_en,
            &quest.title_zh,
            &quest.code,
            &format!("#{}", quest.code),
        ] {
            assert!(
                catalog
                    .quest_category_records("all", query, "en")
                    .iter()
                    .any(|hit| hit.key == key),
                "{query}"
            );
        }
        let daily = catalog.quest_category_records("daily", "", "zh");
        assert_eq!(
            catalog
                .quest_category_records("daily", "Daily quests", "zh")
                .len(),
            daily.len()
        );
        assert_eq!(
            catalog
                .quest_category_records("daily", "每日任務", "en")
                .len(),
            daily.len()
        );
        assert!(catalog
            .quest_category_records("not-a-category", "", "en")
            .is_empty());
    }

    #[test]
    fn standard_subcategories_partition_parent_and_intersect_search() {
        let catalog = Catalog::open().expect("db");
        let parent = catalog.quest_category_records("standard", "", "en");
        let expected: HashSet<_> = parent.iter().map(|row| row.key.clone()).collect();
        let categories = catalog.quest_standard_categories("en");
        assert_eq!(categories.len(), 6);
        assert_eq!(categories[0].key, "qcat:standard");
        assert_eq!(categories[0].aside.parse::<usize>().unwrap(), parent.len());
        let mut seen = HashSet::new();
        for category in categories.iter().skip(1) {
            let rows = catalog.quest_category_records(&category.key, "", "en");
            assert_eq!(category.aside.parse::<usize>().unwrap(), rows.len());
            let bucket: HashSet<_> = rows.iter().map(|row| row.key.clone()).collect();
            for row in &rows {
                assert!(seen.insert(row.key.clone()), "secondary categories overlap");
            }
            for query in ["wolf", "狼", "#71447", category.title.as_str()] {
                let expected: HashSet<_> = catalog
                    .quest_category_records("standard", query, "en")
                    .into_iter()
                    .map(|row| row.key)
                    .filter(|key| bucket.contains(key))
                    .collect();
                let actual: HashSet<_> = catalog
                    .quest_category_records(&category.key, query, "en")
                    .into_iter()
                    .map(|row| row.key)
                    .collect();
                assert_eq!(actual, expected, "{} / {query}", category.key);
            }
            if let Some(first) = rows.first() {
                let id = first.key.strip_prefix("quest:").unwrap().parse().unwrap();
                let facts = catalog.quest_classification_facts(id, "en");
                assert!(facts.iter().any(|(_, _, key)| key == "qcat:standard"));
                assert!(facts.iter().any(|(label, value, key)| {
                    label == "Objective" && value == &category.title && key.is_empty()
                }));
            }
        }
        assert_eq!(seen, expected);
    }
}
