//! Reviewed terminology embedded with the application, separate from mutable user aliases.
//! Entity aliases resolve to typed stable keys; intent vocabulary never proves that
//! a recipe, upgrade path, or drop source exists in the catalog.
use std::collections::HashSet;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

mod load;
pub(crate) use load::load_external;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Evidence {
    pub source: String,
    pub reference: String,
    pub note: String,
    /// Explicitly associates derived or repository spellings with their source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spellings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CanonicalNames {
    pub zh: String,
    pub en: String,
    pub ko: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityTerm {
    pub id: String,
    pub aliases: Vec<String>,
    #[serde(default)]
    pub exact_aliases: Vec<String>,
    /// Common words accepted only alone or as the subject of an explicit question.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subject_aliases: Vec<String>,
    pub keys: Vec<String>,
    pub group: bool,
    pub canonical_names: CanonicalNames,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryIntent {
    Overview,
    Acquisition,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IntentTerm {
    pub id: String,
    pub intent: QueryIntent,
    pub topic: String,
    pub aliases: Vec<String>,
    #[serde(default)]
    pub exact_aliases: Vec<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Glossary {
    pub schema_version: u32,
    pub description: String,
    pub acquisition_note: String,
    pub entities: Vec<EntityTerm>,
    pub intents: Vec<IntentTerm>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntityMatch {
    pub term_id: String,
    pub matched_alias: String,
    pub keys: Vec<String>,
    pub group: bool,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IntentMatch {
    pub matched_alias: String,
    pub intent: QueryIntent,
    pub topic: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TermResolution {
    pub query: String,
    pub intent: QueryIntent,
    pub entities: Vec<EntityMatch>,
    pub intent_matches: Vec<IntentMatch>,
    pub unresolved_modifiers: Vec<String>,
    pub notes: Vec<String>,
}

pub fn glossary() -> &'static Glossary {
    static GLOSSARY: OnceLock<Glossary> = OnceLock::new();
    GLOSSARY.get_or_init(|| {
        serde_json::from_str(include_str!("../../../data/terminology.json"))
            .expect("bundled terminology must pass its validation tests")
    })
}

pub(super) fn normalized(text: &str) -> String {
    // Most catalog names already are NFKC; avoid normalization work for every name while retaining compatibility ideographs/Hangul.
    let folded = if unicode_normalization::is_nfkc_quick(text.chars())
        == unicode_normalization::IsNormalized::Yes
    {
        text.chars()
            .flat_map(char::to_lowercase)
            .collect::<String>()
    } else {
        text.nfkc().flat_map(char::to_lowercase).collect::<String>()
    };
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn exact_query(text: &str) -> &str {
    text.trim_matches(|ch: char| {
        ch.is_whitespace()
            || ch.is_ascii_punctuation()
            || matches!(ch, '。' | '「' | '」' | '『' | '』')
    })
}

#[derive(Clone)]
struct Match<'a> {
    start: usize,
    end: usize,
    term: usize,
    alias: &'a str,
}

fn alias_matches<'a>(query: &str, alias: &'a str, exact: bool, term: usize) -> Vec<Match<'a>> {
    let folded = normalized(alias);
    if folded.is_empty() || (exact && exact_query(query) != folded) {
        return Vec::new();
    }
    query
        .match_indices(&folded)
        .filter_map(|(start, value)| {
            let end = start + value.len();
            // ASCII aliases are words: SSA must never match the middle of assassin.
            let ascii_word = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
            if folded.chars().next().is_some_and(ascii_word)
                && query[..start].chars().next_back().is_some_and(ascii_word)
            {
                return None;
            }
            if folded.chars().next_back().is_some_and(ascii_word)
                && query[end..].chars().next().is_some_and(ascii_word)
            {
                return None;
            }
            Some(Match {
                start,
                end,
                term,
                alias,
            })
        })
        .collect()
}

fn subject_alias_matches<'a>(
    query: &str,
    alias: &'a str,
    term: usize,
    book: &Glossary,
) -> Vec<Match<'a>> {
    let exact = alias_matches(query, alias, true, term);
    if !exact.is_empty() {
        return exact;
    }
    let folded = normalized(alias);
    let Some(rest) = query.strip_prefix(&folded) else {
        return Vec::new();
    };
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('要').unwrap_or(rest).trim_start();
    // Reuse explicit question phrases, not verbs such as 合成: 明明合成了
    // is ordinary prose, whereas 明明要點合 names the subject of a question.
    let question = book
        .intents
        .iter()
        .flat_map(|intent| &intent.aliases)
        .any(|phrase| {
            let phrase = normalized(phrase);
            (phrase.starts_with('點')
                || phrase.starts_with('点')
                || phrase.starts_with('怎')
                || phrase.starts_with("如何"))
                && rest.starts_with(&phrase)
        });
    if question && !folded.is_empty() {
        vec![Match {
            start: 0,
            end: folded.len(),
            term,
            alias,
        }]
    } else {
        Vec::new()
    }
}

fn overlaps(left: &Match<'_>, right: &Match<'_>) -> bool {
    left.start < right.end && right.start < left.end
}

fn longest_matches<'a>(mut matches: Vec<Match<'a>>) -> Vec<Match<'a>> {
    matches.sort_by(|left, right| {
        (right.end - right.start)
            .cmp(&(left.end - left.start))
            .then(left.start.cmp(&right.start))
            .then(left.term.cmp(&right.term))
    });
    let mut selected = Vec::<Match<'a>>::new();
    for found in matches {
        // Equal spans from different entries remain visible as ambiguity. A
        // shorter base alias cannot swallow an explicit set, item, or variant.
        if selected.iter().any(|have| {
            overlaps(have, &found)
                && (have.start != found.start || have.end != found.end || have.term == found.term)
        }) {
            continue;
        }
        selected.push(found);
    }
    selected.sort_by_key(|found| (found.start, found.term));
    selected
}

fn preceding_modifier(query: &str, start: usize) -> Option<&str> {
    let prefix = query[..start].trim_end();
    [
        "覺醒",
        "觉醒",
        "각성",
        "awakened",
        "awa",
        "高階",
        "高阶",
        "進階",
        "进阶",
        "改造",
        "真",
        "advanced",
        "remodeled",
    ]
    .into_iter()
    .find(|modifier| {
        let Some(before) = prefix.strip_suffix(modifier) else {
            return false;
        };
        !modifier.is_ascii()
            || !before
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_alphanumeric())
    })
}

/// Resolve reviewed entity vocabulary and explicit request phrases. The caller
/// still validates each key against its catalog and chooses the relevant data API.
/// User aliases can override this layer without changing the embedded glossary.
pub fn resolve_terms(query: &str) -> TermResolution {
    resolve_terms_with(glossary(), query)
}

pub(crate) fn resolve_terms_with(book: &Glossary, query: &str) -> TermResolution {
    let folded = normalized(query);
    let mut raw = Vec::new();
    for (index, term) in book.entities.iter().enumerate() {
        for alias in &term.aliases {
            raw.extend(alias_matches(&folded, alias, false, index));
        }
        for alias in &term.exact_aliases {
            raw.extend(alias_matches(&folded, alias, true, index));
        }
        for alias in &term.subject_aliases {
            raw.extend(subject_alias_matches(&folded, alias, index, book));
        }
    }
    let mut unresolved_modifiers = Vec::new();
    let selected: Vec<_> = longest_matches(raw)
        .into_iter()
        .filter(|found| {
            if let Some(modifier) = preceding_modifier(&folded, found.start) {
                if !unresolved_modifiers.iter().any(|have| have == modifier) {
                    unresolved_modifiers.push(modifier.to_string());
                }
                false
            } else {
                true
            }
        })
        .collect();
    let mut seen = HashSet::new();
    let entities = selected
        .iter()
        .filter_map(|found| {
            let term = &book.entities[found.term];
            seen.insert(found.term).then(|| EntityMatch {
                term_id: term.id.clone(),
                matched_alias: found.alias.to_string(),
                keys: term.keys.clone(),
                group: term.group,
                evidence: term.evidence.clone(),
            })
        })
        .collect();
    let mut intent_raw = Vec::new();
    for (index, term) in book.intents.iter().enumerate() {
        for alias in &term.aliases {
            intent_raw.extend(alias_matches(&folded, alias, false, index));
        }
        for alias in &term.exact_aliases {
            intent_raw.extend(alias_matches(&folded, alias, true, index));
        }
    }
    // A modifier inside a recognized name (覺醒雀天) is part of the entity,
    // not by itself a request to explain awakening.
    intent_raw.retain(|found| {
        !selected
            .iter()
            .any(|entity| entity.start <= found.start && found.end <= entity.end)
    });
    let intent_matches: Vec<_> = longest_matches(intent_raw)
        .into_iter()
        .map(|found| {
            let term = &book.intents[found.term];
            IntentMatch {
                matched_alias: found.alias.to_string(),
                intent: term.intent,
                topic: term.topic.clone(),
            }
        })
        .collect();
    let intent = if intent_matches
        .iter()
        .any(|found| found.intent == QueryIntent::Acquisition)
    {
        QueryIntent::Acquisition
    } else {
        QueryIntent::Overview
    };
    let mut notes = Vec::new();
    if intent == QueryIntent::Acquisition {
        notes.push(book.acquisition_note.clone());
    }
    if !unresolved_modifiers.is_empty() {
        notes.push("An unrecognized variant modifier was preserved. It may request progression, but no target form was resolved. Specify the canonical target form or its stable key; the base entity was not substituted.".into());
    }
    TermResolution {
        query: query.to_string(),
        intent,
        entities,
        intent_matches,
        unresolved_modifiers,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(query: &str) -> Vec<String> {
        resolve_terms(query)
            .entities
            .into_iter()
            .flat_map(|entity| entity.keys)
            .collect()
    }

    #[test]
    fn cantonese_request_resolves_the_character_and_acquisition_intent() {
        let result = resolve_terms("雀天要點合？");
        assert_eq!(result.intent, QueryIntent::Acquisition);
        assert_eq!(keys(&result.query), ["hero:T49"]);
        assert!(result
            .intent_matches
            .iter()
            .any(|found| found.matched_alias == "點合"));
        assert!(result
            .notes
            .iter()
            .any(|note| note.contains("complete recipe")));
    }

    #[test]
    fn longest_match_separates_character_set_equipment_and_awakened_variants() {
        assert_eq!(keys("雀天"), ["hero:T49"]);
        assert_eq!(keys("雀天套要點合"), ["set:66"]);
        assert_eq!(keys("雀天手要點合"), ["item:6240"]);
        assert_eq!(keys("覺醒雀天"), ["hero:V06"]);
        assert_eq!(keys("覺醒雀天套"), ["set:122"]);
        assert_eq!(keys("覺醒雀天手"), ["item:10699"]);
        assert_eq!(resolve_terms("覺醒雀天").intent, QueryIntent::Overview);
        assert!(keys("高階雀天").is_empty());
        assert_eq!(resolve_terms("高階雀天").unresolved_modifiers, ["高階"]);
    }

    #[test]
    fn canonical_three_language_names_resolve_without_language_specific_search() {
        for (query, key) in [
            ("持國天王", "hero:T49"),
            ("Dhritarashtra", "hero:T49"),
            ("지국천왕", "hero:T49"),
            ("각성 지국천왕 제작", "hero:V06"),
            ("Vaisravana Set", "set:59"),
            ("다문천왕의 손목보호대", "item:6099"),
        ] {
            assert_eq!(keys(query), [key], "{query}");
        }
        assert_eq!(
            resolve_terms("각성 지국천왕 제작").intent,
            QueryIntent::Acquisition
        );
    }

    #[test]
    fn awakening_questions_request_acquisition_without_changing_bare_form_names() {
        for query in [
            "如何覺醒雀天",
            "怎樣覺醒持國天王",
            "點樣覺醒雀天",
            "怎样觉醒雀天",
        ] {
            let result = resolve_terms(query);
            assert_eq!(keys(query), ["hero:V06"], "{query}");
            assert_eq!(result.intent, QueryIntent::Acquisition, "{query}");
        }
        assert_eq!(resolve_terms("覺醒雀天").intent, QueryIntent::Overview);
        let unspecified = resolve_terms("怎樣進階雀天");
        assert_eq!(unspecified.intent, QueryIntent::Acquisition);
        assert!(unspecified.entities.is_empty());
        assert_eq!(unspecified.unresolved_modifiers, ["進階"]);
        assert!(unspecified
            .notes
            .iter()
            .any(|note| note.contains("Specify the canonical target form")));
    }

    #[test]
    fn acronyms_require_word_boundaries_and_group_aliases_keep_all_members() {
        assert!(keys("assassin").is_empty());
        let result = resolve_terms("ＳＳＡ");
        assert_eq!(result.entities.len(), 1);
        assert!(result.entities[0].group);
        assert_eq!(result.entities[0].keys.len(), 5);
        assert_eq!(
            keys("三角"),
            [
                "item:6402",
                "item:6403",
                "item:6404",
                "item:6405",
                "item:6947"
            ]
        );
        assert!(
            keys("小型屬性石箱子").is_empty(),
            "broad group alias must not capture a chest"
        );
        assert_eq!(
            resolve_terms("符合條件").intent,
            QueryIntent::Overview,
            "single 合 is not a phrase match"
        );
    }

    #[test]
    fn common_word_alias_requires_a_standalone_mention_or_question_subject() {
        for query in ["明冥", "明明", "「明明」？"] {
            assert_eq!(keys(query), ["hero:T88"], "{query}");
            assert_eq!(resolve_terms(query).intent, QueryIntent::Overview);
        }
        for query in ["明冥要點合", "明明要點合", "明明點合", "明明 要 如何合成？"]
        {
            assert_eq!(keys(query), ["hero:T88"], "{query}");
            assert_eq!(resolve_terms(query).intent, QueryIntent::Acquisition);
        }
        for query in [
            "明明白白",
            "我明明有材料",
            "明明有材料點解合唔到",
            "明明合成了",
        ] {
            assert!(keys(query).is_empty(), "{query}");
        }
        assert_eq!(keys("雀天明明要點合"), ["hero:T49"]);
    }

    #[test]
    fn yamantaka_form_names_remain_distinct_from_the_base_nickname() {
        for query in ["覺醒大威德明王", "Awa Yamantaka", "각성 대위덕명왕"] {
            assert_eq!(keys(query), ["hero:V27"], "{query}");
            assert_eq!(resolve_terms(query).intent, QueryIntent::Overview);
        }
        let question = resolve_terms("如何覺醒大威德明王");
        assert_eq!(keys(&question.query), ["hero:V27"]);
        assert_eq!(question.intent, QueryIntent::Acquisition);
        for query in ["覺醒明冥", "高階明冥", "覺醒明明"] {
            assert!(
                keys(query).is_empty(),
                "must not substitute the base form: {query}"
            );
        }
    }

    #[test]
    fn community_nicknames_preserve_the_documented_character_stage() {
        for (alias, key) in [
            ("日明", "hero:T87"),
            ("日冥", "hero:T87"),
            ("朝明", "hero:T86"),
            ("朝冥", "hero:T86"),
            ("台明", "hero:T89"),
            ("台冥", "hero:T89"),
            ("印明", "hero:T113"),
            ("印冥", "hero:T113"),
            ("MP女", "hero:U90"),
            ("小燕", "hero:U91"),
            ("權2", "hero:T1"),
            ("龜車", "hero:U102"),
            ("祖3", "hero:T56"),
            ("雷3", "hero:T68"),
        ] {
            let query = format!("{alias}要點合？");
            assert_eq!(keys(&query), [key], "{query}");
            assert_eq!(resolve_terms(&query).intent, QueryIntent::Acquisition);
        }
        assert_eq!(keys("真龜甲車"), ["hero:T40"]);
        assert!(
            keys("真龜車").is_empty(),
            "unknown shorthand must not select the base form"
        );
        assert!(
            keys("覺醒日明").is_empty(),
            "unknown shorthand must not select the base form"
        );
    }

    #[test]
    fn bundled_terms_have_evidence_unique_ids_and_real_typed_targets() {
        let catalog = super::super::Catalog::open().expect("catalog");
        let mut ids = HashSet::new();
        let mut aliases = std::collections::HashMap::<String, &str>::new();
        for term in &glossary().entities {
            assert!(ids.insert(&term.id), "duplicate {}", term.id);
            assert!(!term.evidence.is_empty());
            assert!(!term.keys.is_empty());
            for alias in term
                .aliases
                .iter()
                .chain(&term.exact_aliases)
                .chain(&term.subject_aliases)
            {
                if ![
                    &term.canonical_names.en,
                    &term.canonical_names.zh,
                    &term.canonical_names.ko,
                ]
                .contains(&alias)
                {
                    assert!(
                        term.evidence
                            .iter()
                            .any(|evidence| evidence.spellings.contains(alias)),
                        "non-canonical spelling lacks specific evidence: {} / {alias}",
                        term.id
                    );
                }
                let normalized = normalized(alias);
                if let Some(previous) = aliases.insert(normalized, &term.id) {
                    assert_eq!(previous, term.id, "ambiguous alias {alias}");
                }
            }
            for key in &term.keys {
                let (kind, code) = key.split_once(':').expect("typed key");
                let exists = match kind {
                    "hero" => catalog.index.heroes.iter().any(|row| row.code == code),
                    "set" => catalog
                        .index
                        .sets
                        .iter()
                        .any(|row| row.code.to_string() == code),
                    "item" => code
                        .parse::<i64>()
                        .ok()
                        .is_some_and(|code| catalog.index.find_item(code).is_some()),
                    _ => false,
                };
                assert!(exists, "missing {key}");
            }
        }
    }
}
