use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rusqlite::Connection;

use super::{Catalog, Hit};
use super::character_data::{companion_summary, load_companions};
use super::names::{NameTrie, fuzzy_rows, haystack, plain_name};
use super::paths::resolve_client_file;
use super::rows::{ChainBook, ChainStep, Combo, HeroRow, HeroSkill, Index, ItemRow, RosterUse};
use super::store::{online, table_exists};
use super::text::{caption, commas, lead, phrase, trail};
use super::work::chain_role;

pub(in crate::db) fn hero_sheet_parts(hero: &HeroRow, lang: &str) -> (String, String, String, Vec<(String, String)>, String) {
    let (title, alt) = hero_names(lang, hero);
    let (en, zh) = hero_rank_pair(hero.rank);
    let rank = lead(lang, en, zh);
    let heading = if rank.is_empty() {
        hero.code.clone()
    } else {
        format!("{rank}  {}", hero.code)
    };
    let mut facts = Vec::new();
    let nation = lead(lang, &hero.nation_en, &hero.nation_zh);
    if !nation.is_empty() && hero.rank < 13 {
        facts.push((phrase("nation", lang), nation));
    }
    let hire = tidy(&lead(lang, &hero.hire_en, &hero.hire_zh));
    if !hire.is_empty() {
        facts.push((phrase("hire", lang), hire));
    }
    for skill in &hero.skills {
        // Empty skill slots in the source use the literal "0". Clean each
        // language before fallback so a missing translation cannot hide a real one.
        let clean = |value: &str| {
            let value = tidy(value);
            if value == "0" { String::new() } else { value }
        };
        let name = clean(&skill.name);
        let text = lead(lang, &clean(&skill.en), &clean(&skill.zh));
        let shown = if text.is_empty() { name.clone() } else { text };
        if shown.is_empty() {
            continue;
        }
        let label = if name.is_empty() { phrase("skill", lang) } else { name };
        facts.push((label, shown));
    }
    (title, alt, heading, facts, tidy(&lead(lang, &hero.desc_en, &hero.desc_zh)))
}

pub(in crate::db) fn hero_names(lang: &str, hero: &HeroRow) -> (String, String) {
    let title = lead(lang, &hero.en, &hero.zh);
    let alt = trail(lang, &hero.en, &hero.zh);
    if title.is_empty() {
        let ko = plain_name(&hero.ko);
        if ko.is_empty() { (hero.code.clone(), String::new()) } else { (ko, String::new()) }
    } else {
        (title, alt)
    }
}

pub(in crate::db) fn hero_rank_pair(rank: i64) -> (&'static str, &'static str) {
    match rank {
        0 => ("Mercenary", "傭兵"),
        1 => ("1st general", "一次將帥"),
        2 => ("2nd general", "二次將帥"),
        3 => ("Remodel", "改造"),
        4 => ("Monster", "怪物"),
        5 => ("Promoted", "轉職怪物"),
        6 => ("Summon", "幻獸"),
        7 => ("Fiend", "凶獸"),
        8 => ("Divine", "神獸"),
        9 => ("Four Kings", "四天王"),
        10 => ("Awakened", "覺醒"),
        11 => ("Legend", "傳說"),
        12 => ("Wisdom King", "明王"),
        13 => ("Mounts", "坐騎"),
        14 => ("Spirit", "精靈"),
        _ => ("", ""),
    }
}

pub(in crate::db) fn hero_tier_pair(rank: i64) -> (&'static str, &'static str) {
    hero_rank_pair(rank)
}

pub(in crate::db) fn hero_land_pair(mask: i64) -> (&'static str, &'static str) {
    match mask {
        1 => ("Joseon", "朝鮮"),
        10 => ("Japan", "日本"),
        100 => ("China", "中國"),
        1000 => ("Taiwan", "台灣"),
        10000 => ("India", "印度"),
        _ => ("Other", "其他"),
    }
}

pub(in crate::db) fn hero_land_order(mask: i64) -> usize {
    match mask {
        1 => 0,
        10 => 1,
        100 => 2,
        1000 => 3,
        10000 => 4,
        _ => 5,
    }
}

pub(in crate::db) fn hero_rank_order(rank: i64) -> usize {
    [0, 1, 2, 3, 10, 11, 12, 9, 6, 7, 8, 5, 4, 13, 14]
        .iter()
        .position(|item| *item == rank)
        .unwrap_or(50)
}

pub(in crate::db) fn hero_nation(code: i64) -> (String, String) {
    let (en, zh) = hero_land_pair(code);
    (en.to_string(), zh.to_string())
}

pub(in crate::db) fn tidy(text: &str) -> String {
    text.replace("\\n", " ").replace(['\r', '\n'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(in crate::db) fn drop_leading_name(text: &str, name: &str) -> String {
    let name = name.trim();
    if name.chars().count() < 2 {
        return text.to_string();
    }
    let Some(rest) = text.trim().strip_prefix(name) else {
        return text.to_string();
    };
    rest.trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '·' | '：' | ':' | ',' | '，' | '、'))
        .trim()
        .to_string()
}

pub(in crate::db) fn requirement_line(raw: &str) -> String {
    let raw = raw.replace("\\n", "\n");
    let mut lines = raw.lines().map(str::trim).filter(|line| !line.is_empty());
    let head = lines.next().unwrap_or("");
    let mut text = head.to_string();
    for line in lines {
        if line.contains("信用") || line.to_lowercase().contains("credit") {
            text.push(' ');
            text.push_str(line);
        }
    }
    scrub(&tidy(&text))
}

pub(in crate::db) fn scrub(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if matches!(ch, ',' | '，' | '、' | '[' | ']' | '［' | '］' | '<' | '>' | '＜' | '＞') {
            continue;
        }
        out.push(ch);
    }
    tidy(&out)
}

pub(in crate::db) fn credit_note(text: &str) -> String {
    let text = text.replace("\\n", " ");
    let start = text.find('(').or_else(|| text.find('（'));
    let Some(start) = start else { return String::new() };
    let rest = &text[start..];
    let end = rest.find(')').or_else(|| rest.find('）')).map(|index| index + 1).unwrap_or(rest.len());
    let bit = scrub(&tidy(&rest[..end]));
    if bit.contains("信用") || bit.to_lowercase().contains("credit") { bit } else { String::new() }
}

pub(in crate::db) fn load_chains(conn: &Connection, texts: &HashMap<String, String>, items: &[ItemRow]) -> Result<ChainBook, String> {
    if !table_exists(conn, "job_steps") {
        return Ok(ChainBook { lines: HashMap::new(), combos: HashMap::new(), uses: HashMap::new() });
    }
    let names = NameTrie::from_items(items);
    let has_kind = conn
        .prepare("SELECT kind FROM job_steps LIMIT 1")
        .is_ok();
    let sql = if has_kind {
        "SELECT chain_id, slot, code, note_gts, kind FROM job_steps ORDER BY chain_id, slot"
    } else {
        "SELECT chain_id, slot, code, note_gts, 2 FROM job_steps ORDER BY chain_id, slot"
    };
    let mut stmt = conn.prepare(sql).map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut grouped: HashMap<i64, (i64, Vec<ChainStep>)> = HashMap::new();
    for row in rows {
        let (chain_id, slot, code, note, kind) = row.map_err(|err| err.to_string())?;
        let en_raw = online(texts, note, "en").to_string();
        let zh_raw = online(texts, note, "zh").to_string();
        let en_found = names.find(&en_raw);
        let zh_found = names.find(&zh_raw);
        let materials = if zh_found.items.is_empty() { en_found.items.clone() } else { zh_found.items.clone() };
        let en = if materials.is_empty() { en_raw } else { requirement_line(&en_raw) };
        let zh = if materials.is_empty() { zh_raw } else { requirement_line(&zh_raw) };
        grouped.entry(chain_id).or_insert_with(|| (kind, Vec::new())).1.push(ChainStep {
            slot,
            code,
            en,
            zh,
            materials,
        });
    }
    let extras = load_job_materials(conn)?;
    let mut lines = HashMap::new();
    let mut combos: HashMap<String, Vec<Combo>> = HashMap::new();
    let mut uses: HashMap<i64, Vec<RosterUse>> = HashMap::new();
    for (chain_id, (kind, mut steps)) in grouped {
        if steps.len() < 2 {
            continue;
        }
        if let Some(extra) = extras.get(&chain_id) {
            if let Some(last) = steps.last_mut() {
                last.materials.extend(extra.clone());
            }
        }
        if kind == 1 || kind == 3 {
            let result = steps.last().map(|step| step.code.clone()).unwrap_or_default();
            let parts: Vec<String> = steps.iter().take(steps.len() - 1).map(|step| step.code.clone()).collect();
            let owner = steps.iter().find(|step| !step.materials.is_empty()).unwrap_or(&steps[0]);
            let recipe = Combo {
                result: result.clone(),
                parts: parts.clone(),
                en: owner.en.clone(),
                zh: owner.zh.clone(),
                materials: owner.materials.clone(),
            };
            for code in parts.iter().chain(std::iter::once(&result)) {
                combos.entry(code.clone()).or_default().push(recipe.clone());
            }
            let source = parts.first().cloned().unwrap_or_default();
            for (item, qty) in &recipe.materials {
                remember_use(&mut uses, *item, *qty, result.clone(), source.clone(), true, recipe.zh.clone());
            }
            continue;
        }
        for (index, step) in steps.iter().enumerate() {
            if step.materials.is_empty() {
                continue;
            }
            let target = steps.get(index + 1).map(|next| next.code.clone()).unwrap_or_else(|| step.code.clone());
            for (item, qty) in &step.materials {
                remember_use(&mut uses, *item, *qty, target.clone(), step.code.clone(), false, step.zh.clone());
            }
        }
        for step in &steps {
            lines.insert(step.code.clone(), steps.clone());
        }
    }
    Ok(ChainBook { lines, combos, uses })
}

pub(in crate::db) fn load_job_materials(conn: &Connection) -> Result<HashMap<i64, Vec<(i64, i64)>>, String> {
    let mut found: HashMap<i64, Vec<(i64, i64)>> = HashMap::new();
    if !table_exists(conn, "job_materials") {
        return Ok(found);
    }
    let mut stmt = conn
        .prepare("SELECT chain_id, item_code, qty FROM job_materials ORDER BY chain_id, slot")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)))
        .map_err(|err| err.to_string())?;
    for row in rows {
        let (chain_id, item, qty) = row.map_err(|err| err.to_string())?;
        found.entry(chain_id).or_default().push((item, qty));
    }
    Ok(found)
}

pub(in crate::db) fn remember_use(uses: &mut HashMap<i64, Vec<RosterUse>>, item: i64, qty: i64, target: String, source: String, combo: bool, note: String) {
    let rows = uses.entry(item).or_default();
    if rows.iter().any(|row| row.target == target && row.qty == qty && row.combo == combo) {
        return;
    }
    rows.push(RosterUse { qty, target, source, combo, note });
}

#[derive(Clone, Copy)]
pub(in crate::db) struct FixedPart {
    pub(in crate::db) item: i64,
    pub(in crate::db) qty: i64,
    pub(in crate::db) zh: &'static str,
    pub(in crate::db) en: &'static str,
    pub(in crate::db) hint_zh: &'static str,
    pub(in crate::db) hint_en: &'static str,
}

pub(in crate::db) const fn part(item: i64, qty: i64, hint_zh: &'static str, hint_en: &'static str) -> FixedPart {
    FixedPart { item, qty, zh: "", en: "", hint_zh, hint_en }
}

pub(in crate::db) fn fixed_beasts() -> &'static [(&'static str, &'static [FixedPart])] {
    const NOTE: FixedPart = FixedPart {
        item: 0,
        qty: 3,
        zh: "不同幻獸石像",
        en: "Different fey statues",
        hint_zh: "",
        hint_en: "",
    };
    const STATUES: &[FixedPart] = &[
        part(3817, 0, "", ""),
        part(3828, 0, "", ""),
        part(3839, 0, "", ""),
        part(3850, 0, "", ""),
    ];
    const TIGER: &[FixedPart] = &[NOTE, STATUES[0], STATUES[1], STATUES[2], STATUES[3], part(3895, 10, "", ""), part(3506, 3, "", "")];
    const BIRD: &[FixedPart] = &[NOTE, STATUES[0], STATUES[1], STATUES[2], STATUES[3], part(3896, 10, "", ""), part(3506, 3, "", "")];
    const TORTOISE: &[FixedPart] = &[NOTE, STATUES[0], STATUES[1], STATUES[2], STATUES[3], part(3897, 10, "", ""), part(3506, 3, "", "")];
    const DRAGON: &[FixedPart] = &[NOTE, STATUES[0], STATUES[1], STATUES[2], STATUES[3], part(3898, 10, "", ""), part(3506, 3, "", "")];
    const KIRIN: &[FixedPart] = &[
        part(3894, 1, "", ""),
        part(3861, 1, "", ""),
        part(3872, 1, "", ""),
        part(3883, 1, "", ""),
        part(3899, 10, "", ""),
        part(3506, 4, "", ""),
    ];
    &[
        ("E74", TIGER),
        ("E75", BIRD),
        ("E76", DRAGON),
        ("E77", TORTOISE),
        ("E85", KIRIN),
    ]
}

pub(in crate::db) fn fixed_credit(code: &str) -> i64 {
    match code {
        "E85" => 100,
        "E74" | "E75" | "E76" | "E77" => 95,
        _ => 0,
    }
}

pub(in crate::db) fn fixed_beast(code: &str) -> Option<&'static [FixedPart]> {
    fixed_beasts().iter().find(|(hero, _)| *hero == code).map(|(_, parts)| *parts)
}

pub(in crate::db) fn fixed_beast_items(item: i64) -> Vec<(&'static str, i64)> {
    let mut found = Vec::new();
    for (hero, parts) in fixed_beasts() {
        for part in *parts {
            if part.item == item {
                found.push((*hero, part.qty));
            }
        }
    }
    found
}

pub(in crate::db) fn hero_section(key: &str, lang: &str) -> Hit {
    Hit {
        title: if key == "related" { phrase("related", lang) } else { phrase("craft-need", lang) },
        subtitle: String::new(),
        key: key.to_string(),
        icon: None,
        solo: false,
        aside: String::new(),
        badge: None,
        chip: "section".into(),
    }
}

pub(in crate::db) fn roster_kind(note: &str, target_zh: &str, combo: bool, lang: &str) -> String {
    let key = if combo {
        "combine"
    } else if note.contains("覺醒") || target_zh.contains("覺醒") {
        "awaken"
    } else if note.contains("改造") {
        "remodel"
    } else if note.contains("轉職") {
        "promote"
    } else {
        "upgrade"
    };
    phrase(key, lang)
}

pub(in crate::db) fn load_heroes(
    conn: &Connection,
    asset_roots: &[PathBuf],
    texts: &HashMap<String, String>,
) -> Result<Vec<HeroRow>, String> {
    if !table_exists(conn, "characters") {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT code, nation, rank, name_ko, name_gts, desc_gts, hire_gts, portrait_path, portrait_frame, sprite_path
             FROM characters",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let skills = load_hero_skills(conn, texts)?;
    let mut companions = load_companions(conn);
    let mut heroes = Vec::new();
    for row in rows {
        let (code, nation, rank, ko, name_gts, desc_gts, hire_gts, portrait, frame, sprite) =
            row.map_err(|err| err.to_string())?;
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        let desc_en = online(texts, desc_gts, "en").to_string();
        let desc_zh = online(texts, desc_gts, "zh").to_string();
        let hire_en = online(texts, hire_gts, "en").to_string();
        let hire_zh = online(texts, hire_gts, "zh").to_string();
        let (nation_en, nation_zh) = hero_nation(nation);
        let (rank_en, rank_zh) = hero_rank_pair(rank);
        let mine = skills.get(&code).cloned().unwrap_or_default();
        let companion = companions.remove(&code).or_else(|| matches!(rank, 13 | 14).then(Default::default));
        let mut parts = vec![ko.clone(), plain_name(&ko), en.clone(), zh.clone(), nation_en.clone(), nation_zh.clone()];
        if let Some(companion) = &companion {
            parts.push(companion_summary(companion, rank, "en"));
            parts.push(companion_summary(companion, rank, "zh"));
        }
        for skill in &mine {
            parts.push(skill.name.clone());
            parts.push(skill.en.clone());
            parts.push(skill.zh.clone());
        }
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).chain([rank_en, rank_zh]).collect();
        heroes.push(HeroRow {
            code: code.clone(),
            nation,
            rank,
            ko,
            en,
            zh,
            nation_en,
            nation_zh,
            desc_en,
            desc_zh,
            hire_en,
            hire_zh,
            names: haystack(&borrowed),
            skills: mine,
            icon: hero_icon(asset_roots, &portrait, frame, &sprite),
            companion,
        });
    }
    heroes.sort_by(hero_order);
    Ok(heroes)
}

fn hero_order(a: &HeroRow, b: &HeroRow) -> std::cmp::Ordering {
    let bucket = |hero: &HeroRow| match hero.rank { 13 => 1, 14 => 2, _ => 0 };
    bucket(a).cmp(&bucket(b)).then_with(|| {
        match a.rank {
            13 if b.rank == 13 => {
                let level = |hero: &HeroRow| hero.companion.as_ref().and_then(|row| row.riding_level).unwrap_or(i64::MAX);
                level(a).cmp(&level(b)).then(a.code.cmp(&b.code))
            }
            14 if b.rank == 14 => {
                fn key(hero: &HeroRow) -> (i64, i64, &str, i64, &str) {
                    let row = hero.companion.as_ref();
                    let element = row.and_then(|row| row.family_element).map(|value| if value == 0 { 6 } else { value }).unwrap_or(i64::MAX);
                    let grade = row.and_then(|row| row.spirit_grade).unwrap_or(i64::MAX);
                    let root = row.and_then(|row| row.evolution_root.as_deref()).unwrap_or(&hero.code);
                    let order = row.and_then(|row| row.evolution_order).unwrap_or(i64::MAX);
                    (element, grade, root, order, &hero.code)
                }
                key(a).cmp(&key(b))
            }
            _ => a.zh.cmp(&b.zh).then(a.en.cmp(&b.en)).then(a.code.cmp(&b.code)),
        }
    })
}

fn hero_region_order(a: &HeroRow, b: &HeroRow) -> std::cmp::Ordering {
    hero_rank_order(a.rank).cmp(&hero_rank_order(b.rank))
        .then(a.rank.cmp(&b.rank))
        .then_with(|| hero_order(a, b))
}

pub(in crate::db) fn load_hero_skills(conn: &Connection, texts: &HashMap<String, String>) -> Result<HashMap<String, Vec<HeroSkill>>, String> {
    if !table_exists(conn, "character_skills") {
        return Ok(HashMap::new());
    }
    let mut stmt = conn
        .prepare("SELECT code, name_ko, desc_gts FROM character_skills ORDER BY code, slot")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))
        .map_err(|err| err.to_string())?;
    let mut skills: HashMap<String, Vec<HeroSkill>> = HashMap::new();
    for row in rows {
        let (code, name, gts) = row.map_err(|err| err.to_string())?;
        skills.entry(code).or_default().push(HeroSkill {
            name,
            en: online(texts, gts, "en").to_string(),
            zh: online(texts, gts, "zh").to_string(),
        });
    }
    Ok(skills)
}

pub(in crate::db) fn hero_icon(client_roots: &[PathBuf], portrait: &str, frame: i64, sprite: &str) -> Option<(PathBuf, i32)> {
    if !portrait.is_empty() && frame >= 0 {
        if let Some(path) = client_roots.iter().find_map(|root| resolve_client_file(root, portrait)) {
            return Some((path, frame as i32));
        }
    }
    if sprite.is_empty() {
        return None;
    }
    client_roots.iter().find_map(|root| resolve_client_file(root, sprite)).map(|path| (path, 0))
}

impl Catalog {
    pub fn hero_region_ranks(&self, nation: &str, lang: &str) -> Vec<Hit> {
        let Ok(nation) = nation.parse::<i64>() else { return Vec::new() };
        self.index.hero_rank_groups(Some(nation), lang, false)
    }

    pub fn hero_region_records(&self, nation: &str, rank: &str, lang: &str) -> Vec<Hit> {
        if nation.parse::<i64>().is_err() || rank.parse::<i64>().is_err() {
            return Vec::new();
        }
        self.index.hero_hits("", "", lang, rank, nation, usize::MAX)
    }

    pub fn hero_is_spirit(&self, code: &str) -> bool {
        self.index.heroes.iter().any(|hero| hero.code == code && hero.rank == 14)
    }

    pub fn hero_sheet(&self, code: &str, lang: &str) -> Option<(String, String, String, Vec<(String, String)>, String)> {
        let hero = self.index.heroes.iter().find(|hero| hero.code == code)?;
        let (title, alt, heading, mut facts, note) = hero_sheet_parts(hero, lang);
        let mut details = self.hero_data_facts(code, lang);
        details.append(&mut facts);
        Some((title, alt, heading, details, note))
    }

    pub fn hero_portrait(&self, code: &str) -> Option<(PathBuf, i32)> {
        self.index.heroes.iter().find(|hero| hero.code == code).and_then(|hero| hero.icon.clone())
    }

    pub fn hero_battle_sprite(&self, code: &str) -> Option<(PathBuf, i32)> {
        let relative: String = self.conn.query_row(
            "SELECT sprite_path FROM characters WHERE code = ?1", [code], |row| row.get(0),
        ).ok()?;
        if relative.is_empty() {
            return None;
        }
        self.character_file(&relative).map(|path| (path, 0))
    }

    pub fn hero_mark(&self) -> Option<(PathBuf, i32)> {
        self.index.heroes.iter().find(|hero| hero.code == "U25").and_then(|hero| hero.icon.clone())
    }

    pub(in crate::db) fn open_hero(&self, code: &str, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        if !self.index.heroes.iter().any(|hero| hero.code == code) {
            return (phrase("missing-hero", lang), Vec::new(), None);
        }
        let (name, alt, heading, facts, note) = self.hero_sheet(code, lang).expect("indexed hero");
        let mut all_facts = self.hero_effect_lines(code, lang);
        all_facts.extend(self.hero_stat_lines(code, lang));
        all_facts.extend(facts);
        let mut links = self.index.hero_chain(code, lang);
        links.extend(self.hero_data_links(code, lang));
        let mut text = name.clone();
        if !alt.is_empty() {
            text.push_str(&format!("\n{alt}"));
        }
        if !heading.is_empty() {
            text.push_str(&format!("\n{heading}"));
        }
        for (label, value) in &all_facts {
            text.push_str(&format!("\n{label}  {value}"));
        }
        if !note.is_empty() {
            text.push_str(&format!("\n{note}"));
        }
        (text, links, self.hero_portrait(code))
    }
}

impl Index {
    fn representative_hero_icon(&self, rank: Option<i64>, nation: Option<i64>) -> Option<(PathBuf, i32)> {
        self.heroes.iter()
            .filter(|hero| rank.is_none_or(|rank| hero.rank == rank) && nation.is_none_or(|nation| hero.nation == nation))
            .filter(|hero| hero.icon.is_some())
            .min_by_key(|hero| {
                let is_portrait = hero.icon.as_ref().is_some_and(|(path, _)| {
                    path.to_string_lossy().to_ascii_lowercase().contains("portrait")
                });
                (!is_portrait, &hero.code)
            })
            .and_then(|hero| hero.icon.clone())
    }

    pub(in crate::db) fn hero_rank_hits(&self, lang: &str) -> Vec<Hit> {
        self.hero_rank_groups(None, lang, false)
    }

    fn hero_rank_groups(&self, nation: Option<i64>, lang: &str, legacy_tier: bool) -> Vec<Hit> {
        let mut counts: HashMap<i64, i64> = HashMap::new();
        for hero in self.heroes.iter().filter(|hero| nation.is_none_or(|nation| hero.nation == nation)) {
            *counts.entry(hero.rank).or_default() += 1;
        }
        let mut ranks: Vec<i64> = counts.keys().copied().collect();
        ranks.sort_by_key(|rank| (hero_rank_order(*rank), *rank));
        ranks
            .into_iter()
            .map(|rank| {
                let (en, zh) = if legacy_tier { hero_tier_pair(rank) } else { hero_rank_pair(rank) };
                let count = format!("{} {}", commas(counts[&rank]), phrase("heroes", lang));
                let title = lead(lang, en, zh);
                Hit {
                    title: if title.is_empty() { rank.to_string() } else { title },
                    subtitle: caption(&trail(lang, en, zh), count),
                    key: match nation {
                        Some(_) if legacy_tier => format!("band:{rank}"),
                        Some(nation) => format!("land-rank:{nation}:{rank}"),
                        None => format!("rank:{rank}"),
                    },
                    icon: self.representative_hero_icon(Some(rank), nation),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn hero_land_hits(&self, lang: &str) -> Vec<Hit> {
        let mut counts: HashMap<i64, i64> = HashMap::new();
        for hero in &self.heroes {
            *counts.entry(hero.nation).or_default() += 1;
        }
        let mut masks: Vec<i64> = counts.keys().copied().collect();
        masks.sort_by_key(|mask| hero_land_order(*mask));
        masks
            .into_iter()
            .map(|mask| {
                let (en, zh) = hero_land_pair(mask);
                let count = format!("{} {}", commas(counts[&mask]), phrase("heroes", lang));
                Hit {
                    title: lead(lang, en, zh),
                    subtitle: caption(&trail(lang, en, zh), count),
                    key: format!("land:{mask}"),
                    icon: self.representative_hero_icon(None, Some(mask)),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn hero_tier_hits(&self, nation: &str, lang: &str) -> Vec<Hit> {
        let Ok(nation) = nation.parse::<i64>() else { return Vec::new() };
        self.hero_rank_groups(Some(nation), lang, true)
    }

    pub(in crate::db) fn hero_hits(&self, query: &str, folded: &str, lang: &str, rank: &str, nation: &str, limit: usize) -> Vec<Hit> {
        if limit == 0 {
            return Vec::new();
        }
        let wanted = rank.parse::<i64>().ok();
        let land = nation.parse::<i64>().ok();
        let listed = |hero: &HeroRow| {
            wanted.is_none_or(|rank| hero.rank == rank) && land.is_none_or(|nation| hero.nation == nation)
        };
        let region_order = land.is_some() && wanted.is_none();
        let mut candidates: Vec<_> = self.heroes.iter().filter(|hero| listed(hero)).collect();
        // Apply directory order before the limit, so a short regional result
        // starts with the same ranks as the full regional directory.
        if region_order {
            candidates.sort_by(|a, b| hero_region_order(a, b));
        }
        let mut picked = Vec::new();
        for hero in candidates {
            if !self.name_hit(query, folded, &hero.code, &hero.names) {
                continue;
            }
            picked.push(hero);
            if picked.len() == limit {
                break;
            }
        }
        if picked.len() < limit {
            picked.extend(fuzzy_rows(&self.heroes, folded, limit - picked.len(), |hero| {
                listed(hero) && !self.name_hit(query, folded, &hero.code, &hero.names)
            }, |hero| &hero.names));
        }
        if region_order {
            picked.sort_by(|a, b| hero_region_order(a, b));
        } else if picked.iter().all(|hero| hero.rank == 13) || picked.iter().all(|hero| hero.rank == 14) {
            picked.sort_by(|a, b| hero_order(a, b));
        }
        picked
            .into_iter()
            .map(|hero| {
                let (title, alt) = hero_names(lang, hero);
                let (en, zh) = hero_rank_pair(hero.rank);
                let rank_name = if wanted.is_some() { String::new() } else { lead(lang, en, zh) };
                let companion = matches!(hero.rank, 13 | 14);
                let aside = if companion {
                    hero.companion.as_ref().map(|row| companion_summary(row, hero.rank, lang)).unwrap_or_default()
                } else {
                    lead(lang, &hero.nation_en, &hero.nation_zh)
                };
                let chip = match hero.rank {
                    13 => "mount".to_string(),
                    14 => format!("spirit:{}", hero.companion.as_ref().and_then(|row| row.family_element).map(|value| value.to_string()).unwrap_or_else(|| "unknown".into())),
                    _ => String::new(),
                };
                Hit {
                    title,
                    subtitle: if companion { alt } else { caption(&alt, rank_name) },
                    key: format!("hero:{}", hero.code),
                    icon: hero.icon.clone(),
                    solo: false,
                    aside,
                    badge: None,
                    chip,
                }
            })
            .collect()
    }

    pub(in crate::db) fn hero_chain(&self, code: &str, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        if let Some(steps) = self.chains.get(code) {
            if steps.len() >= 2 {
                for step in steps {
                    let Some(hero) = self.heroes.iter().find(|hero| hero.code == step.code) else { continue };
                    let (title, alt) = hero_names(lang, hero);
                    let mut note = scrub(&tidy(&lead(lang, &step.en, &step.zh)));
                    note = drop_leading_name(&note, &hero.zh);
                    note = drop_leading_name(&note, &hero.en);
                    note = drop_leading_name(&note, &hero.ko);
                    if note.chars().count() < 2 {
                        note.clear();
                    }
                    hits.push(Hit {
                        title,
                        subtitle: caption(&alt, note),
                        key: format!("hero:{}", step.code),
                        icon: hero.icon.clone(),
                        solo: false,
                        aside: chain_role(step.slot, lang),
                        badge: None,
                        chip: String::new(),
                    });
                }
            }
        }
        let mut blocks = Vec::new();
        self.push_fixed_beast(&mut blocks, code, lang);
        if let Some(steps) = self.chains.get(code) {
            for (index, step) in steps.iter().enumerate() {
                let Some(next) = steps.get(index + 1) else { continue };
                if next.code != code {
                    continue;
                }
                let source = self.heroes.iter().find(|hero| hero.code == step.code);
                let mut heading = scrub(&tidy(&lead(lang, &step.en, &step.zh)));
                if let Some(hero) = source {
                    heading = drop_leading_name(&heading, &hero.zh);
                    heading = drop_leading_name(&heading, &hero.en);
                    heading = drop_leading_name(&heading, &hero.ko);
                }
                if step.materials.is_empty() && heading.chars().count() < 2 {
                    continue;
                }
                if heading.chars().count() < 2 {
                    heading = phrase("upgrade", lang);
                }
                let from = source.map(|hero| hero_names(lang, hero).0).unwrap_or_default();
                if step.materials.is_empty() {
                    blocks.push(Hit {
                        title: heading,
                        subtitle: from,
                        key: String::new(),
                        icon: None,
                        solo: false,
                        aside: String::new(),
                        badge: None,
                        chip: "label".into(),
                    });
                } else {
                    self.push_material_block(&mut blocks, lang, heading, from, &step.materials);
                }
            }
        }
        if blocks.is_empty() {
            if let Some(hero) = self.heroes.iter().find(|hero| hero.code == code) {
                if matches!(hero.rank, 4 | 5 | 11) {
                    let hire = scrub(&tidy(&lead(lang, &hero.hire_en, &hero.hire_zh)));
                    if hire.chars().count() >= 2 {
                        blocks.push(Hit {
                            title: hire,
                            subtitle: String::new(),
                            key: String::new(),
                            icon: None,
                            solo: false,
                            aside: String::new(),
                            badge: None,
                            chip: "label".into(),
                        });
                    }
                }
            }
        }
        if let Some(recipes) = self.combos.get(code) {
            let mut routed = HashSet::new();
            for recipe in recipes.iter().filter(|recipe| recipe.result == code) {
                let result = self.heroes.iter().find(|hero| hero.code == recipe.result);
                let result_name = result.map(|hero| hero_names(lang, hero).0).unwrap_or_else(|| recipe.result.clone());
                let note = credit_note(&lead(lang, &recipe.en, &recipe.zh));
                blocks.push(Hit {
                    title: format!("{} · {result_name}", phrase("combine", lang)),
                    subtitle: note,
                    key: format!("hero:{}", recipe.result),
                    icon: result.and_then(|hero| hero.icon.clone()),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: "label".into(),
                });
                for part in &recipe.parts {
                    let Some(hero) = self.heroes.iter().find(|hero| hero.code == *part) else { continue };
                    let (title, alt) = hero_names(lang, hero);
                    blocks.push(Hit {
                        title,
                        subtitle: alt,
                        key: format!("hero:{part}"),
                        icon: hero.icon.clone(),
                        solo: false,
                        aside: phrase("combine", lang),
                        badge: None,
                        chip: String::new(),
                    });
                }
                for (item_code, qty) in &recipe.materials {
                    let item = self.find_item(*item_code);
                    let (name, alt) = item
                        .map(|item| (lead(lang, &item.en, &item.zh), trail(lang, &item.en, &item.zh)))
                        .unwrap_or_default();
                    blocks.push(Hit {
                        title: if name.is_empty() { item_code.to_string() } else { name },
                        subtitle: alt,
                        key: format!("item:{item_code}"),
                        icon: self.item_icons.get(item_code).cloned(),
                        solo: false,
                        aside: format!("× {}", commas(*qty)),
                        badge: None,
                        chip: String::new(),
                    });
                }
            }
            let mut routes = Vec::new();
            for recipe in recipes.iter().filter(|recipe| recipe.result != code) {
                if !routed.insert(recipe.result.clone()) {
                    continue;
                }
                let result = self.heroes.iter().find(|hero| hero.code == recipe.result);
                let result_name = result.map(|hero| hero_names(lang, hero).0).unwrap_or_else(|| recipe.result.clone());
                routes.push(Hit {
                    title: result_name,
                    subtitle: phrase("combine", lang),
                    key: format!("hero:{}", recipe.result),
                    icon: result.and_then(|hero| hero.icon.clone()),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                });
            }
            if !blocks.is_empty() {
                hits.push(hero_section("need", lang));
                hits.extend(blocks);
            }
            if !routes.is_empty() {
                hits.push(hero_section("related", lang));
                hits.extend(routes);
            }
        } else if !blocks.is_empty() {
            hits.push(hero_section("need", lang));
            hits.extend(blocks);
        }
        hits
    }

    pub(in crate::db) fn push_material_block(&self, hits: &mut Vec<Hit>, lang: &str, title: String, subtitle: String, materials: &[(i64, i64)]) {
        hits.push(Hit {
            title,
            subtitle,
            key: String::new(),
            icon: None,
            solo: false,
            aside: String::new(),
            badge: None,
            chip: "label".into(),
        });
        for (code, qty) in materials {
            let item = self.find_item(*code);
            let (name, alt) = item
                .map(|item| (lead(lang, &item.en, &item.zh), trail(lang, &item.en, &item.zh)))
                .unwrap_or_default();
            hits.push(Hit {
                title: if name.is_empty() { code.to_string() } else { name },
                subtitle: alt,
                key: format!("item:{code}"),
                icon: self.item_icons.get(code).cloned(),
                solo: false,
                aside: format!("× {}", commas(*qty)),
                badge: None,
                chip: String::new(),
            });
        }
    }

    pub(in crate::db) fn roster_hits(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        if let Some(uses) = self.roster_uses.get(&code) {
            hits.extend(uses.iter().map(|use_| {
                let target = self.heroes.iter().find(|hero| hero.code == use_.target);
                let source = self.heroes.iter().find(|hero| hero.code == use_.source);
                let target_name = target.map(|hero| hero_names(lang, hero).0).unwrap_or_else(|| use_.target.clone());
                let source_name = source.map(|hero| hero_names(lang, hero).0).unwrap_or_default();
                let kind = roster_kind(&use_.note, target.map(|hero| hero.zh.as_str()).unwrap_or(""), use_.combo, lang);
                let subtitle = if source_name.is_empty() || source_name == target_name {
                    kind
                } else {
                    format!("{kind} · {source_name}")
                };
                Hit {
                    title: target_name,
                    subtitle,
                    key: format!("hero:{}", use_.target),
                    icon: target.and_then(|hero| hero.icon.clone()),
                    solo: false,
                    aside: format!("× {}", commas(use_.qty)),
                    badge: None,
                    chip: String::new(),
                }
            }));
        }
        for (hero_code, qty) in fixed_beast_items(code) {
            let target = self.heroes.iter().find(|hero| hero.code == hero_code);
            let target_name = target.map(|hero| hero_names(lang, hero).0).unwrap_or_else(|| hero_code.to_string());
            hits.push(Hit {
                title: target_name,
                subtitle: phrase("combine", lang),
                key: format!("hero:{hero_code}"),
                icon: target.and_then(|hero| hero.icon.clone()),
                solo: false,
                aside: if qty == 0 { phrase("pick-one", lang) } else { format!("× {}", commas(qty)) },
                badge: None,
                chip: String::new(),
            });
        }
        hits
    }

    pub(in crate::db) fn push_fixed_beast(&self, blocks: &mut Vec<Hit>, code: &str, lang: &str) {
        let Some(parts) = fixed_beast(code) else { return };
        let hero = self.heroes.iter().find(|hero| hero.code == code);
        let name = hero.map(|hero| hero_names(lang, hero).0).unwrap_or_else(|| code.to_string());
        let credit = fixed_credit(code);
        blocks.push(Hit {
            title: format!("{} · {name}", phrase("combine", lang)),
            subtitle: if credit > 0 { format!("{} {credit}", phrase("credit-level", lang)) } else { String::new() },
            key: format!("hero:{code}"),
            icon: hero.and_then(|hero| hero.icon.clone()),
            solo: false,
            aside: String::new(),
            badge: None,
            chip: "label".into(),
        });
        for part in parts {
            if part.item == 0 {
                blocks.push(Hit {
                    title: lead(lang, part.en, part.zh).to_string(),
                    subtitle: lead(lang, part.hint_en, part.hint_zh).to_string(),
                    key: String::new(),
                    icon: None,
                    solo: false,
                    aside: format!("× {}", commas(part.qty)),
                    badge: None,
                    chip: String::new(),
                });
                continue;
            }
            let item = self.find_item(part.item);
            let (title, alt) = item
                .map(|item| (lead(lang, &item.en, &item.zh), trail(lang, &item.en, &item.zh)))
                .unwrap_or_else(|| (part.item.to_string(), String::new()));
            let alt = if part.hint_zh.is_empty() { alt } else { lead(lang, part.hint_en, part.hint_zh).to_string() };
            blocks.push(Hit {
                title: if title.is_empty() { part.item.to_string() } else { title },
                subtitle: alt,
                key: format!("item:{}", part.item),
                icon: self.item_icons.get(&part.item).cloned(),
                solo: false,
                aside: if part.qty == 0 { phrase("pick-one", lang) } else { format!("× {}", commas(part.qty)) },
                badge: None,
                chip: String::new(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regional_rank_directory_scopes_counts_portraits_and_record_keys() {
        let catalog = Catalog::open().expect("catalog");
        let groups = catalog.hero_region_ranks("1", "zh");
        let ranks: Vec<_> = groups.iter().map(|hit| hit.key.strip_prefix("land-rank:1:").unwrap().parse::<i64>().unwrap()).collect();
        assert_eq!(ranks, [0, 1, 2, 3, 10, 11, 12, 6, 7, 5, 4]);
        let english = catalog.hero_region_ranks("1", "en");
        let mut total = 0;
        for (group, rank) in groups.iter().zip(ranks) {
            let expected: Vec<_> = catalog.index.heroes.iter().filter(|hero| hero.nation == 1 && hero.rank == rank).collect();
            let records = catalog.hero_region_records("1", &rank.to_string(), "zh");
            assert_eq!(records.len(), expected.len());
            assert!(records.iter().all(|hit| expected.iter().any(|hero| hit.key == format!("hero:{}", hero.code))));
            assert!(group.subtitle.contains(&format!("{} {}", commas(expected.len() as i64), phrase("heroes", "zh"))));
            assert_eq!(group.title, hero_rank_pair(rank).1);
            assert!(group.icon.is_some());
            assert!(expected.iter().any(|hero| hero.icon == group.icon));
            assert_eq!(group.icon, english.iter().find(|hit| hit.key == group.key).unwrap().icon);
            total += records.len();
        }
        assert_eq!(total, catalog.index.heroes.iter().filter(|hero| hero.nation == 1).count());
        let mercenaries = catalog.hero_region_records("1", "0", "zh");
        assert!(mercenaries.iter().any(|hit| hit.key == "hero:U08" && hit.title == "儒生"));
        assert!(catalog.index.hero_tier_hits("1", "zh").iter().all(|hit| hit.key.starts_with("band:")));
        assert!(catalog.hero_region_ranks("invalid", "zh").is_empty());
        assert!(catalog.hero_region_ranks("999", "zh").is_empty());
        assert!(catalog.hero_region_records("invalid", "0", "zh").is_empty());
        assert!(catalog.hero_region_records("1", "invalid", "zh").is_empty());
    }

    #[test]
    fn mixed_regional_records_follow_rank_tabs_before_limiting() {
        let catalog = Catalog::open().expect("catalog");
        let all = catalog.index.hero_hits("", "", "zh", "", "1", usize::MAX);
        let rank = |hit: &Hit| catalog.index.heroes.iter().find(|hero| hit.key == format!("hero:{}", hero.code)).unwrap().rank;
        let mut ranks: Vec<_> = all.iter().map(rank).collect();
        ranks.dedup();
        assert_eq!(ranks, [0, 1, 2, 3, 10, 11, 12, 6, 7, 5, 4]);
        let limited = catalog.index.hero_hits("", "", "zh", "", "1", 5);
        assert_eq!(limited.iter().map(|hit| &hit.key).collect::<Vec<_>>(), all.iter().take(5).map(|hit| &hit.key).collect::<Vec<_>>());
        assert!(limited.iter().all(|hit| rank(hit) == 0));
        assert!(catalog.index.hero_hits("", "", "zh", "", "1", 0).is_empty());
        let english = catalog.index.hero_hits("", "", "en", "", "1", usize::MAX);
        assert_eq!(all.iter().map(|hit| &hit.key).collect::<Vec<_>>(), english.iter().map(|hit| &hit.key).collect::<Vec<_>>());

        // The regional directory retains riding-level and element/grade order.
        let other = catalog.index.hero_hits("", "", "zh", "", "0", usize::MAX);
        for companion_rank in ["13", "14"] {
            let regional = catalog.hero_region_records("0", companion_rank, "zh");
            let global = catalog.index.hero_hits("", "", "zh", companion_rank, "", usize::MAX);
            let filtered: Vec<_> = other.iter().filter(|hit| rank(hit).to_string() == companion_rank).map(|hit| &hit.key).collect();
            assert_eq!(regional.iter().map(|hit| &hit.key).collect::<Vec<_>>(), global.iter().map(|hit| &hit.key).collect::<Vec<_>>());
            assert_eq!(filtered, global.iter().map(|hit| &hit.key).collect::<Vec<_>>());
        }
    }

    #[test]
    fn empty_skill_slots_do_not_become_zero_facts() {
        let catalog = Catalog::open().expect("catalog");
        for lang in ["zh-en", "en-zh"] {
            let monk = catalog.hero_sheet("U68", lang).expect("monk");
            assert!(monk.3.iter().all(|(label, value)| label != "0" && value != "0"));
            assert!(monk.3.iter().any(|(label, _)| label == &phrase("hire", lang)));

            let general = catalog.hero_sheet("U87", lang).expect("general with skill");
            assert!(general.3.iter().any(|(label, value)| label == "폭뢰격" && !value.is_empty()));
        }
        // The sentinel is specific to unused skill slots, not numeric character stats.
        assert!(catalog.hero_stat_lines("U00", "en").iter().any(|(label, value)| label == "MP" && value == "0"));
    }

    #[test]
    fn mount_lists_show_riding_and_speed_in_required_level_order() {
        let catalog = Catalog::open().expect("catalog");
        assert_eq!(hero_rank_pair(13), ("Mounts", "坐騎"));
        let mounts = catalog.index.hero_hits("", "", "zh", "13", "", usize::MAX);
        assert_eq!(mounts.len(), 15);
        let levels: Vec<_> = mounts.iter().map(|hit| {
            assert_eq!(hit.chip, "mount");
            assert!(hit.aside.contains("馬術 ") && hit.aside.contains("主角移速 +"));
            assert!(!hit.aside.contains("地圖"));
            assert!(!hit.subtitle.contains("馬術"));
            catalog.index.heroes.iter().find(|hero| hit.key == format!("hero:{}", hero.code))
                .unwrap().companion.as_ref().unwrap().riding_level.unwrap()
        }).collect();
        assert!(levels.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(levels.iter().filter(|level| **level == 1).count(), 4);
    }

    #[test]
    fn spirit_lists_group_actual_family_and_order_each_evolution_grade() {
        let catalog = Catalog::open().expect("catalog");
        let spirits = catalog.index.hero_hits("", "", "zh", "14", "", usize::MAX);
        assert_eq!(spirits.len(), 33);
        let water: Vec<_> = spirits.iter().filter(|hit| hit.chip == "spirit:2").map(|hit| hit.key.as_str()).collect();
        assert_eq!(water, ["hero:F16", "hero:F19", "hero:F22", "hero:H29", "hero:V03"]);
        assert!(spirits.iter().find(|hit| hit.key == "hero:F22").unwrap().aside.starts_with("水 · "));
        let mut previous = None;
        for hit in &spirits {
            let hero = catalog.index.heroes.iter().find(|hero| hit.key == format!("hero:{}", hero.code)).unwrap();
            let data = hero.companion.as_ref().unwrap();
            let element = data.family_element.unwrap();
            let key = (if element == 0 { 6 } else { element }, data.spirit_grade.unwrap());
            if let Some(previous) = previous { assert!(previous <= key); }
            previous = Some(key);
        }
        let english = catalog.index.hero_hits("", "", "en", "14", "", usize::MAX);
        assert_eq!(spirits.iter().map(|hit| &hit.key).collect::<Vec<_>>(), english.iter().map(|hit| &hit.key).collect::<Vec<_>>());
    }

    #[test]
    fn spirit_effects_are_prioritized_without_duplicate_detail_facts() {
        let catalog = Catalog::open().expect("catalog");
        assert!(catalog.hero_is_spirit("F22"));
        assert!(!catalog.hero_is_spirit("U68"));
        let effects = catalog.hero_effect_lines("H30", "en");
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].0, "Skill effect");
        assert!(effects[0].1.contains("ATK +500"));
        let sheet = catalog.hero_sheet("H30", "en").unwrap();
        assert!(sheet.3.iter().all(|(label, _)| label != "Skill effect"));
        let text = catalog.open_hero("H30", "en").0;
        assert!(text.find("Skill effect").unwrap() < text.find("STR  ").unwrap());
        assert_eq!(text.matches("ATK +500").count(), 1);
    }
}
