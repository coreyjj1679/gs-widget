use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::Connection;

use super::{Catalog, Hit};
use super::names::haystack;
use super::rows::{ChartRow, Index, TownGood, TownRow};
use super::store::{online, table_exists};
use super::text::{caption, commas, lead, phrase, realm_pair, stack, trail};

pub(in crate::db) fn load_towns(conn: &Connection, texts: &HashMap<String, String>) -> Result<Vec<TownRow>, String> {
    if !table_exists(conn, "towns") {
        return Ok(Vec::new());
    }
    let mut goods: HashMap<String, Vec<TownGood>> = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT town, item_code, specialty FROM town_goods ORDER BY town, slot")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)))
        .map_err(|err| err.to_string())?;
    for row in rows {
        let (town, item, specialty) = row.map_err(|err| err.to_string())?;
        goods.entry(town).or_default().push(TownGood { item, specialty: specialty != 0 });
    }
    let mut stmt = conn
        .prepare("SELECT code, name_ko, name_gts, x, y, region FROM towns")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut towns = Vec::new();
    for row in rows {
        let (code, ko, gts, x, y, region) = row.map_err(|err| err.to_string())?;
        let en = online(texts, gts, "en").to_string();
        let zh = online(texts, gts, "zh").to_string();
        let (realm_en, realm_zh) = realm_pair(&region);
        let mine = goods.remove(&code).unwrap_or_default();
        towns.push(TownRow {
            names: haystack(&[&ko, &code, &en, &zh, realm_en, realm_zh]),
            code,
            en,
            zh,
            region,
            x,
            y,
            goods: mine,
        });
    }
    Ok(towns)
}

pub(in crate::db) fn load_charts(conn: &Connection, texts: &HashMap<String, String>) -> Result<Vec<ChartRow>, String> {
    if !table_exists(conn, "maps") {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare("SELECT code, name_ko, name_gts FROM maps ORDER BY code")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))
        .map_err(|err| err.to_string())?;
    let mut charts = Vec::new();
    for row in rows {
        let (code, ko, gts) = row.map_err(|err| err.to_string())?;
        let en = online(texts, gts, "en").to_string();
        let zh = online(texts, gts, "zh").to_string();
        if en.is_empty() && zh.is_empty() && ko.is_empty() {
            continue;
        }
        charts.push(ChartRow { code, en, zh });
    }
    charts.sort_by(|a, b| a.zh.cmp(&b.zh).then(a.en.cmp(&b.en)).then(a.code.cmp(&b.code)));
    Ok(charts)
}

impl Catalog {
    pub(in crate::db) fn open_chart(&self, code: i64, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let Some(chart) = self.index.charts.iter().find(|chart| chart.code == code) else {
            return (phrase("missing", lang), Vec::new(), None);
        };
        let title = stack(lang, &chart.en, &chart.zh);
        let mut links = Vec::new();
        for npc in self.index.npcs.iter().filter(|npc| npc.map_index == code) {
            links.push(Hit {
                title: lead(lang, &npc.en, &npc.zh),
                subtitle: caption(&trail(lang, &npc.en, &npc.zh), format!("{}, {}", commas(npc.x), commas(npc.y))),
                key: format!("npc:{}", npc.code),
                icon: npc.icon.clone(),
                solo: false,
                aside: phrase("people-here", lang),
                badge: None,
                chip: String::new(),
            });
        }
        for monster in &self.index.monsters {
            if chart.zh.is_empty() || monster.place_zh != chart.zh {
                continue;
            }
            links.push(Hit {
                title: lead(lang, &monster.en, &monster.zh),
                subtitle: trail(lang, &monster.en, &monster.zh),
                key: format!("monster:{}", monster.code),
                icon: monster.icon.clone(),
                solo: false,
                aside: phrase("lurking", lang),
                badge: None,
                chip: String::new(),
            });
        }
        (title, links, None)
    }

    pub fn realm_name(&self, id: &str, lang: &str) -> String {
        let (en, zh) = realm_pair(id);
        lead(lang, en, zh)
    }

    pub fn town_name(&self, code: &str, lang: &str) -> String {
        self.index.town_name(code, lang)
    }

    pub fn town_region(&self, code: &str) -> String {
        self.index.towns.iter().find(|town| town.code == code).map(|town| town.region.clone()).unwrap_or_default()
    }
}

impl Index {
    pub(in crate::db) fn atlas_hits(&self, query: &str, folded: &str, lang: &str, region: &str, town: &str) -> Vec<Hit> {
        if !town.is_empty() {
            return self.town_goods(town, query, folded, lang);
        }
        if !region.is_empty() {
            return self.town_hits(region, query, folded, lang);
        }
        if query.is_empty() {
            return self.realm_hits(lang);
        }
        self.town_hits("", query, folded, lang)
    }

    pub(in crate::db) fn realm_hits(&self, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        for id in ["joseon", "japan", "china", "taiwan", "india", "mongolia"] {
            let count = self.towns.iter().filter(|town| town.region == id).count() as i64;
            if count == 0 {
                continue;
            }
            let (en, zh) = realm_pair(id);
            hits.push(Hit {
                title: lead(lang, en, zh),
                subtitle: caption(&trail(lang, en, zh), count.to_string()),
                key: format!("realm:{id}"),
                icon: None,
                solo: false,
                aside: String::new(),
                badge: None,
                chip: String::new(),
            });
        }
        hits
    }

    pub(in crate::db) fn town_hits(&self, region: &str, query: &str, folded: &str, lang: &str) -> Vec<Hit> {
        let mut towns: Vec<&TownRow> = self
            .towns
            .iter()
            .filter(|town| region.is_empty() || town.region == region)
            .filter(|town| self.name_hit(query, folded, &town.code, &town.names))
            .collect();
        towns.sort_by_key(|town| (town.y, town.x, town.zh.clone()));
        towns
            .into_iter()
            .map(|town| Hit {
                title: lead(lang, &town.en, &town.zh),
                subtitle: caption(&trail(lang, &town.en, &town.zh), format!("{}, {}", commas(town.x), commas(town.y))),
                key: format!("town:{}", town.code),
                icon: None,
                solo: false,
                aside: {
                    let (en, zh) = realm_pair(&town.region);
                    lead(lang, en, zh)
                },
                badge: None,
                chip: String::new(),
            })
            .collect()
    }

    pub(in crate::db) fn realm_goods(&self, region: &str, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        for town in self.towns.iter().filter(|town| town.region == region) {
            hits.extend(self.town_goods(&town.code, "", "", lang));
        }
        hits
    }

    pub(in crate::db) fn town_goods(&self, code: &str, query: &str, folded: &str, lang: &str) -> Vec<Hit> {
        let Some(town) = self.towns.iter().find(|town| town.code == code) else {
            return Vec::new();
        };
        let mut goods = town.goods.clone();
        goods.sort_by_key(|good| (!good.specialty, good.item));
        goods
            .into_iter()
            .filter_map(|good| {
                let item = self.find_item(good.item)?;
                if !self.name_hit(query, folded, &item.code_str, &item.names) {
                    return None;
                }
                let title = lead(lang, &item.en, &item.zh);
                Some(Hit {
                    title: if title.is_empty() { good.item.to_string() } else { title },
                    subtitle: trail(lang, &item.en, &item.zh),
                    key: format!("item:{}", good.item),
                    icon: self.item_icons.get(&good.item).cloned(),
                    solo: false,
                    aside: if good.specialty { phrase("specialty", lang) } else { String::new() },
                    badge: None,
                    chip: String::new(),
                })
            })
            .collect()
    }

    pub fn town_name(&self, code: &str, lang: &str) -> String {
        self.towns
            .iter()
            .find(|town| town.code == code)
            .map(|town| lead(lang, &town.en, &town.zh))
            .unwrap_or_else(|| code.to_string())
    }
}
