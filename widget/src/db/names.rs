use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

use super::rows::ItemRow;

pub(in crate::db) fn plain_name(name: &str) -> String {
    let trimmed = name.trim();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return trimmed.to_string();
    };
    rest.split_once(']').map(|(_, tail)| tail.trim().to_string()).unwrap_or_else(|| trimmed.to_string())
}

pub(in crate::db) fn plain_piece(item: &ItemRow) -> bool {
    const MARKS: &[&str] = &[
        "(", "（", "[", "日", "外觀", "覺醒", "活動", "回歸", "測試", "製造", "交易", "-", " ",
    ];
    matches!(item.category.as_str(), "G" | "B" | "S" | "H" | "A" | "R")
        && !item.zh.is_empty()
        && MARKS.iter().all(|mark| !item.zh.contains(mark))
}

pub(in crate::db) fn name_match(query: &str, folded: &str, exact: &str, names: &str) -> bool {
    query.is_empty() || exact.eq_ignore_ascii_case(query) || (!folded.is_empty() && names.contains(folded))
}

pub(in crate::db) fn text_hit(folded: &str, names: &str) -> bool {
    if folded.is_empty() {
        return false;
    }
    if names.contains(folded) {
        return true;
    }
    let budget = fuzzy_budget(folded);
    budget > 0 && fuzzy_dist(folded, names, budget).is_some()
}

pub(in crate::db) fn haystack(parts: &[&str]) -> String {
    let mut out = String::new();
    for part in parts {
        let folded = part.trim().to_lowercase();
        if folded.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&folded);
    }
    for part in parts {
        for alias in nicknames(part) {
            let alias = alias.to_lowercase();
            if alias.chars().count() < 2 || out.split('\n').any(|line| line == alias) {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&alias);
        }
    }
    out
}

pub(in crate::db) fn nicknames(name: &str) -> Vec<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 48 || name.contains(['[', ']', '【', '】']) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(alias) = latin_acronym(name) {
        out.push(alias);
    }
    if let Some(alias) = paren_alias(name) {
        if !out.iter().any(|have| have == &alias) {
            out.push(alias);
        }
    }
    if let Some(alias) = cjk_initials(name) {
        if !out.iter().any(|have| have == &alias) {
            out.push(alias);
        }
    }
    out
}

pub(in crate::db) fn latin_acronym(name: &str) -> Option<String> {
    let words: Vec<&str> = name
        .split(|ch: char| !ch.is_ascii_alphabetic())
        .filter(|word| word.len() >= 2)
        .collect();
    if !(3..=5).contains(&words.len()) {
        return None;
    }
    Some(words.iter().filter_map(|word| word.chars().next()).map(|ch| ch.to_ascii_lowercase()).collect())
}

pub(in crate::db) fn paren_alias(name: &str) -> Option<String> {
    let (body, inner) = paren_parts(name)?;
    if !short_cjk(inner) {
        return None;
    }
    let head = body.chars().find(|ch| is_cjk(*ch))?;
    Some(format!("{head}{inner}"))
}

pub(in crate::db) fn cjk_initials(name: &str) -> Option<String> {
    if name.chars().any(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let body = match paren_parts(name) {
        Some((body, inner)) if short_cjk(inner) => body,
        _ => name,
    };
    let mut parts = Vec::new();
    let mut word = String::new();
    for ch in body.chars() {
        if matches!(ch, '之' | '的' | '與' | '和' | '及') {
            if !word.is_empty() {
                parts.push(std::mem::take(&mut word));
            }
        } else if is_cjk(ch) {
            word.push(ch);
        } else if !word.is_empty() {
            parts.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        parts.push(word);
    }
    if !(2..=4).contains(&parts.len()) || parts.iter().any(|part| part.chars().count() > 6) {
        return None;
    }
    let alias: String = parts.iter().filter_map(|part| part.chars().next()).collect();
    if (2..=4).contains(&alias.chars().count()) {
        Some(alias)
    } else {
        None
    }
}

pub(in crate::db) fn paren_parts(name: &str) -> Option<(&str, &str)> {
    let chars: Vec<(usize, char)> = name.char_indices().collect();
    let mut open = None;
    let mut close = None;
    for (index, (_, ch)) in chars.iter().enumerate() {
        if *ch == '(' || *ch == '（' {
            open = Some(index);
        } else if (*ch == ')' || *ch == '）') && open.is_some() {
            close = Some(index);
        }
    }
    let open = open?;
    let close = close.filter(|close| *close > open)?;
    let body = name[..chars[open].0].trim();
    let inner_start = chars[open].0 + chars[open].1.len_utf8();
    let inner = name[inner_start..chars[close].0].trim();
    Some((body, inner))
}

pub(in crate::db) fn short_cjk(text: &str) -> bool {
    let count = text.chars().count();
    (1..=2).contains(&count) && text.chars().all(is_cjk)
}

pub(in crate::db) fn is_cjk(ch: char) -> bool {
    matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}')
}

pub(in crate::db) fn fuzzy_budget(query: &str) -> u8 {
    match query.chars().count() {
        0..=2 => 0,
        3..=8 => 1,
        9..=24 => 2,
        _ => 0,
    }
}

pub(in crate::db) fn fuzzy_rows<'a, T>(
    rows: &'a [T],
    folded: &str,
    room: usize,
    take: impl Fn(&T) -> bool,
    names: impl Fn(&T) -> &str,
) -> Vec<&'a T> {
    let budget = fuzzy_budget(folded);
    if budget == 0 || room == 0 {
        return Vec::new();
    }
    let query: Vec<char> = folded.chars().collect();
    let mut found = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if !take(row) {
            continue;
        }
        let names = names(row);
        if !query.iter().any(|ch| names.contains(*ch)) {
            continue;
        }
        let Some(dist) = fuzzy_dist_chars(&query, names, budget) else {
            continue;
        };
        found.push((dist, index, row));
    }
    found.sort_by_key(|(dist, index, _)| (*dist, *index));
    found.into_iter().take(room).map(|(_, _, row)| row).collect()
}

pub(in crate::db) fn fuzzy_dist(query: &str, names: &str, budget: u8) -> Option<u8> {
    if budget == 0 {
        return None;
    }
    let chars: Vec<char> = query.chars().collect();
    fuzzy_dist_chars(&chars, names, budget)
}

pub(in crate::db) fn fuzzy_dist_chars(query: &[char], names: &str, budget: u8) -> Option<u8> {
    let mut best: Option<u8> = None;
    for line in names.split('\n') {
        if line.is_empty() {
            continue;
        }
        let limit = best.map(|have| have.saturating_sub(1)).unwrap_or(budget);
        relax(&mut best, bounded_edit(query, line, limit));
        if best == Some(1) {
            return best;
        }
        if !line.is_ascii() {
            continue;
        }
        for word in line.split(|ch: char| !ch.is_ascii_alphabetic()) {
            if word.len() < 3 {
                continue;
            }
            let limit = best.map(|have| have.saturating_sub(1)).unwrap_or(budget);
            if limit == 0 {
                return best;
            }
            relax(&mut best, bounded_edit(query, word, limit));
            if best == Some(1) {
                return best;
            }
        }
    }
    best
}

pub(in crate::db) fn relax(best: &mut Option<u8>, next: Option<u8>) {
    if let Some(dist) = next {
        if best.map(|have| dist < have).unwrap_or(true) {
            *best = Some(dist);
        }
    }
}

pub(in crate::db) fn bounded_edit(query: &[char], text: &str, budget: u8) -> Option<u8> {
    if budget == 0 || query.len() > 48 {
        return None;
    }
    let width = query.len();
    let max = budget as usize;
    let mut prev = [0u8; 49];
    for index in 0..=width {
        prev[index] = index as u8;
    }
    let mut length = 0usize;
    for ch in text.chars() {
        length += 1;
        if length > width + max {
            return None;
        }
        let mut curr = [0u8; 49];
        curr[0] = length as u8;
        let mut row_min = curr[0];
        for index in 1..=width {
            let cost = if query[index - 1] == ch { 0 } else { 1 };
            let value = (prev[index] + 1).min(curr[index - 1] + 1).min(prev[index - 1] + cost);
            curr[index] = value;
            row_min = row_min.min(value);
        }
        if row_min > budget {
            return None;
        }
        prev = curr;
    }
    if (width as i32 - length as i32).unsigned_abs() > max as u32 {
        return None;
    }
    let dist = prev[width];
    if dist == 0 || dist > budget {
        None
    } else {
        Some(dist)
    }
}

pub(in crate::db) struct FoundMats {
    pub(in crate::db) items: Vec<(i64, i64)>,
}

pub(in crate::db) struct NameNode {
    pub(in crate::db) next: HashMap<char, usize>,
    pub(in crate::db) owners: Vec<(i64, bool)>,
}

pub(in crate::db) struct NameTrie {
    pub(in crate::db) nodes: Vec<NameNode>,
}

impl NameTrie {
    pub(in crate::db) fn from_items(items: &[ItemRow]) -> Self {
        let mut trie = Self { nodes: vec![NameNode { next: HashMap::new(), owners: Vec::new() }] };
        for item in items {
            let Ok(code) = item.code_str.parse::<i64>() else { continue };
            for raw in [&item.zh, &item.en] {
                let raw = raw.trim();
                if raw.chars().count() < 2 {
                    continue;
                }
                let full = fold_item_name(raw, false);
                let loose = fold_item_name(raw, true);
                if full.chars().count() >= 2 {
                    trie.insert(&full, code, true);
                }
                if loose.chars().count() >= 2 && loose != full {
                    trie.insert(&loose, code, false);
                }
            }
        }
        trie
    }

    pub(in crate::db) fn insert(&mut self, key: &str, code: i64, full: bool) {
        let mut node = 0;
        for ch in key.chars() {
            if let Some(next) = self.nodes[node].next.get(&ch).copied() {
                node = next;
                continue;
            }
            let id = self.nodes.len();
            self.nodes.push(NameNode { next: HashMap::new(), owners: Vec::new() });
            self.nodes[node].next.insert(ch, id);
            node = id;
        }
        if !self.nodes[node].owners.iter().any(|(have, kind)| *have == code && *kind == full) {
            self.nodes[node].owners.push((code, full));
        }
    }

    pub(in crate::db) fn find(&self, text: &str) -> FoundMats {
        let (folded, origin) = fold_note(text);
        let chars: Vec<char> = folded.chars().collect();
        let mut found = Vec::new();
        for start in 0..chars.len() {
            let mut node = 0;
            for (offset, ch) in chars[start..].iter().enumerate() {
                let Some(next) = self.nodes[node].next.get(ch).copied() else { break };
                node = next;
                if offset + 1 < 2 {
                    continue;
                }
                let Some(code) = choose_name(&self.nodes[node].owners) else { continue };
                found.push((start, start + offset + 1, code));
            }
        }
        found.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(a.0.cmp(&b.0)));
        let mut taken = vec![false; chars.len()];
        let mut picked = Vec::new();
        for (start, end, code) in found {
            if taken[start..end].iter().any(|flag| *flag) {
                continue;
            }
            let raw_start = origin[start];
            let raw_end = origin[end - 1] + text[origin[end - 1]..].chars().next().map(|ch| ch.len_utf8()).unwrap_or(0);
            let (qty, span_start, span_end) = if let Some((qty, qty_end)) = qty_after(text, raw_end) {
                (qty, raw_start, qty_end)
            } else if let Some((qty, qty_start)) = qty_before(text, raw_start) {
                (qty, qty_start, raw_end)
            } else {
                continue;
            };
            if qty <= 0 {
                continue;
            }
            taken[start..end].fill(true);
            picked.push((span_start, span_end, code, qty));
        }
        FoundMats {
            items: picked.into_iter().map(|(_, _, code, qty)| (code, qty)).collect(),
        }
    }
}

pub(in crate::db) fn choose_name(owners: &[(i64, bool)]) -> Option<i64> {
    let mut exact: Vec<i64> = owners.iter().filter(|(_, full)| *full).map(|(code, _)| *code).collect();
    exact.sort_unstable();
    exact.dedup();
    if !exact.is_empty() {
        return exact.first().copied();
    }
    let mut loose: Vec<i64> = owners.iter().map(|(code, _)| *code).collect();
    loose.sort_unstable();
    loose.dedup();
    if loose.len() == 1 { loose.first().copied() } else { None }
}

pub(in crate::db) fn qty_after(text: &str, end: usize) -> Option<(i64, usize)> {
    let rest = text.get(end..)?;
    let skipped = rest.len() - rest.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, ')' | '）' | ']' | '］' | '>' | '＞' | '】' | '」' | '』' | 'x' | 'X' | '×')
    }).len();
    let rest = &rest[skipped..];
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let mut used = skipped + digits.len();
    if rest[digits.len()..].starts_with('個') {
        used += '個'.len_utf8();
    }
    Some((digits.parse().ok()?, end + used))
}

pub(in crate::db) fn qty_before(text: &str, start: usize) -> Option<(i64, usize)> {
    let head = text.get(..start)?;
    let chars: Vec<(usize, char)> = head.char_indices().collect();
    let mut index = chars.len();
    while index > 0 && chars[index - 1].1.is_whitespace() {
        index -= 1;
    }
    if index >= 2 && chars[index - 2].1 == '一' && matches!(chars[index - 1].1, '個' | '个') {
        return Some((1, chars[index - 2].0));
    }
    if index == 0 || chars[index - 1].1 != '個' {
        return None;
    }
    index -= 1;
    while index > 0 && chars[index - 1].1.is_whitespace() {
        index -= 1;
    }
    let digit_end = index;
    while index > 0 && chars[index - 1].1.is_ascii_digit() {
        index -= 1;
    }
    if index == digit_end {
        return None;
    }
    let digits: String = chars[index..digit_end].iter().map(|(_, ch)| *ch).collect();
    Some((digits.parse().ok()?, chars[index].0))
}

pub(in crate::db) fn fold_note(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::new();
    let mut origin = Vec::new();
    for (byte, ch) in text.char_indices() {
        for piece in ch.nfkc() {
            let piece = if piece == '凶' { '兇' } else { piece };
            if piece.is_whitespace() || name_punct(piece) {
                continue;
            }
            folded.push(piece.to_ascii_lowercase());
            origin.push(byte);
        }
    }
    (folded, origin)
}

pub(in crate::db) fn fold_item_name(text: &str, drop_paren: bool) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for ch in text.nfkc() {
        let ch = if ch == '凶' { '兇' } else { ch };
        if drop_paren && matches!(ch, '(' | '（') {
            depth += 1;
            continue;
        }
        if drop_paren && matches!(ch, ')' | '）') {
            if depth > 0 {
                depth -= 1;
            }
            continue;
        }
        if drop_paren && depth > 0 {
            continue;
        }
        if ch.is_whitespace() || name_punct(ch) {
            continue;
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

pub(in crate::db) fn name_punct(ch: char) -> bool {
    matches!(
        ch,
        '的' | '之'
            | '<' | '>' | '＜' | '＞'
            | '[' | ']' | '［' | '］'
            | '(' | ')' | '（' | '）'
            | '`' | '\'' | '’' | '"' | '「' | '」' | '『' | '』'
            | '·'
    )
}
