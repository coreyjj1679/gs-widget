//! User-facing browse groups use NPC identities, actual NPC maps, or connected
//! quest components. Every grouping partitions all quests, including missing data.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::Connection;

use super::npc::npc_names;
use super::text::{chrome_text, lead3, trail3};
use super::{Catalog, Hit};

#[derive(Default)]
struct GroupMembers {
    key: String,
    ids: BTreeSet<i64>,
    roots: Vec<i64>,
    unresolved: usize,
    cyclic: bool,
}

struct MapName {
    en: String,
    zh: String,
    ko: String,
}

impl MapName {
    fn title(&self, lang: &str) -> String {
        let title = lead3(lang, &self.en, &self.zh, &self.ko);
        if title.is_empty() {
            self.ko.clone()
        } else {
            title
        }
    }
}

fn reliable_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty()
        || matches!(
            name.to_lowercase().as_str(),
            "0" | "-"
                | "?"
                | "unknown"
                | "none"
                | "null"
                | "main"
                | "main map"
                | "未知"
                | "不明"
                | "主要"
                | "主地圖"
                | "未設定"
                | "메인맵"
                | "미정"
        )
    {
        String::new()
    } else {
        name.to_string()
    }
}

fn load_map_names(conn: &Connection) -> HashMap<i64, MapName> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT m.code, m.name_ko, IFNULL(en.text, ''), IFNULL(zh.text, '')
         FROM maps m
         LEFT JOIN strings en ON en.object = 'OnlineText.dat' AND en.lang = 'en' AND en.id = printf('%08d', m.name_gts)
         LEFT JOIN strings zh ON zh.object = 'OnlineText.dat' AND zh.lang = 'zh' AND zh.id = printf('%08d', m.name_gts)
         WHERE m.code > 0 ORDER BY m.code",
    ) else { return HashMap::new() };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    }) else {
        return HashMap::new();
    };
    rows.filter_map(Result::ok)
        .filter_map(|(id, ko, en, zh)| {
            let name = MapName {
                ko: reliable_name(&ko),
                en: reliable_name(&en),
                zh: reliable_name(&zh),
            };
            (!name.ko.is_empty() || !name.en.is_empty() || !name.zh.is_empty())
                .then_some((id, name))
        })
        .collect()
}

fn load_levels(conn: &Connection) -> HashMap<i64, i64> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT quest_id, qty FROM quest_steps WHERE lower(role) = 'requirement' AND lower(kind) = 'level'",
    ) else { return HashMap::new() };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    }) else {
        return HashMap::new();
    };
    let mut levels = HashMap::new();
    for (id, quantity) in rows.filter_map(Result::ok) {
        if let Some(level) = quantity.parse::<i64>().ok().filter(|level| *level > 0) {
            let known = levels.entry(id).or_insert(level);
            *known = (*known).max(level);
        }
    }
    levels
}

fn level_summary(ids: &BTreeSet<i64>, levels: &HashMap<i64, i64>, lang: &str) -> String {
    let known: Vec<_> = ids
        .iter()
        .filter_map(|id| levels.get(id))
        .copied()
        .collect();
    let Some(minimum) = known.iter().min() else {
        return chrome_text(lang, "等級未註明", "Level not recorded");
    };
    let maximum = known.iter().max().unwrap();
    let span = if minimum == maximum {
        minimum.to_string()
    } else {
        format!("{minimum}–{maximum}")
    };
    let mut text = format!("{} {span}", chrome_text(lang, "需求等級", "Required level"));
    if known.len() < ids.len() {
        text.push_str(&format!(
            " · {} {}",
            ids.len() - known.len(),
            chrome_text(lang, "筆未註等級", "without a recorded level")
        ));
    }
    text
}

fn split_group(key: &str) -> Option<(&str, &str)> {
    let (grouping, identity) = key.strip_prefix("qgroup:")?.split_once(':')?;
    (matches!(grouping, "npc" | "region" | "chain") && !identity.is_empty())
        .then_some((grouping, identity))
}

impl Catalog {
    fn quest_group_memberships(
        &self,
        grouping: &str,
        maps: &HashMap<i64, MapName>,
    ) -> Vec<GroupMembers> {
        if grouping == "chain" {
            let mut groups = Vec::new();
            let mut standalone = GroupMembers {
                key: "qgroup:chain:standalone".into(),
                ..GroupMembers::default()
            };
            for component in self.quest_dependency_components() {
                if component.quest_ids.len() == 1 && component.edge_count == 0 {
                    standalone.ids.extend(component.quest_ids);
                    standalone.unresolved += component.unresolved_count;
                } else {
                    groups.push(GroupMembers {
                        key: format!("qgroup:chain:{}", component.canonical_id),
                        ids: component.quest_ids.into_iter().collect(),
                        roots: component.root_ids,
                        unresolved: component.unresolved_count,
                        cyclic: component.cyclic,
                    });
                }
            }
            if !standalone.ids.is_empty() {
                groups.push(standalone);
            }
            return groups;
        }
        if !matches!(grouping, "npc" | "region") {
            return Vec::new();
        }
        let npcs: HashMap<_, _> = self.index.npcs.iter().map(|npc| (npc.code, npc)).collect();
        let mut grouped: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
        for quest in &self.index.quests {
            let npc = npcs.get(&quest.npc).filter(|npc| npc.code > 0);
            let identity = match grouping {
                "npc" => npc.map(|npc| npc.code),
                _ => npc
                    .map(|npc| npc.map_index)
                    .filter(|map| *map > 0 && maps.contains_key(map)),
            };
            let key = format!(
                "qgroup:{grouping}:{}",
                identity
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "unknown".into())
            );
            grouped.entry(key).or_default().insert(quest.id);
        }
        grouped
            .into_iter()
            .map(|(key, ids)| GroupMembers {
                key,
                ids,
                ..GroupMembers::default()
            })
            .collect()
    }

    /// A partition of all quests, with missing NPC/map metadata kept explicitly.
    pub fn quest_browse_groups(&self, grouping: &str, lang: &str) -> Vec<Hit> {
        let maps = load_map_names(&self.conn);
        let levels = load_levels(&self.conn);
        let npcs: HashMap<_, _> = self.index.npcs.iter().map(|npc| (npc.code, npc)).collect();
        let quests: HashMap<_, _> = self
            .index
            .quests
            .iter()
            .map(|quest| (quest.id, quest))
            .collect();
        let groups = self.quest_group_memberships(grouping, &maps);
        let mut hits = Vec::new();
        for group in groups {
            let Some((_, identity)) = split_group(&group.key) else {
                continue;
            };
            let mut parts = Vec::new();
            let mut icon = None;
            let title = match grouping {
                "npc" => {
                    if let Some(npc) = identity.parse::<i64>().ok().and_then(|id| npcs.get(&id)) {
                        let (name, alternate) = npc_names(lang, npc);
                        if !alternate.is_empty() {
                            parts.push(alternate);
                        }
                        icon = npc.icon.clone();
                        parts.push(
                            maps.get(&npc.map_index)
                                .map(|map| map.title(lang))
                                .unwrap_or_else(|| {
                                    chrome_text(lang, "地區未註明", "Region not recorded")
                                }),
                        );
                        parts.push(format!("NPC #{}", npc.code));
                        name
                    } else {
                        parts.push(chrome_text(
                            lang,
                            "資料未記錄發放任務的 NPC",
                            "The quest-giving NPC is not recorded",
                        ));
                        chrome_text(lang, "NPC 未註明", "Unknown NPC")
                    }
                }
                "region" => {
                    if let Some(map) = identity.parse::<i64>().ok().and_then(|id| maps.get(&id)) {
                        let alternate = trail3(lang, &map.en, &map.zh, &map.ko);
                        if !alternate.is_empty() {
                            parts.push(alternate);
                        }
                        parts.push(format!(
                            "{} NPC",
                            group
                                .ids
                                .iter()
                                .filter_map(|id| quests.get(id))
                                .map(|quest| quest.npc)
                                .filter(|npc| npcs.contains_key(npc))
                                .collect::<HashSet<_>>()
                                .len()
                        ));
                        parts.push(format!("{} #{identity}", chrome_text(lang, "地圖", "Map")));
                        map.title(lang)
                    } else {
                        parts.push(chrome_text(
                            lang,
                            "缺少 NPC／地圖資料，或只記錄通用主地圖",
                            "NPC/map data is absent or only identifies the generic main map",
                        ));
                        chrome_text(lang, "地區未註明", "Unknown region")
                    }
                }
                "chain" if identity == "standalone" => {
                    parts.push(chrome_text(
                        lang,
                        "資料未記錄可連接的前置或後續任務",
                        "No connected prerequisite or follow-up is recorded",
                    ));
                    chrome_text(lang, "未連接的任務", "Unconnected quests")
                }
                "chain" => {
                    let canonical = identity.parse::<i64>().ok();
                    let root = canonical.and_then(|id| quests.get(&id));
                    if let Some(root) = root {
                        let alternate = trail3(lang, &root.title_en, &root.title_zh, &root.title_ko);
                        if !alternate.is_empty() {
                            parts.push(alternate);
                        }
                    }
                    let known_npcs: BTreeSet<_> = group
                        .ids
                        .iter()
                        .filter_map(|id| quests.get(id))
                        .map(|quest| quest.npc)
                        .filter(|npc| *npc > 0 && npcs.contains_key(npc))
                        .collect();
                    if let Some(root_npc) = root.and_then(|quest| npcs.get(&quest.npc)) {
                        parts.push(npc_names(lang, root_npc).0);
                        icon = root_npc.icon.clone();
                    }
                    if known_npcs.len() > 1 {
                        parts.push(format!("{} NPC", known_npcs.len()));
                    }
                    if group.roots.len() > 1 {
                        parts.push(format!(
                            "{} {}",
                            group.roots.len(),
                            chrome_text(lang, "個起點", "starting quests")
                        ));
                    }
                    if let Some(root) = root {
                        parts.push(format!(
                            "{} {}:{}",
                            if group.roots.is_empty() {
                                chrome_text(lang, "參照", "Reference")
                            } else {
                                chrome_text(lang, "起點", "Starts at")
                            },
                            root.source,
                            root.code
                        ));
                        let name = lead3(lang, &root.title_en, &root.title_zh, &root.title_ko);
                        if name.is_empty() {
                            root.code.clone()
                        } else {
                            name
                        }
                    } else {
                        format!("{} {identity}", chrome_text(lang, "任務鏈", "Quest chain"))
                    }
                }
                _ => continue,
            };
            parts.push(level_summary(&group.ids, &levels, lang));
            if group.unresolved > 0 {
                parts.push(format!(
                    "{} {}",
                    group.unresolved,
                    chrome_text(lang, "個未解析參照", "unresolved references")
                ));
            }
            if group.cyclic {
                parts.push(chrome_text(
                    lang,
                    "包含循環依賴",
                    "Contains a dependency cycle",
                ));
            }
            hits.push(Hit {
                title,
                subtitle: parts.join(" · "),
                key: group.key,
                icon,
                solo: false,
                badge: None,
                aside: group.ids.len().to_string(),
                chip: String::new(),
            });
        }
        // Actual map/NPC identities remain distinct even when translated names
        // coincide (for example maps307 and327); make that distinction visible.
        let mut counts = HashMap::new();
        for hit in &hits {
            *counts.entry(hit.title.clone()).or_insert(0usize) += 1;
        }
        for hit in &mut hits {
            if counts[&hit.title] > 1 {
                if let Some((_, id)) = split_group(&hit.key) {
                    hit.title.push_str(&format!(" · #{id}"));
                }
            }
        }
        hits.sort_by(|a, b| {
            let last = |key: &str| key.ends_with(":unknown") || key.ends_with(":standalone");
            last(&a.key)
                .cmp(&last(&b.key))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
                .then_with(|| a.key.cmp(&b.key))
        });
        hits
    }

    /// Use the existing bilingual item/reward/title search, then intersect with
    /// group membership. Search ranking and metadata matching remain identical.
    pub fn quest_browse_group_records(&self, key: &str, query: &str, lang: &str) -> Vec<Hit> {
        let Some((grouping, _)) = split_group(key) else {
            return Vec::new();
        };
        let maps = if grouping == "region" {
            load_map_names(&self.conn)
        } else {
            HashMap::new()
        };
        let Some(group) = self
            .quest_group_memberships(grouping, &maps)
            .into_iter()
            .find(|group| group.key == key)
        else {
            return Vec::new();
        };
        self.quest_category_records("qcat:all", query, lang)
            .into_iter()
            .filter(|hit| {
                hit.key
                    .strip_prefix("quest:")
                    .and_then(|id| id.parse::<i64>().ok())
                    .is_some_and(|id| group.ids.contains(&id))
            })
            .collect()
    }

    pub fn quest_browse_group_title(&self, key: &str, lang: &str) -> String {
        let Some((grouping, _)) = split_group(key) else {
            return String::new();
        };
        let Some(group) = self
            .quest_browse_groups(grouping, lang)
            .into_iter()
            .find(|group| group.key == key)
        else {
            return String::new();
        };
        let prefix = match grouping {
            "npc" => "NPC".to_string(),
            "region" => chrome_text(lang, "地區", "Region"),
            _ => chrome_text(lang, "任務鏈", "Chain"),
        };
        format!("{prefix} · {}", group.title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse_groupings_partition_all_sources_and_expose_unknown_metadata() {
        let catalog = Catalog::open().expect("bundled database");
        let all: BTreeSet<_> = catalog.index.quests.iter().map(|quest| quest.id).collect();
        let maps = load_map_names(&catalog.conn);
        for (grouping, expected_groups) in [("npc", 168), ("region", 85), ("chain", 92)] {
            let memberships = catalog.quest_group_memberships(grouping, &maps);
            assert_eq!(memberships.len(), expected_groups, "{grouping}");
            let mut seen = BTreeSet::new();
            for group in memberships {
                for id in group.ids {
                    assert!(
                        seen.insert(id),
                        "{grouping} assigns quest{id} more than once"
                    );
                }
            }
            assert_eq!(seen, all, "{grouping} must include every source");
            assert_eq!(
                catalog
                    .quest_browse_groups(grouping, "en")
                    .iter()
                    .map(|group| group.aside.parse::<usize>().unwrap())
                    .sum::<usize>(),
                all.len()
            );
        }
        for (grouping, key, expected) in [
            ("npc", "qgroup:npc:unknown", 121),
            ("region", "qgroup:region:unknown", 252),
            ("chain", "qgroup:chain:standalone", 125),
        ] {
            let groups = catalog.quest_browse_groups(grouping, "en");
            assert_eq!(groups.last().unwrap().key, key);
            assert_eq!(groups.last().unwrap().aside, expected.to_string());
        }
    }

    #[test]
    fn chain_groups_keep_cross_source_members_and_stable_roots() {
        let catalog = Catalog::open().expect("bundled database");
        let memberships = catalog.quest_group_memberships("chain", &HashMap::new());
        let main = catalog.quest_id_by_code("main", "71562").unwrap();
        let intro = catalog.quest_id_by_code("intro", "3").unwrap();
        let group = memberships
            .iter()
            .find(|group| group.ids.contains(&main))
            .unwrap();
        assert!(group.ids.contains(&intro));
        let canonical = group
            .key
            .rsplit(':')
            .next()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        assert_eq!(canonical, *group.roots.iter().min().unwrap());
        let wide = memberships
            .iter()
            .find(|group| group.key == "qgroup:chain:621")
            .unwrap();
        assert_eq!(wide.ids.len(), 40);
        assert!(wide.ids.contains(&669));
        assert!(catalog
            .quest_browse_group_title(&wide.key, "en")
            .starts_with("Chain · "));
    }

    #[test]
    fn group_search_is_exactly_the_all_quest_search_intersection() {
        let catalog = Catalog::open().expect("bundled database");
        let maps = load_map_names(&catalog.conn);
        for grouping in ["npc", "region", "chain"] {
            let groups = catalog.quest_group_memberships(grouping, &maps);
            let samples: Vec<_> = groups
                .iter()
                .filter(|group| {
                    group.ids.contains(&621)
                        || group.key.ends_with(":unknown")
                        || group.key.ends_with(":standalone")
                })
                .collect();
            for group in samples {
                for query in ["", "72062", "鹿角", "Deer Antler"] {
                    let expected: Vec<_> = catalog
                        .quest_category_records("qcat:all", query, "en")
                        .into_iter()
                        .filter(|hit| {
                            hit.key
                                .strip_prefix("quest:")
                                .and_then(|id| id.parse::<i64>().ok())
                                .is_some_and(|id| group.ids.contains(&id))
                        })
                        .map(|hit| hit.key)
                        .collect();
                    let actual: Vec<_> = catalog
                        .quest_browse_group_records(&group.key, query, "en")
                        .into_iter()
                        .map(|hit| hit.key)
                        .collect();
                    assert_eq!(actual, expected, "{} / {query}", group.key);
                }
            }
        }
        assert!(!catalog
            .quest_browse_group_records("qgroup:chain:621", "72062", "en")
            .is_empty());
        assert!(catalog
            .quest_browse_group_records("qgroup:invalid:621", "", "en")
            .is_empty());
    }

    #[test]
    fn real_map_ids_remain_separate_and_map_zero_is_unknown() {
        let catalog = Catalog::open().expect("bundled database");
        let groups = catalog.quest_browse_groups("region", "en");
        let first = groups
            .iter()
            .find(|group| group.key == "qgroup:region:307")
            .unwrap();
        let second = groups
            .iter()
            .find(|group| group.key == "qgroup:region:327")
            .unwrap();
        assert_ne!(first.title, second.title);
        assert!(first.title.contains("#307") && second.title.contains("#327"));
        assert!(!groups.iter().any(|group| group.key == "qgroup:region:0"));
        assert_eq!(reliable_name("Main"), "");
        assert_eq!(reliable_name("Temporal Vortex"), "Temporal Vortex");
    }

    #[test]
    fn directory_subtitles_preserve_alternate_names_for_bilingual_search() {
        let catalog = Catalog::open().expect("bundled database");
        for grouping in ["npc", "region", "chain"] {
            let english = catalog.quest_browse_groups(grouping, "en-zh");
            let chinese: HashMap<_, _> = catalog
                .quest_browse_groups(grouping, "zh-en")
                .into_iter()
                .map(|group| (group.key.clone(), group))
                .collect();
            let mut checked = 0;
            for en in &english {
                if en.key.ends_with(":unknown") || en.key.ends_with(":standalone") {
                    continue;
                }
                let zh = &chinese[&en.key];
                // Duplicate-name suffixes identify the actual NPC/map/root and
                // need not be repeated in the alternate-name subtitle.
                let en_name = en.title.split(" · #").next().unwrap();
                let zh_name = zh.title.split(" · #").next().unwrap();
                if en_name != zh_name {
                    assert!(
                        zh.subtitle.starts_with(en_name),
                        "{} lost its English name",
                        en.key
                    );
                    assert!(
                        en.subtitle.starts_with(zh_name),
                        "{} lost its Chinese name",
                        en.key
                    );
                    checked += 1;
                }
            }
            assert!(checked > 0, "{grouping} must exercise bilingual names");
        }
    }
}
