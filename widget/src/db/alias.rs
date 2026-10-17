use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::paths::data_dir;

pub(in crate::db) const SET_NICKS: &[(&str, &str, &str)] = &[
    ("龜天", "多聞天王", "다문천왕"),
    ("雀天", "持國天王", "지국천왕"),
    ("龍天", "增長天王", "증장천왕"),
    ("虎天", "廣目天王", "광목천왕"),
];

pub(in crate::db) const SLOT_NICKS: &[(&str, &str)] = &[
    ("手", "G"),
    ("腰", "B"),
    ("鞋", "S"),
    ("頭", "H"),
    ("甲", "A"),
    ("戒", "R"),
];

pub(in crate::db) fn set_query(query: &str) -> Option<(&'static str, &'static str, Option<&'static str>)> {
    for &(nick, zh, ko) in SET_NICKS {
        let Some(rest) = query.strip_prefix(nick) else { continue };
        if rest.is_empty() {
            return Some((zh, ko, None));
        }
        if let Some(&(_, category)) = SLOT_NICKS.iter().find(|(slot, _)| *slot == rest) {
            return Some((zh, ko, Some(category)));
        }
        return None;
    }
    None
}

pub(in crate::db) const DEFAULT_ALIAS_LINE: &str = "SSA 6213 6214 6215 6216 6943";

pub fn alias_editor_text() -> String {
    alias_editor_text_in(&data_dir())
}

pub(in crate::db) fn alias_editor_text_in(dir: &Path) -> String {
    let path = dir.join("aliases.txt");
    if !path.is_file() {
        let _ = std::fs::write(&path, format!("# 一行一個簡稱，後面寫道具編號。\n{DEFAULT_ALIAS_LINE}\n"));
        return DEFAULT_ALIAS_LINE.to_string();
    }
    alias_entry_text(&std::fs::read_to_string(&path).unwrap_or_default())
}

pub fn alias_rows() -> Vec<(String, String)> {
    alias_rows_from(&alias_editor_text())
}

pub(in crate::db) fn alias_rows_from(text: &str) -> Vec<(String, String)> {
    let mut order = Vec::new();
    let mut shown: HashMap<String, String> = HashMap::new();
    let mut codes: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let tokens = alias_tokens(line);
        if tokens.len() < 2 {
            continue;
        }
        let rest_are_codes = tokens[1..].iter().all(|token| is_code_token(token));
        if !is_code_token(&tokens[0]) && rest_are_codes {
            remember_alias(&mut order, &mut shown, &mut codes, &tokens[0], &tokens[1..]);
        } else {
            let code = code_key(&tokens[0]);
            for alias in &tokens[1..] {
                remember_alias(&mut order, &mut shown, &mut codes, alias, &[code.clone()]);
            }
        }
    }
    order
        .into_iter()
        .map(|key| (shown.remove(&key).unwrap_or(key.clone()), codes.remove(&key).unwrap_or_default().join(" ")))
        .collect()
}

pub(in crate::db) fn remember_alias(
    order: &mut Vec<String>,
    shown: &mut HashMap<String, String>,
    codes: &mut HashMap<String, Vec<String>>,
    alias: &str,
    incoming: &[String],
) {
    let display = alias.trim().trim_start_matches('#').to_string();
    let key = display.to_lowercase();
    if key.chars().count() < 2 {
        return;
    }
    if !shown.contains_key(&key) {
        order.push(key.clone());
        shown.insert(key.clone(), display);
    }
    let list = codes.entry(key).or_default();
    for code in incoming {
        let code = code_key(code);
        if !code.is_empty() && !list.contains(&code) {
            list.push(code);
        }
    }
}

pub(in crate::db) fn alias_tokens(line: &str) -> Vec<String> {
    line.split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

pub(in crate::db) fn alias_entry_text(raw: &str) -> String {
    raw.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn save_alias_text(text: &str) {
    let body = text.replace('\r', "").trim().to_string();
    let _ = std::fs::write(
        data_dir().join("aliases.txt"),
        format!("# 一行一個簡稱，後面寫道具編號。\n{body}\n"),
    );
}

pub(in crate::db) fn parse_alias_book(text: &str) -> HashMap<String, HashSet<String>> {
    let mut book = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let tokens: Vec<String> = line
            .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_string)
            .collect();
        if tokens.len() < 2 {
            continue;
        }
        let rest_are_codes = tokens[1..].iter().all(|token| is_code_token(token));
        if !is_code_token(&tokens[0]) && rest_are_codes {
            let alias = tokens[0].trim_start_matches('#').to_string();
            for code in &tokens[1..] {
                add_alias(&mut book, &code_key(code), &alias);
            }
        } else {
            let code = code_key(&tokens[0]);
            for alias in &tokens[1..] {
                add_alias(&mut book, &code, alias);
            }
        }
    }
    book
}

pub(in crate::db) fn is_code_token(token: &str) -> bool {
    let code = code_key(token);
    !code.is_empty() && code.chars().all(|ch| ch.is_ascii_digit())
}

pub(in crate::db) fn code_key(token: &str) -> String {
    let token = token.trim().trim_start_matches('#');
    token.split_once(':').map(|(_, code)| code.trim()).unwrap_or(token).to_string()
}

pub(in crate::db) fn add_alias(book: &mut HashMap<String, HashSet<String>>, code: &str, alias: &str) {
    let alias = alias.trim().trim_start_matches('#').to_lowercase();
    if code.is_empty() || alias.chars().count() < 2 {
        return;
    }
    book.entry(alias).or_default().insert(code.to_string());
}

pub(in crate::db) fn alias_codes(book: &HashMap<String, HashSet<String>>, folded: &str) -> HashSet<String> {
    if folded.chars().count() < 2 {
        return HashSet::new();
    }
    let mut codes = HashSet::new();
    if let Some(found) = book.get(folded) {
        codes.extend(found.iter().cloned());
    }
    if folded.chars().count() >= 3 {
        for (alias, found) in book {
            if alias != folded && alias.starts_with(folded) {
                codes.extend(found.iter().cloned());
            }
        }
    }
    codes
}
