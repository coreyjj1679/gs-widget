use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::alias::parse_alias_book;
use super::hero::{load_chains, load_heroes};
use super::item::{load_item_icons, load_items, load_sets};
use super::monster::{load_locations, load_monsters, load_places, nation_counts};
use super::names::haystack;
use super::npc::load_npcs;
use super::quest::load_quests;
use super::recipe::load_recipes;
use super::rows::Index;
use super::store::load_online;
use super::town::{load_charts, load_towns};
use super::work::{load_formulas, load_works};

pub(in crate::db) fn load_portraits(conn: &Connection) -> HashMap<String, (String, i32)> {
    let mut stmt = match conn.prepare("SELECT code, path, frame FROM monster_portraits") {
        Ok(stmt) => stmt,
        Err(_) => return HashMap::new(),
    };
    stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i32>(2)?))
    })
    .map(|rows| {
        rows.filter_map(Result::ok)
            .map(|(code, path, frame)| (code, (path, frame)))
            .collect()
    })
    .unwrap_or_default()
}

pub(in crate::db) fn choose_pictures(
    client_root: &Path,
    sprites: &HashMap<String, String>,
    portraits: &HashMap<String, (String, i32)>,
) -> HashMap<String, (PathBuf, i32)> {
    let mut pictures = HashMap::new();
    for (code, relative) in sprites {
        let path = client_root.join(relative.replace('\\', "/"));
        if path.is_file() {
            pictures.insert(code.clone(), (path, 0));
        }
    }
    for (code, (relative, frame)) in portraits {
        let path = client_root.join(relative.replace('\\', "/"));
        if path.is_file() {
            pictures.insert(code.clone(), (path, *frame));
        }
    }
    pictures
}

pub(in crate::db) fn load_sprites(conn: &Connection) -> std::collections::HashMap<String, String> {
    let mut stmt = match conn.prepare("SELECT code, path FROM monster_sprites") {
        Ok(stmt) => stmt,
        Err(_) => return std::collections::HashMap::new(),
    };
    stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

impl Index {
    pub(in crate::db) fn load(
        conn: &Connection,
        client_root: &Path,
        hero_asset_roots: &[PathBuf],
        pictures: &HashMap<String, (PathBuf, i32)>,
        alias_text: &str,
    ) -> Result<Self, String> {
        let texts = load_online(conn)?;
        let locations = load_locations(conn)?;
        let monsters = load_monsters(conn, client_root, pictures, &texts, &locations)?;
        let nations = nation_counts(&monsters);
        let places = load_places(conn, &texts)?;
        let items = load_items(conn, &texts)?;
        let item_icons = load_item_icons(conn, client_root)?;
        let recipes = load_recipes(conn, &texts, &item_icons)?;
        let quests = load_quests(conn)?;
        let npcs = load_npcs(conn, client_root, &texts)?;
        let heroes = load_heroes(conn, hero_asset_roots, &texts)?;
        let book = load_chains(conn, &texts, &items)?;
        let (works, yards) = load_works(conn, client_root, &texts)?;
        let formulas = load_formulas(conn)?;
        let (sets, set_magic) = load_sets(conn, &texts, &item_icons)?;
        let towns = load_towns(conn, &texts)?;
        let charts = load_charts(conn, &texts)?;
        let aliases = RefCell::new(parse_alias_book(alias_text));
        let game_version: Option<String> = conn.query_row(
            "SELECT value FROM meta WHERE key='game_version'", [], |r| r.get(0)
        ).ok();
        let previous_names = game_version.as_deref()
            .map(|v| super::name_history::previous_names(v.trim()))
            .unwrap_or_default();
        let mut index = Self {
            game_version,
            monsters,
            nations,
            places,
            items,
            item_icons,
            recipes,
            quests,
            npcs,
            heroes,
            chains: book.lines,
            combos: book.combos,
            roster_uses: book.uses,
            works,
            yards,
            formulas,
            sets,
            set_magic,
            towns,
            charts,
            aliases,
            active_alias: Cell::new(false),
            active_codes: RefCell::new(HashSet::new()),
        };
        index.add_former_names(previous_names);
        Ok(index)
    }

    /// Add official former names to the same ranked terms as current names.
    /// Their routes are scoped to this installed version by the archive.
    fn add_former_names<'a>(&mut self, entries: impl IntoIterator<Item = (&'a str, &'a str)>) {
        for (route, name) in entries {
            let Some((kind, id)) = route.split_once(':') else { continue };
            let number = id.parse::<i64>().ok();
            let terms = match kind {
                "item" => number.and_then(|code| self.items.binary_search_by_key(&code, |r| r.code).ok())
                    .map(|index| &mut self.items[index].names),
                "monster" => self.monsters.iter_mut().find(|r| r.code == id).map(|r| &mut r.names),
                "hero" => self.heroes.iter_mut().find(|r| r.code == id).map(|r| &mut r.names),
                "npc" => number.and_then(|code| self.npcs.iter_mut().find(|r| r.code == code)).map(|r| &mut r.names),
                "set" => number.and_then(|code| self.sets.iter_mut().find(|r| r.code == code)).map(|r| &mut r.names),
                "quest" => id.split_once(':').and_then(|(source, code)| self.quests.iter_mut().find(|r| r.source == source && r.code == code)).map(|r| &mut r.names),
                "town" => self.towns.iter_mut().find(|r| r.code == id).map(|r| &mut r.names),
                "yard" => self.works.iter_mut().find(|r| r.code == id).map(|r| &mut r.names),
                "bay" => self.yards.get_mut(id).map(|r| &mut r.names),
                _ => None,
            };
            let Some(terms) = terms else { continue };
            for term in haystack(&[name]).lines() {
                if !terms.lines().any(|t| t == term) {
                    terms.push('\n');
                    terms.push_str(term);
                }
            }
        }
    }
}
