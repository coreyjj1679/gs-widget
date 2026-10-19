use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::alias::parse_alias_book;
use super::hero::{load_chains, load_heroes};
use super::item::{load_item_icons, load_items, load_sets};
use super::monster::{load_locations, load_monsters, load_places, nation_counts};
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
        let heroes = load_heroes(conn, client_root, &texts)?;
        let book = load_chains(conn, &texts, &items)?;
        let (works, yards) = load_works(conn, client_root, &texts)?;
        let formulas = load_formulas(conn)?;
        let (sets, set_magic) = load_sets(conn, &texts, &item_icons)?;
        let towns = load_towns(conn, &texts)?;
        let charts = load_charts(conn, &texts)?;
        let aliases = RefCell::new(parse_alias_book(alias_text));
        Ok(Self {
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
        })
    }
}
