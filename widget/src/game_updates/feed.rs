//! Lightweight section index. A collapsed section never formats fields or requests artwork.
use std::cell::OnceCell;
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;
use std::rc::Rc;

use slint::{Model, ModelExt, ModelRc, VecModel};

use super::{db, drops, field_label, table_label, value_text, Artwork, Entry, Kind, Report};
use crate::{GameUpdateFeedRow, GameUpdateLine, GameUpdateRow};

struct Section {
    id: &'static str,
    category: &'static str,
    zh: &'static str,
    en: &'static str,
}

const SECTIONS: &[Section] = &[
    section("new-monsters", "monsters", "新增怪物", "New monsters"),
    section("new-items", "items", "新增道具", "New items"),
    section("renamed", "renamed", "改名", "Renamed"),
    section("monsters", "monsters", "怪物與掉落", "Monsters and drops"),
    section("items", "items", "道具與裝備", "Items and equipment"),
    section("recipes", "recipes", "配方", "Recipes"),
    section("quests", "quests", "任務", "Quests"),
    section("places", "places", "地點與 NPC", "Locations and NPCs"),
    section(
        "characters",
        "characters",
        "角色與傭兵",
        "Characters / Mercenaries",
    ),
    section("production", "production", "生產", "Production"),
    section("enchant", "enchant", "鬼道", "Enchantments"),
    section("patches", "patches", "更新公告", "Patch notes"),
    section("extra", "other", "未分類與文字", "Unclassified and text"),
];
const fn section(
    id: &'static str,
    category: &'static str,
    zh: &'static str,
    en: &'static str,
) -> Section {
    Section {
        id,
        category,
        zh,
        en,
    }
}

fn section_for(entry: &Entry) -> usize {
    let id = match (&*entry.table, entry.kind) {
        (_, Kind::Renamed) => "renamed",
        ("monsters", Kind::Added) => "new-monsters",
        ("items", Kind::Added) => "new-items",
        ("strings" | "quest_lines", _) => "extra",
        _ if entry.is_long_prose_change() => "extra",
        _ => entry.category.as_str(),
    };
    SECTIONS
        .iter()
        .position(|spec| spec.id == id)
        .unwrap_or(SECTIONS.len() - 1)
}

impl Entry {
    fn is_long_prose_change(&self) -> bool {
        // A change to stats or another identifiable game mechanic stays in its section,
        // even if the same record also includes a lengthy description.
        let values = self.before.iter().chain(&self.after);
        values.clone().all(|(k, _)| {
            matches!(
                k.as_str(),
                "text" | "description" | "detail" | "note" | "body" | "en" | "zh" | "ko"
            )
        }) && values
            .filter_map(|(_, v)| v.as_str())
            .any(|text| text.chars().take(161).count() > 160)
    }

    pub(super) fn name_field(&self, field: &str) -> bool {
        matches!(field, "name_en" | "name_zh" | "name_ko" | "result_name_ko")
            || (self.table == "quests" && field == "title")
    }

    fn row(&self, lang: &str) -> GameUpdateRow {
        let (kind, zh, en) = self.kind.labels();
        let mixed = self.kind == Kind::Renamed
            && self
                .before
                .keys()
                .chain(self.after.keys())
                .any(|k| !self.name_field(k));
        let status = if mixed {
            db::ui_text_ref(lang, "改名＋修改", "Renamed and changed")
        } else {
            db::ui_text_ref(lang, zh, en)
        };
        let name = self.name.display(lang);
        let subtitle = format!("{} · {}", table_label(&self.table, lang), self.key);
        let rename = self.rename(lang, mixed);
        let category = SECTIONS
            .iter()
            .find(|s| s.id == self.category)
            .map(|s| db::ui_text_ref(lang, s.zh, s.en))
            .unwrap_or_else(|| db::ui_text_ref(lang, "其他", "Other"));
        GameUpdateRow {
            title: if name.is_empty() {
                self.key.as_str().into()
            } else {
                name.into()
            },
            subtitle: subtitle.into(),
            category_id: self.category.as_str().into(),
            category: category.into(),
            name_changes: ModelRc::new(VecModel::from(rename.changes)),
            rename_label: rename.label.into(),
            name_note: rename.note.into(),
            location: drops::locations(&self.regions, lang).into(),
            fields: self.fields(lang),
            detail: format!("{} · {}", self.table, self.key).into(),
            kind: kind.into(),
            status: status.into(),
            ..Default::default()
        }
    }

    fn searchable(&self, lang: &str, section_name: &str) -> String {
        let (_, zh, en) = self.kind.labels();
        let mut s = format!(
            "{} {} {} {} {} {} ",
            self.table,
            self.key,
            table_label(&self.table, lang),
            section_name,
            self.category,
            db::ui_text_ref(lang, zh, en)
        );
        for name in std::iter::once(&self.name)
            .chain(&self.previous_name)
            .chain(self.regions.iter().map(|r| &r.name))
        {
            let _ = write!(s, "{} {} {} ", name.zh, name.en, name.ko);
        }
        for (key, value) in self.before.iter().chain(&self.after) {
            let _ = write!(
                s,
                "{} {} {} ",
                key,
                field_label(key, lang),
                value_text(key, Some(value), lang)
            );
        }
        s.push_str(&drops::search_text(self, lang));
        db::search_punctuation(&s.to_lowercase()).into_owned()
    }
}

struct Record {
    entry: Entry,
    visual: bool,
    subgroup_terms: String,
    display: OnceCell<GameUpdateRow>,
    search: OnceCell<String>,
}
struct Subgroup {
    role: &'static str,
    id: String,
    title: String,
    rows: Vec<usize>,
}

fn build_subgroups(section: &str, rows: &[usize], records: &[Record], lang: &str) -> Vec<Subgroup> {
    if rows.len() < 16 && !matches!(section, "new-monsters" | "monsters") {
        return Vec::new();
    }
    let mut groups = BTreeMap::<String, Subgroup>::new();
    let mut add = |id: String, title: String, role, index| {
        groups
            .entry(id.clone())
            .or_insert_with(|| Subgroup {
                id: format!("{section}:{id}"),
                title,
                role,
                rows: Vec::new(),
            })
            .rows
            .push(index);
    };
    for &index in rows {
        let entry = &records[index].entry;
        match section {
            "new-monsters" => {
                if entry.regions.is_empty() {
                    add(
                        "unknown".into(),
                        db::ui_text(lang, "地區未記錄", "Region not recorded"),
                        "region",
                        index,
                    );
                }
                for place in &entry.regions {
                    add(
                        place.id.clone(),
                        drops::locations(std::slice::from_ref(place), lang),
                        "region",
                        index,
                    );
                }
            }
            "monsters" => {
                if let Some(monster) = drops::monster(entry, lang) {
                    add(monster.code, monster.title, "monster", index);
                } else {
                    add(
                        entry.table.clone(),
                        table_label(&entry.table, lang).into(),
                        "group",
                        index,
                    );
                }
            }
            "renamed" => {
                let (id, title) = entry.rename_group(lang);
                add(id, title, "group", index);
            }
            "new-items" => {
                let kind = entry
                    .after
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                add(
                    kind.into(),
                    db::item_category_label(kind, lang),
                    "group",
                    index,
                );
            }
            _ => {
                let mut id = entry.table.clone();
                let mut title = table_label(&entry.table, lang).to_string();
                match entry.table.as_str() {
                    "strings" => {
                        let mut key = entry.key.split(" / ");
                        let source = key.next().unwrap_or_default();
                        let language = key.last().unwrap_or_default().to_uppercase();
                        id = format!("strings:{source}:{language}");
                        title = format!("{source} · {language}");
                    }
                    // Main-quest text and quest records can each span hundreds of IDs.
                    "quests" | "quest_lines" => {
                        let mut key = entry.key.split(" / ");
                        let source = key.next().unwrap_or_default();
                        if let Some(code) = key.next().and_then(|s| s.parse::<u32>().ok()) {
                            let lower = code / 100 * 100;
                            id = format!("{id}:{source}:{lower}");
                            title = format!(
                                "{title} · {} · {lower}–{}",
                                db::source_label(source, lang),
                                lower + 99
                            );
                        }
                    }
                    _ => {}
                }
                add(id, title, "group", index);
            }
        }
    }
    if groups.len() == 1 && !matches!(section, "new-monsters" | "monsters") {
        return Vec::new();
    }
    let mut groups: Vec<_> = groups.into_values().collect();
    groups.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| a.id.cmp(&b.id)));
    if section == "monsters" {
        for group in &mut groups {
            group
                .rows
                .sort_by_key(|&i| records[i].entry.table != "monster_drops");
        }
    }
    groups
}
struct Group {
    section: &'static Section,
    id: String,
    title: String,
    rows: Vec<usize>,
    subgroups: Vec<Subgroup>,
}
pub(super) struct Feed {
    pub(super) versions: slint::SharedString,
    lang: String,
    records: Vec<Record>,
    groups: Vec<Group>,
}
#[derive(Clone)]
enum Content {
    Header {
        id: String,
        title: String,
        category: &'static str,
        count: usize,
        role: &'static str,
        monster: Option<usize>,
        expanded: bool,
    },
    Record(usize),
}
impl Content {
    fn line(&self, feed: &Feed) -> (&'static str, i32) {
        match self {
            Self::Header { role, .. } => (
                role,
                match *role {
                    "section" => 42,
                    "monster" => 56,
                    _ => 36,
                },
            ),
            Self::Record(i) => {
                let entry = &feed.records[*i].entry;
                if entry.table == "monster_drops" {
                    ("drop", 54 * drops::line_count(entry).max(1) as i32)
                } else if entry.table == "monsters" && entry.kind == Kind::Added {
                    ("new-monster", 108)
                } else if entry.kind == Kind::Renamed {
                    (
                        "rename",
                        72 + 54 * entry.rename_count().saturating_sub(1) as i32,
                    )
                } else {
                    ("record", 54)
                }
            }
        }
    }
}
pub(super) struct Published {
    contents: Rc<VecModel<(usize, Content)>>,
    lines: Vec<(&'static str, i32)>,
    pub(super) summary: String,
}

impl Feed {
    pub(super) fn new(report: Report, lang: &str) -> Self {
        let mut rows: Vec<Vec<usize>> = (0..SECTIONS.len()).map(|_| vec![]).collect();
        let mut records = report
            .entries
            .into_iter()
            .enumerate()
            .map(|(i, entry)| {
                let section = section_for(&entry);
                rows[section].push(i);
                Record {
                    entry,
                    subgroup_terms: String::new(),
                    visual: !matches!(SECTIONS[section].id, "extra" | "patches"),
                    search: OnceCell::new(),
                    display: OnceCell::new(),
                }
            })
            .collect::<Vec<_>>();
        let groups = SECTIONS
            .iter()
            .zip(rows)
            .filter(|(_, rows)| !rows.is_empty())
            .map(|(spec, mut rows)| {
                rows.sort_by_key(|&i| records[i].entry.kind as u8);
                let subgroups = build_subgroups(spec.id, &rows, &records, lang);
                for group in &subgroups {
                    let label = db::search_punctuation(&group.title.to_lowercase()).into_owned();
                    for &index in &group.rows {
                        records[index].subgroup_terms.push_str(&label);
                        records[index].subgroup_terms.push(' ');
                    }
                }
                Group {
                    section: spec,
                    id: format!("section:{}", spec.id),
                    title: db::ui_text(lang, spec.zh, spec.en),
                    rows,
                    subgroups,
                }
            })
            .collect();
        Self {
            versions: format!("{} → {}", report.from_version, report.to_version).into(),
            lang: lang.to_owned(),
            records,
            groups,
        }
    }

    pub(super) fn default_expanded(&self) -> HashSet<String> {
        let Some(group) = self.groups.iter().find(|g| g.section.id != "extra") else {
            return HashSet::new();
        };
        std::iter::once(group.id.clone())
            .chain(
                group
                    .subgroups
                    .iter()
                    .filter(|r| r.role == "region")
                    .map(|r| r.id.clone()),
            )
            .collect()
    }

    pub(super) fn publish(
        &self,
        show_extras: bool,
        query: &str,
        toggles: &HashSet<String>,
    ) -> Published {
        let folded = query.trim().to_lowercase();
        let query = db::search_punctuation(&folded);
        let searching = !query.is_empty();
        let is_open = |id: &str| {
            if searching {
                !toggles.contains(id)
            } else {
                toggles.contains(id)
            }
        };
        let mut contents = Vec::new();
        let mut counts = [0usize; 4];
        let mut hidden = 0;
        for group in &self.groups {
            if group.section.id == "extra" && !show_extras {
                hidden += group.rows.len();
                continue;
            }
            let matches = |i: &usize| {
                !searching
                    || self.records[*i].subgroup_terms.contains(query.as_ref())
                    || self.records[*i]
                        .search
                        .get_or_init(|| self.records[*i].entry.searchable(&self.lang, &group.title))
                        .contains(query.as_ref())
            };
            let matched = group.rows.iter().filter(|i| matches(i));
            let mut count = 0;
            for &i in matched.clone() {
                counts[self.records[i].entry.kind as usize] += 1;
                count += 1;
            }
            if count == 0 {
                continue;
            }
            let expanded = is_open(&group.id);
            contents.push(Content::Header {
                id: group.id.clone(),
                title: group.title.clone(),
                category: group.section.category,
                count,
                role: "section",
                monster: None,
                expanded,
            });
            if !expanded {
                continue;
            }
            if group.subgroups.is_empty() {
                contents.extend(matched.copied().map(Content::Record));
            } else {
                for region in &group.subgroups {
                    let rows = region.rows.iter().filter(|i| matches(i));
                    let count = rows.clone().count();
                    if count == 0 {
                        continue;
                    }
                    let expanded = is_open(&region.id);
                    contents.push(Content::Header {
                        id: region.id.clone(),
                        title: region.title.clone(),
                        category: group.section.category,
                        count,
                        role: region.role,
                        monster: (region.role == "monster").then(|| region.rows[0]),
                        expanded,
                    });
                    if expanded {
                        contents.extend(rows.copied().map(Content::Record));
                    }
                }
            }
        }
        let mut summary = [Kind::Added, Kind::Modified, Kind::Renamed, Kind::Removed]
            .iter()
            .map(|kind| {
                let (_, zh, en) = kind.labels();
                format!(
                    "{} {}",
                    db::ui_text_ref(&self.lang, zh, en),
                    counts[*kind as usize]
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        if hidden > 0 && !searching {
            let _ = write!(
                summary,
                " · {} {}",
                db::ui_text_ref(&self.lang, "已隱藏", "Hidden"),
                hidden
            );
        }
        if contents.is_empty() {
            summary = if searching {
                db::ui_text(&self.lang, "沒有符合的更新內容", "No matching changes")
            } else if hidden > 0 {
                format!(
                    "{} {}",
                    db::ui_text_ref(&self.lang, "已隱藏", "Hidden"),
                    hidden
                )
            } else {
                db::ui_text(
                    &self.lang,
                    "這兩個版本之間沒有更新內容。",
                    "No updates between these versions.",
                )
            };
        }
        let lines = contents.iter().map(|c| c.line(self)).collect();
        Published {
            contents: Rc::new(VecModel::from(
                contents.into_iter().enumerate().collect::<Vec<_>>(),
            )),
            lines,
            summary,
        }
    }
}

fn apply_subject(
    row: &mut GameUpdateRow,
    art: &Artwork,
    subject: &super::artwork::Subject,
    position: usize,
) {
    row.route_key = art.route(subject).unwrap_or_default().into();
    if let Some(image) = art.image(subject, position) {
        row.has_icon = true;
        row.icon = image;
    }
}

impl Published {
    pub(super) fn model(
        &self,
        feed: Rc<Feed>,
        artwork: Option<Rc<Artwork>>,
    ) -> ModelRc<GameUpdateFeedRow> {
        // MapModel forwards row notifications from the source. A decoded icon
        // refreshes the affected record without moving the user's scroll position.
        ModelRc::new(
            self.contents
                .clone()
                .map(move |(position, content)| match content {
                    Content::Header {
                        id,
                        title,
                        category,
                        count,
                        role,
                        monster,
                        expanded,
                    } => {
                        let mut row = GameUpdateRow {
                            category_id: category.into(),
                            ..Default::default()
                        };
                        if let Some(m) =
                            monster.and_then(|i| drops::monster(&feed.records[i].entry, &feed.lang))
                        {
                            row.location = m.location.into();
                            if let Some(art) = artwork.as_ref() {
                                let subject = super::artwork::Subject::Monster(m.code);
                                apply_subject(&mut row, art, &subject, position);
                            }
                        }
                        GameUpdateFeedRow {
                            id: id.into(),
                            title: title.into(),
                            count: count as i32,
                            section: role == "section",
                            region: role != "section",
                            expanded,
                            record: row,
                        }
                    }
                    Content::Record(i) => {
                        let record = &feed.records[i];
                        let mut row = record
                            .display
                            .get_or_init(|| record.entry.row(&feed.lang))
                            .clone();
                        if record.entry.table == "monster_drops" {
                            row.drops = ModelRc::new(VecModel::from(drops::items(
                                &record.entry,
                                &feed.lang,
                                artwork.as_deref(),
                                position,
                            )));
                        } else if let (Some(art), Some(subject)) = (
                            artwork.as_ref().filter(|_| record.visual),
                            record.entry.subject(),
                        ) {
                            apply_subject(&mut row, art, &subject, position);
                        }
                        if !record.entry.loot.is_empty() {
                            let items = record
                                .entry
                                .loot
                                .iter()
                                .map(|slot| {
                                    drops::loot_item(slot, &feed.lang, artwork.as_deref(), position)
                                })
                                .collect::<Vec<_>>();
                            row.drops = ModelRc::new(VecModel::from(items));
                        }
                        GameUpdateFeedRow {
                            id: format!("record:{}:{}", record.entry.table, record.entry.key)
                                .into(),
                            title: row.title.clone(),
                            record: row,
                            ..Default::default()
                        }
                    }
                }),
        )
    }

    pub(super) fn refresh_icons(&self, updated: &HashSet<usize>) {
        for &position in updated {
            if let Some(current) = self.contents.row_data(position) {
                self.contents.set_row_data(position, current);
            }
        }
    }

    pub(super) fn layout(&self) -> Layout {
        let mut top = 0;
        let lines = self
            .lines
            .iter()
            .enumerate()
            .map(|(index, &(role, height))| {
                let line = GameUpdateLine {
                    start: index as i32,
                    count: 1,
                    top,
                    height,
                    role: role.into(),
                };
                top += height;
                line
            })
            .collect();
        Layout { lines }
    }
}

#[derive(Default)]
pub(super) struct Layout {
    pub(super) lines: Vec<GameUpdateLine>,
}
impl Layout {
    pub(super) fn at_y(&self, y: i32) -> i32 {
        self.lines
            .partition_point(|line| line.top <= y)
            .saturating_sub(1) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn entry(table: &str, category: &str, before: Value, after: Value) -> Entry {
        serde_json::from_value(json!({
            "table":table,"category":category,"key":"42","kind":"modified","name":{},
            "before":before,"after":after,
        }))
        .unwrap()
    }

    #[test]
    fn hidden_prose_keeps_a_simultaneous_gameplay_change_visible() {
        let long = "遊戲文字".repeat(55);
        let text_only = entry(
            "items",
            "items",
            json!({"description":""}),
            json!({"description":long}),
        );
        assert_eq!(SECTIONS[section_for(&text_only)].id, "extra");
        let with_stat = entry(
            "items",
            "items",
            json!({"description":"","price":4}),
            json!({"description":long,"price":5}),
        );
        assert_eq!(SECTIONS[section_for(&with_stat)].id, "items");
        let dialogue = entry(
            "quest_lines",
            "quests",
            json!({"subject":"1"}),
            json!({"subject":"2"}),
        );
        assert_eq!(SECTIONS[section_for(&dialogue)].id, "extra");
    }

    #[test]
    fn rename_exposes_the_changed_language_even_if_selected_name_is_identical() {
        let mut changed = entry(
            "items",
            "items",
            json!({"name_en":"Ligh","price":1}),
            json!({"name_en":"Light","price":2}),
        );
        changed.kind = Kind::Renamed;
        changed.name = serde_json::from_value(json!({"zh":"光","en":"Light"})).unwrap();
        changed.previous_name =
            Some(serde_json::from_value(json!({"zh":"光","en":"Ligh"})).unwrap());
        let row = changed.row("zh");
        let change = row.name_changes.row_data(0).unwrap();
        assert_eq!(
            (
                change.before.as_str(),
                change.after.as_str(),
                change.label.as_str()
            ),
            ("Ligh", "Light", "英文")
        );
        assert!(row.name_note.contains("繁中名稱未變"));
        assert!(row.name_note.contains("兩版均未提供韓文譯名"));
        assert_eq!(row.status.as_str(), "改名＋修改");
        assert_eq!(row.fields.row_data(0).unwrap().label.as_str(), "英文名稱");
        assert_eq!(SECTIONS[section_for(&changed)].id, "renamed");
        assert!(changed.searchable("zh", "改名").contains("ligh "));
    }

    #[test]
    fn each_record_occupies_its_own_line_and_search_skips_record_details() {
        let items = (0..5)
            .map(|i| {
                let mut e = entry("items", "items", json!({}), json!({"price":i}));
                e.key = i.to_string();
                e.kind = Kind::Added;
                e
            })
            .collect();
        let feed = Feed::new(
            Report {
                from_version: "1".into(),
                to_version: "2".into(),
                entries: items,
            },
            "zh",
        );
        let published = feed.publish(false, "", &feed.default_expanded());
        let lines = published.layout();
        let shape = lines
            .lines
            .iter()
            .map(|line| (line.role.as_str(), line.start, line.count))
            .collect::<Vec<_>>();
        assert_eq!(shape.len(), 6);
        assert_eq!(shape[0], ("section", 0, 1));
        for (index, row) in shape.iter().enumerate().skip(1) {
            assert_eq!(*row, ("record", index as i32, 1));
        }
        assert_eq!(lines.at_y(42 + 54), 2);
        feed.publish(false, "price", &HashSet::new());
        assert!(feed
            .records
            .iter()
            .all(|record| record.display.get().is_none()));
    }
}
