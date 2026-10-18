use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::alias::{alias_codes, set_query};
use super::item::{item_category_order, item_category_pair, pref_rank, set_kind_label, set_kind_rank, set_piece_label, set_title_lang, weapon_family_pair};
use super::Hit;
use super::names::{fuzzy_rows, haystack, name_match, plain_piece, text_hit};
use super::quest::{main_type_name, quest_source_rank};
use super::rows::{Index, ItemRow, MonsterRow, PlaceRow, QuestRow, RecipeRow, SetRow};
use super::text::{caption, commas, label, lead, nation_pair, phrase, source_names, trail};

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

pub(in crate::db) fn all_floor_names(en: &str, zh: &str) -> (String, String) {
    let en_base = if en.is_empty() { zh } else { en };
    let zh_base = if zh.is_empty() { en } else { zh };
    (format!("{en_base} - All"), format!("{zh_base} - 所有"))
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
                Hit {
                title: lead(lang, en, zh),
                subtitle: caption(&trail(lang, en, zh), format!("{} {}", commas(*count), phrase("monsters", lang))),
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
        let saved = if lang == "en-zh" { "Saved" } else { "收藏" };
        let mut hits = vec![Hit {
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
            hits.push(Hit {
                title: format!("{}:", lead(lang, en, zh)),
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
            let (en_title, zh_title) = all_floor_names(&en, &zh);
            hits.insert(0, place_hit(lang, &en_title, &zh_title, self.count_at(&gts), format!("span:{area}")));
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
                    title: lead(lang, &monster.en, &monster.zh),
                    subtitle: caption(&trail(lang, &monster.en, &monster.zh), meta),
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
                    title: lead(lang, en, zh),
                    subtitle: caption(&trail(lang, en, zh), count),
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
                    title: lead(lang, en, zh),
                    subtitle: caption(&trail(lang, en, zh), count),
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
        if !query.is_empty() && rows.len() > 40 {
            rows.truncate(40);
        }
        rows.into_iter()
            .map(|set| {
                let title = set_title_lang(lang, &set.ko, &set.en, &set.zh);
                Hit {
                    title,
                    subtitle: caption(&trail(lang, &set.en, &set.zh), set_piece_label(set.pieces, lang)),
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
                    title: lead(lang, en, zh),
                    subtitle: caption(&trail(lang, en, zh), count),
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
                    title: lead(lang, &item.en, &item.zh),
                    subtitle: caption(&trail(lang, &item.en, &item.zh), format!("#{code}")),
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
                let title = lead(lang, en, zh);
                Hit {
                    title: if title.is_empty() { key.to_string() } else { title },
                    subtitle: caption(&trail(lang, en, zh), count),
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
                    title: lead(lang, &recipe.en, &recipe.zh),
                    subtitle: caption(&trail(lang, &recipe.en, &recipe.zh), meta),
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
                let title = lead(lang, en, zh);
                Hit {
                    title: if title.is_empty() { key.to_string() } else { title },
                    subtitle: caption(&trail(lang, en, zh), count),
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
                    title: lead(lang, &quest.title_en, &quest.title_zh),
                    subtitle: caption(&trail(lang, &quest.title_en, &quest.title_zh), meta),
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
        let key = code.to_string();
        self.items.iter().find(|item| item.code_str == key)
    }
}
