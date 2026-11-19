//! Project archived monster and drop-slot records onto the same item names and
//! probability scale as the catalog. Only artwork and usable routes are local.
use serde::Deserialize;
use serde_json::Value;
use std::fmt::Write;

use super::artwork::Subject;
use super::{db, Artwork, Entry, Name, Region};
use crate::GameUpdateDrop;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(super) struct DropSnapshot {
    pub(super) item_code: i64,
    pub(super) rate: Option<i64>,
    #[serde(default)]
    pub(super) item: Name,
    #[serde(default)]
    pub(super) monster: Name,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(super) struct LootSlot {
    pub(super) slot: i64,
    #[serde(flatten)]
    pub(super) drop: DropSnapshot,
}

pub(super) struct MonsterSummary {
    pub(super) code: String,
    pub(super) title: String,
    pub(super) location: String,
}

fn region_name(region: &Region, lang: &str) -> String {
    let title = region.name.display(lang);
    if title.is_empty() {
        format!("#{}", region.id)
    } else {
        title
    }
}

pub(super) fn locations(regions: &[Region], lang: &str) -> String {
    regions
        .iter()
        .map(|r| region_name(r, lang))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Compare precisely the IDs present on this side. A removed location might
/// only have a label in the older snapshot; retain its ID if neither has one.
pub(super) fn location_value(
    entry: &Entry,
    value: &Value,
    lang: &str,
    before: bool,
) -> Option<String> {
    let previous = entry.previous_regions.as_deref().unwrap_or(&entry.regions);
    let (preferred, fallback) = if before {
        (previous, entry.regions.as_slice())
    } else {
        (entry.regions.as_slice(), previous)
    };
    let label = |id: i64| {
        let key = id.to_string();
        preferred
            .iter()
            .chain(fallback)
            .find(|r| r.id == key)
            .map(|r| region_name(r, lang))
            .unwrap_or_else(|| format!("#{id}"))
    };
    match value {
        Value::Array(ids) => {
            let ids = ids.iter().map(Value::as_i64).collect::<Option<Vec<_>>>()?;
            let labels = ids
                .into_iter()
                .filter(|id| *id > 0)
                .map(label)
                .collect::<Vec<_>>();
            Some(if labels.is_empty() {
                "—".to_owned()
            } else {
                labels.join(" / ")
            })
        }
        value => value
            .as_i64()
            .map(|id| if id > 0 { label(id) } else { "—".to_owned() }),
    }
}

pub(super) fn search_text(entry: &Entry, lang: &str) -> String {
    let mut text = String::new();
    for side in entry
        .drop_before
        .iter()
        .chain(&entry.drop_after)
        .chain(entry.loot.iter().map(|s| &s.drop))
    {
        let _ = write!(
            text,
            " #{} {}",
            side.item_code,
            db::drop_rate_text(side.rate, lang)
        );
        for name in [&side.monster, &side.item] {
            let _ = write!(text, " {} {} {}", name.en, name.zh, name.ko);
        }
    }
    for region in entry.previous_regions.iter().flatten() {
        let _ = write!(
            text,
            " #{} {} {} {}",
            region.id, region.name.en, region.name.zh, region.name.ko
        );
    }
    text
}

pub(super) fn monster(entry: &Entry, lang: &str) -> Option<MonsterSummary> {
    if entry.category != "monsters" {
        return None;
    }
    let code = entry.key.split(" / ").next()?.to_owned();
    let title = if entry.table == "monster_drops" {
        entry
            .drop_after
            .as_ref()
            .or(entry.drop_before.as_ref())
            .map(|s| s.monster.display(lang))
            .unwrap_or_default()
    } else {
        entry.name.display(lang)
    };
    let location = if entry.regions.is_empty() {
        db::ui_text(lang, "地區未記錄", "Region not recorded")
    } else {
        locations(&entry.regions, lang)
    };
    Some(MonsterSummary {
        title: if title.is_empty() {
            code.clone()
        } else {
            title
        },
        code,
        location,
    })
}

pub(super) fn line_count(entry: &Entry) -> usize {
    match (&entry.drop_before, &entry.drop_after) {
        (Some(before), Some(after)) if before.item_code != after.item_code => 2,
        (None, None) => 0,
        _ => 1,
    }
}

fn item_row(
    snapshot: &DropSnapshot,
    slot: Option<i64>,
    lang: &str,
    artwork: Option<&Artwork>,
    position: usize,
) -> GameUpdateDrop {
    let subject = Subject::Item(snapshot.item_code);
    let icon = artwork.and_then(|art| art.image(&subject, position));
    let route = artwork
        .and_then(|art| art.route(&subject))
        .unwrap_or_default();
    let mut title = snapshot.item.display(lang);
    if title.is_empty() {
        title = format!("#{}", snapshot.item_code);
    }
    if slot.is_some_and(|n| n >= 9) {
        let _ = write!(
            title,
            " ({})",
            db::ui_text_ref(lang, "單人限定", "Solo only")
        );
    }
    GameUpdateDrop {
        title: title.into(),
        route: route.into(),
        has_icon: icon.is_some(),
        icon: icon.unwrap_or_default(),
        ..Default::default()
    }
}

pub(super) fn loot_item(
    slot: &LootSlot,
    lang: &str,
    artwork: Option<&Artwork>,
    position: usize,
) -> GameUpdateDrop {
    GameUpdateDrop {
        kind: "loot".into(),
        rate: db::drop_rate_text(slot.drop.rate, lang).into(),
        ..item_row(&slot.drop, Some(slot.slot), lang, artwork, position)
    }
}

pub(super) fn items(
    entry: &Entry,
    lang: &str,
    artwork: Option<&Artwork>,
    record_id: usize,
) -> Vec<GameUpdateDrop> {
    let slot = entry
        .key
        .rsplit_once(" / ")
        .and_then(|(_, s)| s.parse().ok());
    let make_item = |snapshot: &DropSnapshot,
                     kind: &str,
                     before: Option<&DropSnapshot>,
                     after: Option<&DropSnapshot>| {
        GameUpdateDrop {
            kind: kind.into(),
            previous_rate: before
                .map(|s| db::drop_rate_text(s.rate, lang))
                .unwrap_or_default()
                .into(),
            rate: after
                .map(|s| db::drop_rate_text(s.rate, lang))
                .unwrap_or_default()
                .into(),
            ..item_row(snapshot, slot, lang, artwork, record_id)
        }
    };
    match (&entry.drop_before, &entry.drop_after) {
        (Some(before), Some(after)) if before.item_code == after.item_code => {
            vec![make_item(after, "changed", Some(before), Some(after))]
        }
        (before, after) => before
            .iter()
            .map(|s| make_item(s, "removed", Some(s), None))
            .chain(after.iter().map(|s| make_item(s, "added", None, Some(s))))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn treats_item_replacement_as_two_distinct_drops_and_keeps_unknown_rates() {
        let entry: Entry = serde_json::from_value(json!({
            "table":"monster_drops","category":"monsters","key":"K188 / 4","kind":"modified","name":{},
            "before":{"item_code":1},"after":{"item_code":2},
            "drop_before":{"item_code":1,"item":{"en":"Old item"},"rate":0},
            "drop_after":{"item_code":2,"item":{"en":"New item"},"rate":null}
        })).unwrap();
        let rows = items(&entry, "en", None, 0);
        assert_eq!(line_count(&entry), 2);
        assert_eq!(
            (
                rows[0].title.as_str(),
                rows[0].kind.as_str(),
                rows[0].previous_rate.as_str(),
                rows[0].rate.as_str()
            ),
            ("Old item", "removed", "0%", "")
        );
        assert_eq!(
            (
                rows[1].title.as_str(),
                rows[1].kind.as_str(),
                rows[1].previous_rate.as_str(),
                rows[1].rate.as_str()
            ),
            ("New item", "added", "", "Not recorded")
        );
    }

    #[test]
    fn region_fields_use_the_names_in_each_snapshot_even_when_ids_are_reused() {
        let entry: Entry = serde_json::from_value(json!({
            "table":"monster_locations","category":"monsters","key":"M14","kind":"modified","name":{},
            "before":{"location_gts":[1,2]},"after":{"location_gts":[1,3]},
            "regions":[{"id":"1","name":{"en":"New marsh"}}],
            "previous_regions":[{"id":"1","name":{"en":"Old marsh"}},{"id":"2","name":{"en":"Meadow"}}]
        })).unwrap();
        assert_eq!(
            location_value(&entry, &entry.before["location_gts"], "en", true).as_deref(),
            Some("Old marsh / Meadow")
        );
        assert_eq!(
            location_value(&entry, &entry.after["location_gts"], "en", false).as_deref(),
            Some("New marsh / #3")
        );
    }

    #[test]
    fn new_monster_loot_matches_every_slot_in_the_bundled_target_database() {
        use rusqlite::{Connection, OpenFlags};
        use std::path::Path;

        let history = super::super::bundled_history().unwrap();
        let report = history
            .report(history.versions.len() - 2, history.versions.len() - 1)
            .unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/gersang.db");
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let mut query = conn.prepare(
            "SELECT d.slot, d.item_code, d.drop_rate_per_100000,
                    IFNULL(i.name_ko,''), IFNULL(en.text,''), IFNULL(zh.text,'')
             FROM monster_drops d LEFT JOIN items i ON i.code=d.item_code
             LEFT JOIN strings en ON en.object='OnlineText.dat' AND en.lang='en' AND i.name_gts>0 AND en.id=printf('%08d',i.name_gts)
             LEFT JOIN strings zh ON zh.object='OnlineText.dat' AND zh.lang='zh' AND i.name_gts>0 AND zh.id=printf('%08d',i.name_gts)
             WHERE d.monster_code=? ORDER BY d.slot"
        ).unwrap();
        for entry in report
            .entries
            .iter()
            .filter(|e| e.table == "monsters" && matches!(e.kind, super::super::Kind::Added))
        {
            let expected = query
                .query_map([&entry.key], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let actual = entry
                .loot
                .iter()
                .map(|s| {
                    (
                        s.slot,
                        s.drop.item_code,
                        s.drop.rate,
                        s.drop.item.ko.clone(),
                        s.drop.item.en.clone(),
                        s.drop.item.zh.clone(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                actual, expected,
                "incomplete or stale loot for {}",
                entry.key
            );
        }
        let index = |v: &str| history.versions.iter().position(|name| name == v).unwrap();
        let historic = history.report(index("V3.4220"), index("V3.4411")).unwrap();
        let first = historic
            .entries
            .iter()
            .find(|e| e.table == "monsters" && e.key == "P125")
            .unwrap();
        assert_eq!(first.loot.len(), 9);
        let single = first.loot.iter().find(|s| s.slot == 9).unwrap();
        let row = loot_item(single, "zh", None, 0);
        assert_eq!(
            (single.drop.item_code, row.title.as_str(), row.rate.as_str()),
            (9464, "降三世明王的戒指 (單人限定)", "0.001%")
        );
        assert!(search_text(first, "en").contains("Cao Guojiu Old Sword Case"));
    }

    #[test]
    fn archived_drop_rate_only_change_has_its_item_code_and_exact_location() {
        let history = super::super::bundled_history().unwrap();
        let index = |v: &str| history.versions.iter().position(|name| name == v).unwrap();
        let report = history.report(index("V3.4220"), index("V3.4411")).unwrap();
        let entry = report
            .entries
            .iter()
            .find(|e| e.table == "monster_drops" && e.key == "K188 / 4")
            .unwrap();
        assert_eq!(entry.drop_after.as_ref().unwrap().item_code, 9605);
        let rows = items(entry, "zh", None, 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (
                rows[0].title.as_str(),
                rows[0].previous_rate.as_str(),
                rows[0].rate.as_str()
            ),
            ("冰封之星", "15%", "12%")
        );
        let summary = monster(entry, "zh").unwrap();
        assert_eq!(
            (summary.code.as_str(), summary.title.as_str()),
            ("K188", "道士_女")
        );
        assert!(summary.location.contains("星之祠堂(水)"));
    }
}
