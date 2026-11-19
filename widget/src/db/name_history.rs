//! Catalog search and detail history read a small name-only index, independently of the update page.
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
pub(crate) struct NameChange {
    pub(crate) lang: String,
    pub(crate) before: String,
    pub(crate) after: String,
}

pub(crate) struct RenameEvent {
    pub(crate) from: &'static str,
    pub(crate) to: &'static str,
    pub(crate) changes: &'static [NameChange],
}

#[derive(Deserialize)]
struct Event {
    version: usize,
    changes: Vec<NameChange>,
}

#[derive(Deserialize)]
struct Lifetime {
    since: usize,
    until: Option<usize>,
    events: Vec<Event>,
}

#[derive(Deserialize)]
struct Index {
    versions: Vec<String>,
    entities: BTreeMap<String, Vec<Lifetime>>,
}

fn index() -> Option<&'static Index> {
    static INDEX: OnceLock<Option<Index>> = OnceLock::new();
    INDEX
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../../data/catalog-names.json"))
                .inspect_err(|err| eprintln!("Could not read catalog name history: {err}"))
                .ok()
        })
        .as_ref()
}

fn recorded_events(lives: &[Lifetime], version: usize) -> &[Event] {
    let Some(life) = lives
        .iter()
        .find(|s| s.since <= version && s.until.is_none_or(|end| version < end))
    else {
        return &[];
    };
    &life.events[..life.events.partition_point(|e| e.version <= version)]
}

pub(crate) fn events(version: &str, route: &str) -> Vec<RenameEvent> {
    let Some(index) = index() else {
        return vec![];
    };
    let (Some(lives), Some(version)) = (
        index.entities.get(route),
        index.versions.iter().position(|v| v == version.trim()),
    ) else {
        return vec![];
    };
    recorded_events(lives, version)
        .iter()
        .rev()
        .filter_map(|e| {
            Some(RenameEvent {
                from: index.versions.get(e.version.checked_sub(1)?)?.as_str(),
                to: index.versions.get(e.version)?.as_str(),
                changes: &e.changes,
            })
        })
        .collect()
}

/// Each pair can be appended to the existing search terms of the installed entity.
pub(crate) fn previous_names(version: &str) -> Vec<(&'static str, &'static str)> {
    let Some(index) = index() else {
        return vec![];
    };
    let Some(version) = index.versions.iter().position(|v| v == version.trim()) else {
        return vec![];
    };
    let mut names = Vec::new();
    for (route, lives) in &index.entities {
        let mut seen = HashSet::new();
        for change in recorded_events(lives, version)
            .iter()
            .flat_map(|e| &e.changes)
        {
            let old = change.before.as_str();
            if !old.is_empty() && seen.insert(old) {
                names.push((route.as_str(), old));
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_past_names_from_the_installed_entity_lifetime_are_visible() {
        let lives: Vec<Lifetime> = serde_json::from_str(
            r#"[
            {"since":0,"until":2,"events":[{"version":1,"changes":[]}]},
            {"since":3,"events":[{"version":4,"changes":[]},{"version":6,"changes":[]}]}
        ]"#,
        )
        .unwrap();
        assert!(recorded_events(&lives, 0).is_empty());
        assert_eq!(recorded_events(&lives, 1)[0].version, 1);
        assert!(recorded_events(&lives, 2).is_empty());
        assert!(recorded_events(&lives, 3).is_empty());
        assert_eq!(recorded_events(&lives, 5).len(), 1);
        assert_eq!(recorded_events(&lives, 6)[1].version, 6);
    }

    #[test]
    fn catalog_uses_the_affected_language_and_searchable_old_names() {
        let aliases = previous_names("V3.4411");
        assert!(aliases.contains(&("item:4518", "Sliver Key")));
        assert!(aliases.contains(&("hero:U96", "Sorcerer")));
        assert!(previous_names("V3.4220").is_empty());
        assert!(previous_names("unrecorded").is_empty());
        let rows = events("V3.4411", "npc:1332");
        let event = &rows[0];
        assert_eq!((event.from, event.to), ("V3.4220", "V3.4411"));
        assert!(event
            .changes
            .iter()
            .any(|c| c.lang == "zh" && c.before == "沉睡的靈魂" && c.after.is_empty()));
    }
}
