use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

mod alias;
mod hero;
mod item;
mod load;
mod monster;
mod names;
mod npc;
mod paths;
mod prefs;
mod quest;
mod recipe;
mod rows;
mod search;
mod store;
mod text;
mod town;
mod work;

pub use alias::{alias_rows, save_alias_text};
pub use paths::client_label;
pub use prefs::{DockPref, HomePref, WindowPref, load_dock, load_favourites, load_home, load_lang, load_opacity, load_set_extra, load_window, save_dock, save_favourites, save_home, save_lang, save_opacity, save_set_extra, save_window};
pub use text::{hero_land_label, hero_rank_label, hero_tier_label, item_category_label, nation_label, relation_label, source_label, weapon_family_label};

use alias::parse_alias_book;
use load::{choose_pictures, load_portraits, load_sprites};
use paths::{client_build, find_db, locate_client};
use rows::Index;
use search::all_floor_names;
use text::{
    caption, dotted_version, hit_version, lead, notes_placeholder, patch_number, phrase, pick_place, server_name,
    ship_label, show, stack, trail,
};

#[derive(Clone)]
pub struct Hit {
    pub title: String,
    pub subtitle: String,
    pub key: String,
    pub icon: Option<(PathBuf, i32)>,
    pub solo: bool,
    pub badge: Option<(PathBuf, i32)>,
    pub aside: String,
    pub chip: String,
}

pub struct Catalog {
    conn: Connection,
    client_root: PathBuf,
    pictures: HashMap<String, (PathBuf, i32)>,
    index: Index,
}

#[cfg(test)]
mod tests;

impl Catalog {
    pub fn open() -> Result<Self, String> {
        let db = find_db()?;
        let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|err| err.to_string())?;
        let stored = conn
            .query_row(
                "SELECT path FROM sources WHERE role = 'gersang.gcs' ORDER BY revision_id DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .map(|path| PathBuf::from(path).parent().map(Path::to_path_buf).unwrap_or_default())
            .unwrap_or_default();
        let client_root = locate_client(stored, db.parent().unwrap_or(Path::new(".")));
        let pictures = choose_pictures(&client_root, &load_sprites(&conn), &load_portraits(&conn));
        let index = Index::load(&conn, &client_root, &pictures, db.parent().unwrap_or(Path::new(".")))?;
        Ok(Self { conn, client_root, pictures, index })
    }

    pub fn set_aliases(&self, text: &str) {
        save_alias_text(text);
        *self.index.aliases.borrow_mut() = parse_alias_book(text);
    }

    pub fn alias_items(&self, codes: &str, lang: &str) -> Vec<Hit> {
        codes
            .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .filter_map(|code| {
                let id = code.trim_start_matches('#').parse::<i64>().ok()?;
                let item = self.index.items.iter().find(|item| item.code_str == id.to_string())?;
                let name = lead(lang, &item.en, &item.zh);
                Some(Hit {
                    title: if name.is_empty() { format!("#{id}") } else { name },
                    subtitle: format!("#{id}"),
                    key: format!("item:{id}"),
                    icon: self.index.item_icons.get(&id).cloned(),
                    solo: false,
                    badge: None,
                    aside: String::new(),
                    chip: String::new(),
                })
            })
            .collect()
    }

    pub fn alias_names(&self, codes: &str, lang: &str) -> String {
        codes
            .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .map(|code| {
                let code = code.trim_start_matches('#');
                self.index
                    .items
                    .iter()
                    .find(|item| item.code_str == code)
                    .map(|item| {
                        let name = lead(lang, &item.en, &item.zh);
                        if name.is_empty() { format!("#{code}") } else { name }
                    })
                    .unwrap_or_else(|| format!("#{code}"))
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub fn search(&self, category: &str, query: &str, lang: &str, place: &str, nation: &str) -> Vec<Hit> {
        self.search_scoped(category, query, lang, place, nation, "", "")
    }

    pub fn search_scoped(
        &self,
        category: &str,
        query: &str,
        lang: &str,
        place: &str,
        nation: &str,
        ship: &str,
        area: &str,
    ) -> Vec<Hit> {
        let query = query.trim();
        let folded = query.to_lowercase();
        self.index.prepare_aliases(&folded);
        match category {
            "monster" if place.is_empty() && area.is_empty() && nation.is_empty() && query.is_empty() => self.index.nation_hits(lang),
            "monster" if place.is_empty() && area.is_empty() && nation.is_empty() => self.index.monster_hits(query, &folded, lang, &[], Some(80)),
            "monster" if place.is_empty() && area.is_empty() => self.index.place_hits(nation, &folded, lang),
            "monster" if place.is_empty() => self.index.floor_hits(nation, area, &folded, lang),
            "monster" => {
                if let Some(area_key) = place.strip_prefix("span:") {
                    let gts = self.index.group_gts(nation, area_key);
                    self.index.monster_hits(query, &folded, lang, &gts, Some(80))
                } else {
                    match place.parse::<i64>() {
                        Ok(gts) => self.index.monster_hits(query, &folded, lang, &[gts], Some(80)),
                        Err(_) => Vec::new(),
                    }
                }
            }
            "monster-all" => self.index.monster_hits(query, &folded, lang, &[], None),
            "item" if place.is_empty() && query.is_empty() => self.index.item_category_hits(lang),
            "item" if place == "set" => self.index.set_hits(query, &folded, lang, area),
            "item" if area.is_empty() && query.is_empty() && self.index.item_has_subtypes(place) => self.index.item_subtype_hits(place, lang),
            "item" => self.index.item_hits(query, &folded, lang, place, area),
            "recipe" if place.is_empty() && query.is_empty() => self.index.source_hits(lang),
            "recipe" => self.index.recipe_hits(query, &folded, lang, place),
            "quest" if place.is_empty() && query.is_empty() => self.index.quest_source_hits(lang),
            "quest" => self.index.quest_hits(query, &folded, lang, place, area),
            "npc" => self.index.npc_hits(query, &folded, lang),
            "hero" if ship == "land" && place.is_empty() && area.is_empty() && query.is_empty() => self.index.hero_land_hits(lang),
            "hero" if ship == "land" && area.is_empty() && query.is_empty() => self.index.hero_tier_hits(place, lang),
            "hero" if ship == "land" => self.index.hero_hits(query, &folded, lang, area, place),
            "hero" if place.is_empty() && query.is_empty() => self.index.hero_rank_hits(lang),
            "hero" => self.index.hero_hits(query, &folded, lang, place, ""),
            "works" => self.index.work_hits(query, &folded, lang, place, area),
            "atlas" => self.index.atlas_hits(query, &folded, lang, place, area),
            "patch" => self.search_patches(query, &format!("%{query}%"), lang, place, nation, ship),
            _ => Vec::new(),
        }
    }

    fn has_table(&self, name: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [name],
                |row| row.get::<_, i64>(0),
            )
            .is_ok()
    }

    fn search_patches(&self, query: &str, like: &str, lang: &str, server: &str, sort: &str, ship: &str) -> Vec<Hit> {
        if !self.has_table("patches") {
            return notes_placeholder(lang);
        }
        let mut stmt = match self.conn.prepare(
            "SELECT version, server, posted_on, title_ko, title_en, title_zh
             FROM patches
             WHERE (?4 = '' OR server = ?4)
               AND (?1 = ''
                OR version LIKE ?2
                OR posted_on LIKE ?2
                OR title_ko LIKE ?2 OR title_en LIKE ?2 OR title_zh LIKE ?2
                OR EXISTS (
                    SELECT 1 FROM patch_lines l
                    WHERE l.version = patches.version AND l.server = patches.server
                      AND (l.ko LIKE ?2 OR l.en LIKE ?2 OR l.zh LIKE ?2)
                ))
             ORDER BY CASE WHEN posted_on = '' THEN 1 ELSE 0 END,
                      CASE WHEN ?3 = 'old' THEN posted_on END ASC,
                      CASE WHEN ?3 = 'old' THEN NULL ELSE posted_on END DESC,
                      CAST(substr(version, 2) AS INTEGER) DESC",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return notes_placeholder(lang),
        };
        let current = client_build();
        let hits: Vec<Hit> = stmt
            .query_map(rusqlite::params![query, like, sort, server], |row| {
                let version: String = row.get(0)?;
                let server: String = row.get(1)?;
                let posted: String = row.get(2)?;
                let en: String = row.get(4)?;
                let zh: String = row.get(5)?;
                let server_label = server_name(&server, lang);
                let shipped = patch_number(&version) <= current;
                let shown = dotted_version(&version);
                let meta = if posted.is_empty() {
                    format!("{shown}  {server_label}")
                } else {
                    format!("{shown}  {posted}  {server_label}")
                };
                let title = lead(lang, &en, &zh);
                Ok(Hit {
                    title: if title.is_empty() { shown } else { title },
                    subtitle: caption(&trail(lang, &en, &zh), meta),
                    key: format!("patch:{version}:{server}"),
                    icon: None,
                    solo: false,
                    aside: phrase(if shipped { "shipped" } else { "unshipped" }, lang),
                    badge: None,
                    chip: if shipped { "yes" } else { "no" }.to_string(),
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default();
        if hits.is_empty() && query.is_empty() && server.is_empty() && ship.is_empty() {
            return notes_placeholder(lang);
        }
        hits.into_iter()
            .filter(|hit| match ship {
                "yes" => patch_number(hit_version(&hit.key)) <= current,
                "no" => patch_number(hit_version(&hit.key)) > current,
                _ => true,
            })
            .collect()
    }

    fn patch_text(&self, id: &str, lang: &str) -> String {
        let Some((version, server)) = id.split_once(':') else {
            return phrase("notes-help", lang);
        };
        if !self.has_table("patch_lines") {
            return phrase("notes-help", lang);
        }
        let row = self.conn.query_row(
            "SELECT posted_on, title_ko, title_en, title_zh FROM patches WHERE version = ?1 AND server = ?2",
            (version, server),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        );
        let Ok((posted, _title_ko, title_en, title_zh)) = row else {
            return phrase("missing", lang);
        };
        let mut text = format!(
            "{}    {}    {}",
            dotted_version(version),
            server_name(server, lang),
            ship_label(version, lang)
        );
        if !posted.is_empty() {
            text.push_str(&format!("\n{posted}"));
        }
        let title = stack(lang, &title_en, &title_zh);
        if !title.is_empty() {
            text.push_str(&format!("\n\n{title}"));
        }
        text.push('\n');
        let mut stmt = match self.conn.prepare(
            "SELECT kind, ko, en, zh FROM patch_lines WHERE version = ?1 AND server = ?2 ORDER BY ord",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return text,
        };
        let rows = stmt.query_map((version, server), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        });
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                let (kind, _ko, en, zh) = row;
                let line = stack(lang, &en, &zh);
                if kind == "heading" {
                    text.push_str(&format!("\n\n{line}\n"));
                } else {
                    text.push_str(&format!("\n{line}\n"));
                }
            }
        }
        text
    }

    pub fn region_name(&self, gts: &str, lang: &str) -> String {
        self.conn
            .query_row(
                "SELECT IFNULL(lz.text, ''), IFNULL(le.text, '')
                 FROM strings lz
                 LEFT JOIN strings le ON le.object = 'OnlineText.dat' AND le.lang = 'en' AND le.id = lz.id
                 WHERE lz.object = 'OnlineText.dat' AND lz.lang = 'zh' AND lz.id = printf('%08d', ?1)",
                [gts],
                |row| Ok(pick_place(lang, &row.get::<_, String>(1)?, &row.get::<_, String>(0)?)),
            )
            .unwrap_or_else(|_| gts.to_string())
    }

    pub fn area_name(&self, nation: &str, area: &str, lang: &str) -> String {
        self.index
            .group_bases(nation, area)
            .map(|(en, zh)| show(lang, &en, &zh))
            .unwrap_or_else(|| area.to_string())
    }

    pub fn span_name(&self, nation: &str, area: &str, lang: &str) -> String {
        self.index
            .group_bases(nation, area)
            .map(|(en, zh)| {
                let (en_title, zh_title) = all_floor_names(&en, &zh);
                show(lang, &en_title, &zh_title)
            })
            .unwrap_or_else(|| area.to_string())
    }

    pub fn scope_saved(&self, key: &str, nation: &str, codes: &HashSet<String>) -> bool {
        if codes.is_empty() {
            return false;
        }
        if let Some(id) = key.strip_prefix("nation:") {
            let Ok(id) = id.parse::<i64>() else { return false };
            return self.index.monsters.iter().any(|monster| monster.nation == id && codes.contains(&monster.code));
        }
        let gts = if let Some(area) = key.strip_prefix("area:").or_else(|| key.strip_prefix("span:")) {
            self.index.group_gts(nation, area)
        } else if let Some(id) = key.strip_prefix("region:") {
            let Ok(id) = id.parse::<i64>() else { return false };
            vec![id]
        } else {
            return false;
        };
        self.index.monsters.iter().any(|monster| {
            codes.contains(&monster.code) && monster.places.iter().any(|place| gts.contains(place))
        })
    }

    pub fn open_key(&self, key: &str, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let (kind, id) = key.split_once(':').unwrap_or(("", ""));
        match kind {
            "item" => self.open_item(id.parse().unwrap_or(0), lang),
            "recipe" => self.open_recipe(id.parse().unwrap_or(0), lang),
            "quest" => {
                let quest_id = id.parse().unwrap_or(0);
                (self.quest_text(quest_id, lang), self.quest_links(quest_id, lang), self.quest_npc_icon(quest_id))
            }
            "monster" => (self.monster_text(id, lang), self.monster_drops(id, lang), self.sprite(id)),
            "npc" => self.open_npc(id.parse().unwrap_or(0), lang),
            "hero" => self.open_hero(id, lang),
            "set" => self.open_set(id.parse().unwrap_or(0), lang),
            "chart" => self.open_chart(id.parse().unwrap_or(0), lang),
            "patch" => (self.patch_text(id, lang), Vec::new(), None),
            "note" => (phrase("notes-help", lang), Vec::new(), None),
            _ => (phrase("missing", lang), Vec::new(), None),
        }
    }

    fn push_section(&self, hits: &mut Vec<Hit>, lang: &str, key: &str, mut rows: Vec<Hit>) {
        if rows.is_empty() {
            return;
        }
        let count = rows.len();
        hits.push(Hit {
            title: phrase(key, lang),
            subtitle: count.to_string(),
            key: format!("section:{key}"),
            icon: None,
            solo: false,
            aside: String::new(),
            badge: None,
            chip: "section".into(),
        });
        for row in rows.iter_mut().take(24) {
            if row.chip.is_empty() {
                row.chip = key.to_string();
            }
        }
        hits.extend(rows.into_iter().take(24));
    }
}
