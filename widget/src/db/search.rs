use super::text::{lead3, trail3};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::alias::{alias_codes, set_query};
use super::item::{item_category_order, item_category_pair, pref_rank, set_kind_label, set_kind_rank, set_piece_label, set_title_lang, weapon_family_pair};
use super::Hit;
use super::names::{fuzzy_rows, haystack, name_match, plain_piece, text_hit};
use super::quest::{main_type_name, quest_source_rank};
use super::rows::{HeroRow, Index, ItemRow, MonsterRow, NpcRow, PlaceRow, QuestRow, RecipeRow, SetRow};
use super::text::{caption, commas, label, lead, nation_pair, phrase, source_names, trail};

/// Records ranked at or below this are direct matches; the rest are typo suggestions.
const DIRECT_MATCH: u8 = 3;
/// Typo suggestions only fill a short result list; direct matches are never dropped.
const TYPO_SUGGESTIONS: usize = 40;

pub(in crate::db) struct ListedPlace {
    pub(in crate::db) count: i64,
    pub(in crate::db) sort: String,
    pub(in crate::db) gts: i64,
    pub(in crate::db) hit: Hit,
}

pub(in crate::db) fn place_hit(lang: &str, en: &str, zh: &str, count: i64, key: String) -> Hit {
    Hit {
        title: lead(lang, en, zh),
        subtitle: caption(&trail(lang, en, zh), format!("{} {}", commas(count), phrase("monsters", lang))),
        key,
        icon: None,
        solo: false,
        aside: String::new(),
                badge: None,
                    chip: String::new(),
    }
}

pub(in crate::db) fn is_floor_suffix(floor: &str) -> bool {
    let floor = floor.trim();
    if floor.is_empty() {
        return false;
    }
    if floor.chars().any(|ch| matches!(ch, '層' | '樓' | '层' | '층')) {
        return true;
    }
    let lower = floor.to_lowercase();
    lower.contains("floor") || lower.contains("basement")
}

fn norm_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn standalone_names(places: &[&PlaceRow]) -> HashSet<String> {
    let mut names = HashSet::new();
    for place in places {
        for name in [&place.en, &place.zh] {
            let (base, floor) = place_split(name);
            if floor.is_empty() && !base.is_empty() {
                names.insert(norm_name(&base));
            }
        }
    }
    names
}

fn decorative(place: &PlaceRow, standalone: &HashSet<String>) -> bool {
    let source = if place.zh.trim().is_empty() { place.en.as_str() } else { place.zh.as_str() };
    let (_, floor) = place_split(source);
    if floor.is_empty() || is_floor_suffix(&floor) {
        return false;
    }
    let (_, en_floor) = place_split(&place.en);
    if is_floor_suffix(&en_floor) {
        return false;
    }
    !standalone.contains(&norm_name(&floor)) && !standalone.contains(&norm_name(&en_floor))
}

fn bucket_of(place: &PlaceRow, standalone: &HashSet<String>) -> String {
    if decorative(place, standalone) {
        return group_key(place);
    }
    let source = if place.zh.trim().is_empty() { place.en.as_str() } else { place.zh.as_str() };
    let (_, floor) = place_split(source);
    if !floor.is_empty() && standalone.contains(&norm_name(&floor)) {
        return format!("keep:{}", place.gts);
    }
    group_key(place)
}

pub(in crate::db) fn group_key(place: &PlaceRow) -> String {
    let source = if place.zh.trim().is_empty() { place.en.as_str() } else { place.zh.as_str() };
    let base = place_split(source).0;
    if base.is_empty() { format!("gts:{}", place.gts) } else { base }
}

pub(in crate::db) fn bases_of(members: &[&PlaceRow]) -> (String, String) {
    let en = majority(members.iter().map(|place| place_split(&place.en).0));
    let zh = majority(members.iter().map(|place| place_split(&place.zh).0));
    (en, zh)
}

pub(in crate::db) fn member_gts(members: &[&PlaceRow]) -> Vec<i64> {
    members.iter().map(|place| place.gts).collect()
}

pub(in crate::db) fn majority(values: impl Iterator<Item = String>) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for value in values {
        let value = value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        *counts.entry(value).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then(right.0.len().cmp(&left.0.len())))
        .map(|(value, _)| value)
        .unwrap_or_default()
}

pub(in crate::db) fn all_floor_names(lang: &str, en: &str, zh: &str) -> (String, String) {
    // Localize the generated suffix, while keeping the location's honest
    // primary fallback and never inventing a missing secondary translation.
    let primary = lead(lang, en, zh);
    let alternate = trail(lang, en, zh);
    let suffix = |language| super::ui_text(language, "所有", "All");
    let title = format!("{primary} - {}", suffix(lang));
    let alternate = if alternate.is_empty() {
        String::new()
    } else {
        format!("{alternate} - {}", suffix(super::secondary_lang(lang).unwrap_or(lang)))
    };
    (title, alternate)
}

pub(in crate::db) fn floor_sort_label(lang: &str, en: &str, zh: &str) -> String {
    let shown = lead(lang, en, zh);
    let (_, floor) = place_split(&shown);
    if floor.is_empty() { shown } else { floor }
}

pub(in crate::db) fn place_split(name: &str) -> (String, String) {
    let name = name.trim();
    if let Some(index) = name.find(is_dash) {
        let dash = name[index..].chars().next().map(|mark| mark.len_utf8()).unwrap_or(1);
        let base = name[..index].trim();
        let floor = name[index + dash..].trim();
        if !base.is_empty() && !floor.is_empty() {
            return (base.to_string(), floor.to_string());
        }
    }
    if let Some(split) = split_attached_floor(name) {
        return split;
    }
    (name.to_string(), String::new())
}

pub(in crate::db) fn is_dash(mark: char) -> bool {
    matches!(mark, '-' | '－' | '–' | '—' | '―')
}

pub(in crate::db) fn split_attached_floor(name: &str) -> Option<(String, String)> {
    let chars: Vec<char> = name.chars().collect();
    let last = *chars.last()?;
    if !matches!(last, '層' | '樓' | '层' | '층' | 'F' | 'f') {
        return None;
    }
    let mut index = chars.len() - 1;
    if index == 0 {
        return None;
    }
    index -= 1;
    if chars[index].is_whitespace() {
        if index == 0 {
            return None;
        }
        index -= 1;
    }
    if !chars[index].is_ascii_digit() {
        return None;
    }
    while index > 0 && chars[index - 1].is_ascii_digit() {
        index -= 1;
    }
    if index == 0 {
        return None;
    }
    let base: String = chars[..index].iter().collect();
    let base = base.trim().to_string();
    if base.is_empty() {
        return None;
    }
    let floor: String = chars[index..].iter().collect();
    Some((base, floor.trim().to_string()))
}

impl Index {
    /// 0 code or alias target, 1 whole name, 2 name prefix, 3 other direct match, 4 typo.
    fn search_rank(&self, code: &str, names: &str, query: &str, folded: &str) -> u8 {
        if code.eq_ignore_ascii_case(query) || (self.active_alias.get() && self.active_codes.borrow().contains(code)) {
            0
        } else if names.split('\n').any(|line| line == folded) {
            1
        } else if names.split('\n').any(|line| line.starts_with(folded)) {
            2
        } else if !folded.is_empty() && names.contains(folded) {
            DIRECT_MATCH
        } else {
            DIRECT_MATCH + 1
        }
    }

    /// Order category search results by closeness, keeping index order within each
    /// rank. Every direct match stays listed; rows that are not records keep their place.
    pub(in crate::db) fn rank_search_hits(&self, hits: Vec<Hit>, query: &str, folded: &str) -> Vec<Hit> {
        let listed = |kind: &str| hits.iter().any(|hit| hit.key.split_once(':').is_some_and(|(key, _)| key == kind));
        let monsters: HashMap<&str, &MonsterRow> = if listed("monster") {
            self.monsters.iter().map(|row| (row.code.as_str(), row)).collect()
        } else { HashMap::new() };
        let quests: HashMap<i64, &QuestRow> = if listed("quest") {
            self.quests.iter().map(|row| (row.id, row)).collect()
        } else { HashMap::new() };
        let recipes: HashMap<i64, &RecipeRow> = if listed("recipe") {
            self.recipes.iter().map(|row| (row.id, row)).collect()
        } else { HashMap::new() };
        let heroes: HashMap<&str, &HeroRow> = if listed("hero") {
            self.heroes.iter().map(|row| (row.code.as_str(), row)).collect()
        } else { HashMap::new() };
        let npcs: HashMap<i64, &NpcRow> = if listed("npc") {
            self.npcs.iter().map(|row| (row.code, row)).collect()
        } else { HashMap::new() };
        let sets: HashMap<i64, &SetRow> = if listed("set") {
            self.sets.iter().map(|row| (row.code, row)).collect()
        } else { HashMap::new() };
        let rank = |key: &str| {
            let Some((kind, id)) = key.split_once(':') else { return DIRECT_MATCH };
            let number = id.parse::<i64>().ok();
            let record = match kind {
                "item" => number.and_then(|code| self.find_item(code)).map(|row| (row.code_str.clone(), row.names.as_str())),
                "monster" => monsters.get(id).map(|row| (row.code.clone(), row.names.as_str())),
                "quest" => number.and_then(|id| quests.get(&id)).map(|row| (row.code.clone(), row.names.as_str())),
                "recipe" => number.and_then(|id| recipes.get(&id)).map(|row| (row.result_str.clone(), row.names.as_str())),
                "hero" => heroes.get(id).map(|row| (row.code.clone(), row.names.as_str())),
                "npc" => number.and_then(|code| npcs.get(&code)).map(|row| (row.code.to_string(), row.names.as_str())),
                "set" => number.and_then(|code| sets.get(&code)).map(|row| (row.code.to_string(), row.names.as_str())),
                _ => None,
            };
            record.map_or(DIRECT_MATCH, |(code, names)| self.search_rank(&code, names, query, folded))
        };
        let mut ranked: Vec<(u8, usize, Hit)> = hits.into_iter().enumerate()
            .map(|(index, hit)| (rank(&hit.key), index, hit))
            .collect();
        ranked.sort_by_key(|(rank, index, _)| (*rank, *index));
        let direct = ranked.iter().filter(|(rank, _, _)| *rank <= DIRECT_MATCH).count();
        let typo_room = TYPO_SUGGESTIONS.saturating_sub(direct);
        ranked.into_iter()
            .enumerate()
            .filter(|(position, (rank, _, _))| *rank <= DIRECT_MATCH || *position < direct + typo_room)
            .map(|(_, (_, _, hit))| hit)
            .collect()
    }

    pub(in crate::db) fn prepare_aliases(&self, folded: &str) {
        let codes = alias_codes(&self.aliases.borrow(), folded);
        self.active_alias.set(!codes.is_empty());
        *self.active_codes.borrow_mut() = codes;
    }

    pub(in crate::db) fn name_hit(&self, query: &str, folded: &str, exact: &str, names: &str) -> bool {
        if name_match(query, folded, exact, names) {
            return true;
        }
        self.active_alias.get() && self.active_codes.borrow().contains(exact)
    }

    pub(in crate::db) fn nation_hits(&self, lang: &str) -> Vec<Hit> {
        self.nations
            .iter()
            .map(|(nation, count)| {
                let (en, zh) = nation_pair(*nation);
                let ko = super::ui_text("ko", zh, en);
                Hit {
                title: lead3(lang, en, zh, &ko),
                subtitle: caption(&trail3(lang, en, zh, &ko), format!("{} {}", commas(*count), phrase("monsters", lang))),
                key: format!("nation:{nation}"),
                icon: None,
                    solo: false,
                    aside: String::new(),
                badge: None,
                    chip: String::new(),
            }
            })
            .collect()
    }

    pub(in crate::db) fn place_hits(&self, nation: &str, folded: &str, lang: &str) -> Vec<Hit> {
        let Ok(nation) = nation.parse::<i64>() else { return Vec::new() };
        let nation_places: Vec<&PlaceRow> = self.places.iter().filter(|place| place.nation == nation).collect();
        let standalone = standalone_names(&nation_places);
        let mut buckets: HashMap<String, Vec<&PlaceRow>> = HashMap::new();
        for place in nation_places {
            buckets.entry(bucket_of(place, &standalone)).or_default().push(place);
        }
        let mut listed = Vec::new();
        for (key, members) in buckets {
            let (en, zh) = bases_of(&members);
            let base_names = haystack(&[&en, &zh]);
            let matched = folded.is_empty()
                || text_hit(folded, &base_names)
                || members.iter().any(|place| text_hit(folded, &place.names));
            if !matched {
                continue;
            }
            if members.iter().all(|place| decorative(place, &standalone)) {
                let (en, zh) = if members.len() == 1 {
                    let place = members[0];
                    (place_split(&place.en).0, place_split(&place.zh).0)
                } else {
                    (en, zh)
                };
                let en = if en.is_empty() { zh.clone() } else { en };
                let zh = if zh.is_empty() { en.clone() } else { zh };
                let gts = member_gts(&members);
                let count = self.count_at(&gts);
                let sort = if zh.is_empty() { en.clone() } else { zh.clone() };
                let id = if gts.len() == 1 {
                    format!("region:{}", gts[0])
                } else {
                    format!("span:{key}")
                };
                listed.push(ListedPlace {
                    count,
                    sort,
                    gts: gts.iter().copied().min().unwrap_or(0),
                    hit: place_hit(lang, &en, &zh, count, id),
                });
                continue;
            }
            if members.len() < 2 {
                let place = members[0];
                listed.push(ListedPlace {
                    count: place.count,
                    sort: place.sort.clone(),
                    gts: place.gts,
                    hit: place_hit(lang, &place.en, &place.zh, place.count, format!("region:{}", place.gts)),
                });
                continue;
            }
            let gts = member_gts(&members);
            let count = self.count_at(&gts);
            let sort = if zh.is_empty() { en.clone() } else { zh.clone() };
            listed.push(ListedPlace {
                count,
                sort,
                gts: gts.iter().copied().min().unwrap_or(0),
                hit: place_hit(lang, &en, &zh, count, format!("area:{key}")),
            });
        }
        listed.sort_by(|a, b| b.count.cmp(&a.count).then(a.sort.cmp(&b.sort)).then(a.gts.cmp(&b.gts)));
        listed.into_iter().map(|row| row.hit).collect()
    }

    pub(in crate::db) fn directory_hits(&self, lang: &str) -> Vec<Hit> {
        let all = super::ui_text(lang, "所有怪物", "All monsters");
        let saved = super::ui_text(lang, "收藏", "Saved");
        let mut hits = vec![Hit {
            title: all.into(),
            subtitle: self.monsters.len().to_string(),
            key: "all:".into(),
            icon: None,
            solo: false,
            aside: String::new(),
            badge: None,
            chip: "label".into(),
        }, Hit {
            title: saved.into(),
            subtitle: String::new(),
            key: "saved:".into(),
            icon: None,
            solo: false,
            aside: String::new(),
            badge: None,
            chip: "label".into(),
        }];
        let mut order = vec![1_i64, 3, 2, 4, 5, 6];
        let mut present: Vec<i64> = self.nations.iter().map(|(id, _)| *id).collect();
        present.sort_unstable();
        for id in present {
            if !order.contains(&id) {
                order.push(id);
            }
        }
        for nation in order {
            if !self.nations.iter().any(|(id, _)| *id == nation) {
                continue;
            }
            let (en, zh) = nation_pair(nation);
            let ko = super::ui_text("ko", zh, en);
            hits.push(Hit {
                title: format!("{}:", lead3(lang, en, zh, &ko)),
                subtitle: String::new(),
                key: format!("nation:{nation}"),
                icon: None,
                solo: false,
                aside: String::new(),
                badge: None,
                chip: "label".into(),
            });
            let nation_id = nation.to_string();
            for mut place in self.place_hits(&nation_id, "", lang) {
                place.key = format!("jump:{nation}|{}", place.key);
                hits.push(place);
            }
        }
        hits
    }

    pub(in crate::db) fn region_rows(&self, query: &str, country: i32, lang: &str) -> Vec<Hit> {
        let query = query.trim().to_lowercase();
        // The picker lists China before Japan; the catalog IDs put Japan at 2 and China at 3.
        let country = match country { 2 => 3, 3 => 2, other => other };
        let directory = self.directory_hits(lang);
        let alternate: HashMap<String, String> = if query.is_empty() {
            HashMap::new()
        } else {
            let mut names = HashMap::<String, String>::new();
            for language in ["zh", "en", "ko"] {
                for hit in self.directory_hits(language) {
                    let name = names.entry(hit.key).or_default();
                    name.push_str(&hit.title.to_lowercase());
                    name.push('\n');
                }
            }
            names
        };
        let matches_name = |hit: &Hit| query.is_empty() || hit.title.to_lowercase().contains(&query)
            || alternate.get(&hit.key).is_some_and(|name| name.contains(&query));
        let mut rows = Vec::new();
        for heading in &directory {
            let Some(nation) = heading.key.strip_prefix("nation:") else { continue };
            if country != 0 && nation.parse::<i32>().ok() != Some(country) {
                continue;
            }
            let heading_matches = matches_name(heading);
            let prefix = format!("jump:{nation}|");
            let children: Vec<Hit> = directory.iter()
                .filter(|hit| hit.key.starts_with(&prefix) && (heading_matches || matches_name(hit)))
                .cloned().collect();
            if heading_matches || !children.is_empty() {
                rows.push(heading.clone());
                rows.extend(children);
            }
        }
        rows
    }

    pub(in crate::db) fn scope_monsters(&self, key: &str, query: &str, lang: &str) -> Vec<Hit> {
        let query = query.trim();
        let folded = query.to_lowercase();
        let (nation, tail) = if let Some(rest) = key.strip_prefix("jump:") {
            let Some((nation, tail)) = rest.split_once('|') else {
                return Vec::new();
            };
            (nation.to_string(), tail.to_string())
        } else if let Some(id) = key.strip_prefix("nation:") {
            (id.to_string(), String::new())
        } else {
            return Vec::new();
        };
        let places = if tail.is_empty() {
            self.nation_gts(&nation)
        } else if let Some(area) = tail.strip_prefix("area:").or_else(|| tail.strip_prefix("span:")) {
            self.group_gts(&nation, area)
        } else if let Some(id) = tail.strip_prefix("region:") {
            id.parse::<i64>().ok().into_iter().collect()
        } else {
            return Vec::new();
        };
        self.monster_hits(query, &folded, lang, &places, None)
    }

    fn nation_gts(&self, nation: &str) -> Vec<i64> {
        let Ok(nation) = nation.parse::<i64>() else {
            return Vec::new();
        };
        self.places
            .iter()
            .filter(|place| place.nation == nation)
            .map(|place| place.gts)
            .collect()
    }

    pub(in crate::db) fn floor_hits(&self, nation: &str, area: &str, folded: &str, lang: &str) -> Vec<Hit> {
        let members = self.group_members(nation, area);
        if members.is_empty() {
            return Vec::new();
        }
        let (en, zh) = bases_of(&members);
        let plain = HashSet::new();
        let mut floors = Vec::new();
        for place in members.iter().copied().filter(|place| !decorative(place, &plain)) {
            if !folded.is_empty() && !text_hit(folded, &place.names) {
                continue;
            }
            let sort = floor_sort_label(lang, &place.en, &place.zh);
            floors.push((
                sort,
                place_hit(lang, &place.en, &place.zh, place.count, format!("region:{}", place.gts)),
            ));
        }
        floors.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.key.cmp(&b.1.key)));
        let mut hits: Vec<Hit> = floors.into_iter().map(|(_, hit)| hit).collect();
        if folded.is_empty() {
            let gts = member_gts(&members);
            let count = self.count_at(&gts);
            let (title, alternate) = all_floor_names(lang, &en, &zh);
            let mut hit = place_hit(lang, &en, &zh, count, format!("span:{area}"));
            hit.title = title;
            hit.subtitle = caption(&alternate, format!("{} {}", commas(count), phrase("monsters", lang)));
            hits.insert(0, hit);
        }
        hits
    }

    pub(in crate::db) fn group_members(&self, nation: &str, area: &str) -> Vec<&PlaceRow> {
        let Ok(nation) = nation.parse::<i64>() else { return Vec::new() };
        self.places
            .iter()
            .filter(|place| place.nation == nation && group_key(place) == area)
            .collect()
    }

    pub(in crate::db) fn group_bases(&self, nation: &str, area: &str) -> Option<(String, String)> {
        let members = self.group_members(nation, area);
        if members.len() < 2 {
            return None;
        }
        Some(bases_of(&members))
    }

    pub(in crate::db) fn group_gts(&self, nation: &str, area: &str) -> Vec<i64> {
        member_gts(&self.group_members(nation, area))
    }

    pub(in crate::db) fn count_at(&self, gts: &[i64]) -> i64 {
        self.monsters
            .iter()
            .filter(|monster| monster.places.iter().any(|place| gts.contains(place)))
            .count() as i64
    }

    pub(in crate::db) fn monster_hits(&self, query: &str, folded: &str, lang: &str, places: &[i64], limit: Option<usize>) -> Vec<Hit> {
        let listed = |monster: &MonsterRow| places.is_empty() || monster.places.iter().any(|place| places.contains(place));
        let mut picked = Vec::new();
        for monster in &self.monsters {
            if !listed(monster) || !self.name_hit(query, folded, &monster.code, &monster.names) {
                continue;
            }
            picked.push(monster);
            if limit.is_some_and(|limit| picked.len() == limit) {
                break;
            }
        }
        if let Some(limit) = limit {
            if picked.len() < limit {
                picked.extend(fuzzy_rows(&self.monsters, folded, limit - picked.len(), |monster| {
                    listed(monster) && !self.name_hit(query, folded, &monster.code, &monster.names)
                }, |monster| &monster.names));
            }
        }
        picked
            .into_iter()
            .map(|monster| {
                let place_name = lead(lang, &monster.place_en, &monster.place_zh);
                let meta = if place_name.is_empty() {
                    monster.code.clone()
                } else {
                    format!("{}  {place_name}", monster.code)
                };
                Hit {
                    title: lead3(lang, &monster.en, &monster.zh, &monster.ko),
                    subtitle: caption(&trail3(lang, &monster.en, &monster.zh, &monster.ko), meta),
                    key: format!("monster:{}", monster.code),
                    icon: monster.icon.clone(),
                    solo: false,
                    aside: String::new(),
                    badge: monster.badge.as_ref().map(|path| (path.clone(), 0)),
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn item_category_hits(&self, lang: &str) -> Vec<Hit> {
        let mut counts: HashMap<&str, i64> = HashMap::new();
        for item in &self.items {
            *counts.entry(item.category.as_str()).or_default() += 1;
        }
        let mut keys: Vec<&str> = counts.keys().copied().collect();
        keys.sort_by_key(|key| item_category_order(key));
        let mut hits: Vec<Hit> = keys
            .into_iter()
            .map(|key| {
                let (en, zh) = item_category_pair(key);
                let count = format!("{} {}", commas(counts[key]), phrase("pieces", lang));
                let id = if key.is_empty() { "other" } else { key };
                Hit {
                    title: lead3(lang, en, zh, &super::ui_text("ko", zh, en)),
                    subtitle: caption(&trail3(lang, en, zh, &super::ui_text("ko", zh, en)), count),
                    key: format!("icat:{id}"),
                    icon: self.item_group_icon(key, ""),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect();
        if !self.sets.is_empty() {
            let (en, zh) = item_category_pair("set");
            let count = format!("{} {}", commas(self.sets.len() as i64), phrase("sets", lang));
            hits.insert(
                0,
                Hit {
                    title: lead3(lang, en, zh, &super::ui_text("ko", zh, en)),
                    subtitle: caption(&trail3(lang, en, zh, &super::ui_text("ko", zh, en)), count),
                    key: "icat:set".into(),
                    icon: self.sets.iter().find_map(|set| set.icon.clone()),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                },
            );
        }
        hits
    }

    pub(in crate::db) fn set_hits(&self, query: &str, folded: &str, lang: &str, group: &str) -> Vec<Hit> {
        let kind = match group {
            "0" => Some(0),
            "1" => Some(1),
            "2" => Some(2),
            _ => None,
        };
        let mut rows: Vec<&SetRow> = if query.is_empty() {
            self.sets.iter().filter(|set| kind.is_none_or(|kind| set.kind == kind)).collect()
        } else {
            self.sets
                .iter()
                .filter(|set| kind.is_none_or(|kind| set.kind == kind) && self.name_hit(query, folded, &set.code.to_string(), &set.names))
                .collect()
        };
        rows.sort_by(|left, right| {
            set_kind_rank(left.kind)
                .cmp(&set_kind_rank(right.kind))
                .then(left.zh.cmp(&right.zh))
                .then(left.en.cmp(&right.en))
                .then(left.code.cmp(&right.code))
        });
        rows.into_iter()
            .map(|set| {
                let title = set_title_lang(lang, &set.ko, &set.en, &set.zh);
                Hit {
                    title,
                    subtitle: caption(&trail3(lang, &set.en, &set.zh, &set.ko), set_piece_label(set.pieces, lang)),
                    key: format!("set:{}", set.code),
                    icon: set.icon.clone(),
                    solo: false,
                    aside: if set.kind == 0 { String::new() } else { set_kind_label(set.kind, lang) },
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn item_has_subtypes(&self, category: &str) -> bool {
        let kind = if category == "other" { "" } else { category };
        let mut seen = HashSet::new();
        for item in &self.items {
            if item.category == kind && !item.subtype.is_empty() {
                seen.insert(item.subtype.as_str());
            }
        }
        seen.len() > 1
    }

    pub(in crate::db) fn item_subtype_hits(&self, category: &str, lang: &str) -> Vec<Hit> {
        let kind = if category == "other" { "" } else { category };
        let mut counts: HashMap<&str, i64> = HashMap::new();
        for item in self.items.iter().filter(|item| item.category == kind && !item.subtype.is_empty()) {
            *counts.entry(item.subtype.as_str()).or_default() += 1;
        }
        let mut keys: Vec<&str> = counts.keys().copied().collect();
        keys.sort_by(|left, right| {
            let other = |key: &&str| if *key == "other" { 1 } else { 0 };
            other(left).cmp(&other(right)).then(counts[right].cmp(&counts[left])).then(left.cmp(right))
        });
        keys.into_iter()
            .map(|key| {
                let (en, zh) = weapon_family_pair(key);
                let count = format!("{} {}", commas(counts[key]), phrase("pieces", lang));
                Hit {
                    title: lead3(lang, en, zh, &super::ui_text("ko", zh, en)),
                    subtitle: caption(&trail3(lang, en, zh, &super::ui_text("ko", zh, en)), count),
                    key: format!("isub:{key}"),
                    icon: self.item_group_icon(kind, key),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn item_group_icon(&self, category: &str, subtype: &str) -> Option<(PathBuf, i32)> {
        let mut best: Option<(i64, (PathBuf, i32))> = None;
        for item in &self.items {
            if item.category != category {
                continue;
            }
            if !subtype.is_empty() && item.subtype != subtype {
                continue;
            }
            let Ok(code) = item.code_str.parse::<i64>() else { continue };
            let Some(icon) = self.item_icons.get(&code) else { continue };
            if best.as_ref().is_none_or(|(have, _)| code < *have) {
                best = Some((code, icon.clone()));
            }
        }
        best.map(|(_, icon)| icon)
    }

    pub(in crate::db) fn item_hits(&self, query: &str, folded: &str, lang: &str, category: &str, subtype: &str, limit: usize) -> Vec<Hit> {
        let set = set_query(query);
        let limit = if set.is_some() { usize::MAX } else { limit };
        let kind = match category {
            "" => None,
            "other" => Some(""),
            other => Some(other),
        };
        let listed = |item: &ItemRow| {
            kind.is_none_or(|kind| item.category == kind) && (subtype.is_empty() || item.subtype == subtype)
        };
        let named = |item: &ItemRow| self.name_hit(query, folded, &item.code_str, &item.names);
        let mut picked = Vec::new();
        for item in &self.items {
            let hit = match set {
                Some((zh, ko, slot)) => {
                    (item.zh.contains(zh) || item.names.contains(ko)) && slot.is_none_or(|slot| item.category == slot)
                }
                None => named(item),
            };
            if !listed(item) || !hit {
                continue;
            }
            picked.push(item);
            if picked.len() == limit {
                break;
            }
        }
        if set.is_none() && picked.len() < limit {
            picked.extend(fuzzy_rows(&self.items, folded, limit - picked.len(), |item| {
                listed(item) && !named(item)
            }, |item| &item.names));
        }
        if set.is_some() {
            picked.sort_by_key(|item| (if plain_piece(item) { 0 } else { 1 }, item.code_str.parse::<i64>().unwrap_or(i64::MAX)));
        }
        picked
            .into_iter()
            .map(|item| {
                let code = &item.code_str;
                Hit {
                    title: lead3(lang, &item.en, &item.zh, &item.ko),
                    subtitle: caption(&trail3(lang, &item.en, &item.zh, &item.ko), format!("#{code}")),
                    key: format!("item:{code}"),
                    icon: code.parse().ok().and_then(|id: i64| self.item_icons.get(&id).cloned()),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn source_hits(&self, lang: &str) -> Vec<Hit> {
        let mut counts: HashMap<&str, i64> = HashMap::new();
        for recipe in &self.recipes {
            *counts.entry(recipe.source.as_str()).or_default() += 1;
        }
        let mut keys: Vec<&str> = counts.keys().copied().collect();
        keys.sort_by_key(|key| pref_rank(key));
        keys.into_iter()
            .map(|key| {
                let (zh, en) = source_names(key);
                let count = format!("{} {}", commas(counts[key]), phrase("recipes", lang));
                let title = lead3(lang, en, zh, &super::ui_text("ko", zh, en));
                Hit {
                    title: if title.is_empty() { key.to_string() } else { title },
                    subtitle: caption(&trail3(lang, en, zh, &super::ui_text("ko", zh, en)), count),
                    key: format!("craft:{key}"),
                    icon: self
                        .recipes
                        .iter()
                        .find(|recipe| recipe.source == key && recipe.icon.is_some())
                        .and_then(|recipe| recipe.icon.clone()),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn recipe_hits(&self, query: &str, folded: &str, lang: &str, source: &str, limit: usize) -> Vec<Hit> {
        let listed = |recipe: &RecipeRow| source.is_empty() || recipe.source == source;
        let mut matched: Vec<&RecipeRow> = self
            .recipes
            .iter()
            .filter(|recipe| listed(recipe) && self.name_hit(query, folded, &recipe.result_str, &recipe.names))
            .collect();
        if query.is_empty() {
            matched.sort_by_key(|recipe| (recipe.level_min.unwrap_or(i64::MAX), recipe.id));
        }
        if matched.len() > limit {
            matched.truncate(limit);
        } else if !query.is_empty() {
            matched.extend(fuzzy_rows(&self.recipes, folded, limit - matched.len(), |recipe| {
                listed(recipe) && !self.name_hit(query, folded, &recipe.result_str, &recipe.names)
            }, |recipe| &recipe.names));
        }
        matched
            .into_iter()
            .map(|recipe| {
                let meta = if source.is_empty() {
                    label(&recipe.source, lang)
                } else {
                    String::new()
                };
                let aside = recipe
                    .level_min
                    .filter(|value| *value > 0)
                    .map(commas)
                    .unwrap_or_default();
                Hit {
                    title: lead3(lang, &recipe.en, &recipe.zh, &recipe.ko),
                    subtitle: caption(&trail3(lang, &recipe.en, &recipe.zh, &recipe.ko), meta),
                    key: format!("recipe:{}", recipe.id),
                    icon: recipe.icon.clone(),
                    solo: false,
                    aside,
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn quest_source_hits(&self, lang: &str) -> Vec<Hit> {
        let mut counts: HashMap<&str, i64> = HashMap::new();
        for quest in &self.quests {
            *counts.entry(quest.source.as_str()).or_default() += 1;
        }
        let mut keys: Vec<&str> = counts.keys().copied().collect();
        keys.sort_by_key(|key| quest_source_rank(key));
        keys.into_iter()
            .map(|key| {
                let (zh, en) = source_names(key);
                let count = format!("{} {}", commas(counts[key]), phrase("quest-count", lang));
                let title = lead3(lang, en, zh, &super::ui_text("ko", zh, en));
                Hit {
                    title: if title.is_empty() { key.to_string() } else { title },
                    subtitle: caption(&trail3(lang, en, zh, &super::ui_text("ko", zh, en)), count),
                    key: format!("qsrc:{key}"),
                    icon: None,
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn quest_hits(&self, query: &str, folded: &str, lang: &str, source: &str, kind: &str, limit: usize) -> Vec<Hit> {
        let listed = |quest: &QuestRow| {
            (source.is_empty() || quest.source == source) && (kind.is_empty() || quest.kind == kind)
        };
        let mut picked = Vec::new();
        for quest in &self.quests {
            if !listed(quest) || !self.name_hit(query, folded, &quest.code, &quest.names) {
                continue;
            }
            picked.push(quest);
            if picked.len() == limit {
                break;
            }
        }
        if picked.len() < limit {
            picked.extend(fuzzy_rows(&self.quests, folded, limit - picked.len(), |quest| {
                listed(quest) && !self.name_hit(query, folded, &quest.code, &quest.names)
            }, |quest| &quest.names));
        }
        picked
            .into_iter()
            .map(|quest| {
                let meta = if source.is_empty() {
                    format!("{}  {}", label(&quest.source, lang), quest.code)
                } else if let Some(type_name) = main_type_name(&quest.kind, lang).filter(|_| source == "main" && kind.is_empty()) {
                    format!("{type_name}  {}", quest.code)
                } else {
                    quest.code.clone()
                };
                Hit {
                    title: lead3(lang, &quest.title_en, &quest.title_zh, &quest.title_ko),
                    subtitle: caption(&trail3(lang, &quest.title_en, &quest.title_zh, &quest.title_ko), meta),
                    key: format!("quest:{}", quest.id),
                    icon: self.npc_face(quest.npc),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub(in crate::db) fn find_item(&self, code: i64) -> Option<&ItemRow> {
        // load_items orders by numeric code; keep lookups logarithmic when a
        // production or town list resolves hundreds of item references.
        self.items.binary_search_by_key(&code, |item| item.code).ok().map(|index| &self.items[index])
    }
}

#[cfg(test)]
mod language_tests {
    use super::*;

    #[test]
    fn generated_all_floor_suffix_uses_primary_language_without_fake_secondary_locations() {
        assert_eq!(all_floor_names("ko", "Forest", "森林"), ("Forest - 전체".into(), String::new()));
        assert_eq!(all_floor_names("ko-zh", "Forest", "森林"), ("森林 - 전체".into(), String::new()));
        assert_eq!(all_floor_names("en-ko", "Forest", "森林"), ("Forest - All".into(), String::new()));
        assert_eq!(all_floor_names("zh-en", "Forest", "森林"), ("森林 - 所有".into(), "Forest - All".into()));
        assert_eq!(all_floor_names("en-zh", "", "森林"), ("森林 - All".into(), String::new()));
    }

    #[test]
    fn all_floor_row_and_scope_title_share_the_korean_label() {
        let catalog = super::super::Catalog::open().expect("catalog");
        let place = catalog.index.places.iter().find(|place| {
            catalog.index.group_members(&place.nation.to_string(), &group_key(place)).len() >= 2
        }).expect("multi-floor region");
        let nation = place.nation.to_string();
        let area = group_key(place);
        let row = catalog.index.floor_hits(&nation, &area, "", "ko").into_iter().next().expect("all floors");
        assert!(row.title.ends_with(" - 전체"));
        assert_eq!(row.title, catalog.span_name(&nation, &area, "ko"));
        assert!(!row.subtitle.contains("All") && !row.subtitle.contains("所有"));
    }
}
