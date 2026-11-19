//! Offline net differences between any two recorded game versions.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::mem;

use serde::Deserialize;
use serde_json::{value::RawValue, Value};

use super::{
    drops::{DropSnapshot, LootSlot},
    Entry, Kind, Name, Region,
};

#[derive(Deserialize)]
pub(super) struct Report {
    pub(super) from_version: String,
    pub(super) to_version: String,
    pub(super) entries: Vec<Entry>,
}

#[derive(Deserialize)]
pub(super) struct History {
    pub(super) versions: Vec<String>,
    releases: Vec<Release>,
}

#[derive(Deserialize)]
struct Release {
    from_version: String,
    to_version: String,
    #[serde(default)]
    region_changes: HashMap<String, RegionChange>,
    #[serde(default)]
    caption_changes: Vec<CaptionChange>,
    #[serde(default)]
    loot_updates: Option<Box<RawValue>>,
    // Only decode the rows between the selected endpoints. Opening the latest
    // release does not build per-field maps for years of previous snapshots.
    entries: Box<RawValue>,
}

#[derive(Deserialize)]
struct RegionChange {
    before: Vec<Region>,
    after: Vec<Region>,
}

#[derive(Deserialize)]
struct CaptionChange {
    table: String,
    key: String,
    before: Name,
    after: Name,
    #[serde(default)]
    drop_before: Option<DropSnapshot>,
    #[serde(default)]
    drop_after: Option<DropSnapshot>,
}

impl History {
    pub(super) fn parse(src: &str) -> Result<Self, String> {
        let history: Self = serde_json::from_str(src).map_err(|error| error.to_string())?;
        let versions = &history.versions;
        if versions.is_empty()
            || versions.iter().any(|v| v.trim().is_empty())
            || versions.iter().collect::<HashSet<_>>().len() != versions.len()
            || history.releases.len() != versions.len() - 1
            || history
                .releases
                .iter()
                .zip(versions.windows(2))
                .any(|(release, pair)| {
                    release.from_version != pair[0] || release.to_version != pair[1]
                })
        {
            return Err("Invalid game-version history chain".into());
        }
        Ok(history)
    }

    pub(super) fn report(&self, from: usize, to: usize) -> Option<Report> {
        if from >= to {
            return None;
        }
        let transitions = self.releases.get(from..to)?;
        let entries = match transitions {
            [release] => serde_json::from_str(release.entries.get()).ok()?,
            _ => net_changes(transitions)?,
        };
        Some(Report {
            from_version: self.versions[from].clone(),
            to_version: self.versions[to].clone(),
            entries,
        })
    }
}

// Keep both endpoints of every touched field. None means an absent column or
// relation, distinct from the actual SQL NULL value represented by Value::Null.
struct Change {
    existed: bool,
    old_name: Name,
    old_regions: Vec<Region>,
    old_drop: Option<DropSnapshot>,
    fields: BTreeMap<String, (Option<Value>, Option<Value>)>,
    latest: Entry,
}

impl Change {
    fn new(mut entry: Entry) -> Self {
        let mut change = Self {
            existed: !matches!(entry.kind, Kind::Added),
            old_name: entry.previous_name.as_ref().unwrap_or(&entry.name).clone(),
            old_regions: entry
                .previous_regions
                .as_ref()
                .unwrap_or(&entry.regions)
                .clone(),
            old_drop: entry.drop_before.take(),
            fields: BTreeMap::new(),
            latest: entry,
        };
        change.collect_fields();
        change
    }

    fn collect_fields(&mut self) {
        let mut after = mem::take(&mut self.latest.after);
        for (field, old) in mem::take(&mut self.latest.before) {
            let new = after.remove(&field);
            self.fields.entry(field).or_insert((Some(old), None)).1 = new;
        }
        for (field, new) in after {
            self.fields.entry(field).or_default().1 = Some(new);
        }
    }

    fn finish(self) -> Option<Entry> {
        let mut entry = self.latest;
        let exists = !matches!(entry.kind, Kind::Removed);
        if !self.existed && !exists {
            return None;
        }
        for (field, (old, new)) in self.fields {
            if old == new {
                continue;
            }
            if let Some(old) = old {
                entry.before.insert(field.clone(), old);
            }
            if let Some(new) = new {
                entry.after.insert(field, new);
            }
        }
        entry.previous_name = None;
        entry.previous_regions = None;
        entry.drop_before = self.old_drop;
        entry.kind = match (self.existed, exists) {
            (false, true) => Kind::Added,
            (true, false) => {
                entry.name = self.old_name;
                entry.regions = self.old_regions;
                Kind::Removed
            }
            (true, true) => {
                if entry.before.is_empty() && entry.after.is_empty() {
                    return None;
                }
                if self.old_name != entry.name {
                    entry.previous_name = Some(self.old_name);
                }
                if self.old_regions != entry.regions {
                    entry.previous_regions = Some(self.old_regions);
                }
                if renamed(&entry) {
                    Kind::Renamed
                } else {
                    Kind::Modified
                }
            }
            (false, false) => unreachable!(),
        };
        Some(entry)
    }
}

fn renamed(entry: &Entry) -> bool {
    let named = |value: &Value| {
        value
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.trim() != "0")
    };
    entry.before.iter().any(|(field, old)| {
        let name_field = matches!(
            field.as_str(),
            "name_ko" | "name_en" | "name_zh" | "result_name_ko"
        ) || (entry.table == "quests" && field == "title");
        name_field
            && old != entry.after.get(field).unwrap_or(&Value::Null)
            && named(old)
            && entry.after.get(field).is_some_and(named)
    })
}

fn net_changes(releases: &[Release]) -> Option<Vec<Entry>> {
    let mut changes: Vec<Change> = Vec::new();
    let mut indices = HashMap::<(String, String), usize>::new();
    let mut regions = HashMap::new();
    let mut loot = HashMap::new();
    // Only a few rows inherit a caption change during an interval where their
    // own data stays still. Track the first/last titles of those rows only.
    let mut captions = HashMap::new();
    for change in releases.iter().flat_map(|r| &r.caption_changes) {
        captions.insert((change.table.clone(), change.key.clone()), None);
    }
    type Caption = (Name, Option<DropSnapshot>);
    let track = |span: &mut Option<(Caption, Caption)>,
                 before: &Name,
                 after: &Name,
                 old_drop: &Option<DropSnapshot>,
                 new_drop: &Option<DropSnapshot>| {
        let new = (after.clone(), new_drop.clone());
        if let Some((_, last)) = span {
            *last = new;
        } else {
            *span = Some(((before.clone(), old_drop.clone()), new));
        }
    };
    for release in releases {
        if !loot.is_empty() {
            if let Some(updates) = &release.loot_updates {
                let rosters: HashMap<String, Vec<LootSlot>> =
                    serde_json::from_str(updates.get()).ok()?;
                for (monster, slots) in rosters {
                    if let Some(current) = loot.get_mut(&monster) {
                        *current = slots;
                    }
                }
            }
        }
        for change in &release.caption_changes {
            if let Some(span) = captions.get_mut(&(change.table.clone(), change.key.clone())) {
                track(
                    span,
                    &change.before,
                    &change.after,
                    &change.drop_before,
                    &change.drop_after,
                );
            }
        }
        for (code, group) in &release.region_changes {
            regions
                .entry(code.as_str())
                .or_insert((&group.before, &group.after))
                .1 = &group.after;
        }
        let rows: Vec<Entry> = serde_json::from_str(release.entries.get()).ok()?;
        for mut entry in rows {
            if entry.table == "monsters" {
                match entry.kind {
                    Kind::Added => {
                        loot.insert(entry.key.clone(), mem::take(&mut entry.loot));
                    }
                    Kind::Removed => {
                        loot.remove(&entry.key);
                    }
                    _ => {}
                }
            }
            let key = (entry.table.clone(), entry.key.clone());
            if let Some(span) = captions.get_mut(&key) {
                track(
                    span,
                    entry.previous_name.as_ref().unwrap_or(&entry.name),
                    &entry.name,
                    &entry.drop_before,
                    &entry.drop_after,
                );
            }
            if let Some(index) = indices.get(&key) {
                let change = &mut changes[*index];
                change.latest = entry;
                change.collect_fields();
            } else {
                indices.insert(key, changes.len());
                changes.push(Change::new(entry));
            }
        }
    }
    let mut rows: Vec<_> = changes.into_iter().filter_map(Change::finish).collect();
    for entry in &mut rows {
        if entry.table == "monsters" && matches!(entry.kind, Kind::Added) {
            entry.loot = loot.remove(&entry.key).unwrap_or_default();
        }
        if !captions.is_empty() {
            if let Some(Some(((old, old_drop), (new, new_drop)))) =
                captions.remove(&(entry.table.clone(), entry.key.clone()))
            {
                match entry.kind {
                    Kind::Added => {
                        entry.name = new;
                        entry.drop_after = new_drop;
                    }
                    Kind::Removed => {
                        entry.name = old;
                        entry.drop_before = old_drop;
                    }
                    Kind::Modified | Kind::Renamed => {
                        entry.previous_name = (old != new).then_some(old);
                        entry.name = new;
                        entry.drop_before = old_drop;
                        entry.drop_after = new_drop;
                    }
                }
            }
        }
        if entry.category != "monsters" {
            continue;
        }
        let code = entry.key.split(" / ").next().unwrap_or(&entry.key);
        if let Some(&(old, new)) = regions.get(code) {
            entry.regions = if matches!(entry.kind, Kind::Removed) {
                old
            } else {
                new
            }
            .clone();
            if matches!(entry.kind, Kind::Modified | Kind::Renamed) {
                entry.previous_regions = (old != new).then(|| old.clone());
            }
        }
    }
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(key: &str, kind: &str, before: Value, after: Value) -> Value {
        json!({"table":"items","category":"items","key":key,"name":{},"kind":kind,"before":before,"after":after})
    }

    fn history() -> History {
        let drop_at = |rate, name| json!({"item_code":90,"rate":rate,"item":{"en":name},"monster":{"en":"Fox"}});
        let first = vec![
            entry("transient", "added", json!({}), json!({"cost":1})),
            entry(
                "reverted",
                "modified",
                json!({"level":1}),
                json!({"level":2}),
            ),
            entry("restored", "removed", json!({"cost":null}), json!({})),
            entry(
                "mixed",
                "renamed",
                json!({"name_en":"X","cost":3}),
                json!({"name_en":"Y","cost":4}),
            ),
            entry("replaced", "removed", json!({"a":1}), json!({})),
            json!({"category":"monsters","table":"monster_drops","key":"M / 1","name":{"en":"Fox · Leaf"},"kind":"modified",
                   "regions":[{"id":"1","name":{}}],"before":{"drop_rate_per_100000":2},"after":{"drop_rate_per_100000":3},
                   "drop_before":drop_at(2,"Leaf"),"drop_after":drop_at(3,"Leaf")}),
        ];
        let second = vec![
            entry("transient", "removed", json!({"cost":1}), json!({})),
            entry(
                "reverted",
                "modified",
                json!({"level":2}),
                json!({"level":1}),
            ),
            entry("restored", "added", json!({}), json!({"cost":null})),
            entry(
                "mixed",
                "renamed",
                json!({"name_en":"Y"}),
                json!({"name_en":"X"}),
            ),
            entry("replaced", "added", json!({}), json!({"a":null,"b":2})),
            entry("new", "added", json!({}), json!({})),
        ];
        let source = json!({
            "versions":["A","B","C"],
            "releases":[{"from_version":"A","to_version":"B","entries":first},
                        {"from_version":"B","to_version":"C","entries":second,
                         "region_changes":{"M":{"before":[{"id":"1","name":{}}],"after":[{"id":"2","name":{}}]}},
                         "caption_changes":[{"table":"monster_drops","key":"M / 1",
                                             "before":{"en":"Fox · Leaf"},"after":{"en":"Fox · Herb"},
                                             "drop_before":drop_at(3,"Leaf"),"drop_after":drop_at(3,"Herb")}]}]
        });
        History::parse(&source.to_string()).unwrap()
    }

    #[test]
    fn nonadjacent_pair_keeps_endpoint_changes_and_region_even_when_drops_did_not_change_again() {
        let report = history().report(0, 2).unwrap();
        let keys: Vec<_> = report.entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["mixed", "replaced", "M / 1", "new"]);
        let e = &report.entries[0];
        assert!(matches!(e.kind, Kind::Modified));
        assert_eq!(e.before, BTreeMap::from([("cost".into(), json!(3))]));
        assert_eq!(e.after, BTreeMap::from([("cost".into(), json!(4))]));
        let e = &report.entries[1];
        assert!(matches!(e.kind, Kind::Modified));
        assert_eq!(e.before, BTreeMap::from([("a".into(), json!(1))]));
        assert_eq!(
            e.after,
            BTreeMap::from([("a".into(), Value::Null), ("b".into(), json!(2))])
        );
        assert_eq!(report.entries[2].regions[0].id, "2");
        assert_eq!(report.entries[2].name.en, "Fox · Herb");
        let old = report.entries[2].drop_before.as_ref().unwrap();
        let new = report.entries[2].drop_after.as_ref().unwrap();
        assert_eq!(
            (old.item_code, old.rate, old.item.en.as_str()),
            (90, Some(2), "Leaf")
        );
        assert_eq!(
            (new.item_code, new.rate, new.item.en.as_str()),
            (90, Some(3), "Herb")
        );
        assert_eq!(
            report.entries[2].previous_name.as_ref().unwrap().en,
            "Fox · Leaf"
        );
        assert!(matches!(report.entries[3].kind, Kind::Added));
    }

    #[test]
    fn only_ordered_archived_endpoints_can_be_compared() {
        let history = history();
        let report = history.report(0, 2).unwrap();
        assert_eq!((&*report.from_version, &*report.to_version), ("A", "C"));
        assert!(history.report(2, 0).is_none());
        assert!(history.report(1, 0).is_none());
        assert!(history.report(1, 1).is_none());
        assert!(history.report(1, 3).is_none());
    }

    #[test]
    fn added_monsters_show_the_full_loot_at_the_selected_endpoint() {
        let slot = |slot, name, rate: Option<i64>| json!({"slot":slot,"item_code":50,"item":{"en":name},"rate":rate});
        let first = json!({"table":"monsters","category":"monsters","key":"X","name":{},"kind":"added",
                           "before":{},"after":{"hp":10},"loot":[slot(3,"Leaf",Some(1)),slot(7,"Leaf",None)]});
        let modified = json!({"table":"monsters","category":"monsters","key":"X","name":{},"kind":"modified",
                             "before":{"hp":10},"after":{"hp":20}});
        let source = json!({"versions":["A","B","C","D"],"releases":[
            {"from_version":"A","to_version":"B","entries":[first]},
            {"from_version":"B","to_version":"C","entries":[modified],
             "loot_updates":{"X":[slot(3,"Herb",Some(2)),slot(7,"Herb",None)]}},
            {"from_version":"C","to_version":"D","entries":[],"loot_updates":{"X":[]}}
        ]});
        let archive = History::parse(&source.to_string()).unwrap();
        let added = |end| archive.report(0, end).unwrap().entries.remove(0);
        let initial = added(1);
        assert_eq!(initial.loot.len(), 2);
        assert_eq!(initial.loot[0].drop.item.en, "Leaf");
        let later = added(2);
        assert_eq!(later.loot.len(), 2);
        assert_eq!((later.loot[0].slot, later.loot[0].drop.rate), (3, Some(2)));
        assert_eq!(later.loot[1].drop.item.en, "Herb");
        assert_eq!(later.loot[1].drop.rate, None);
        assert_eq!(later.after["hp"], 20);
        assert!(added(3).loot.is_empty());
        assert!(archive.report(1, 3).unwrap().entries[0].loot.is_empty());
    }
}
