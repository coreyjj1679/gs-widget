use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use unicode_normalization::UnicodeNormalization;

const PREF: &[&str] = &[
    "alchemy",
    "blacksmith",
    "jeweler",
    "unified",
    "formation",
    "baekje",
    "npc",
    "reinforce",
];

const SOURCE_LABEL: &[(&str, &str, &str, &str)] = &[
    ("unified", "統合", "Unified", "통합"),
    ("alchemy", "鍊金", "Alchemy", "연금"),
    ("blacksmith", "鐵匠", "Blacksmith", "대장"),
    ("jeweler", "細工", "Jeweler", "세공"),
    ("npc", "NPC製造", "NPC craft", "NPC제작"),
    ("formation", "陣形", "Formation", "진형"),
    ("baekje", "百濟寶物", "Baekje", "백제보물"),
    ("reinforce", "強化", "Reinforce", "강화"),
    ("main", "一般任務", "Quest", "일반"),
    ("letter", "書信", "Letter", "서신"),
    ("intro", "入門", "Intro", "입문"),
    ("mercenary", "傭兵劇情", "Mercenary", "용병"),
    ("dispatch", "客棧派遣", "Dispatch", "파견"),
];

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

    fn sprite(&self, code: &str) -> Option<(PathBuf, i32)> {
        self.pictures.get(code).cloned()
    }

    pub fn element_icon(&self, code: &str) -> Option<(PathBuf, i32)> {
        let element: i64 = self
            .conn
            .query_row("SELECT element FROM monsters WHERE code = ?1", [code], |row| row.get(0))
            .ok()?;
        let relative = element_file(element)?;
        let path = self.client_root.join(relative);
        path.is_file().then_some((path, 0))
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

    fn open_item(&self, code: i64, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let row = self.conn.query_row(
            "SELECT i.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''), price, weight,
                    description, large_image, large_frame
             FROM items i
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE i.code = ?1",
            [code],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        );
        let Ok((_ko, en, zh, price, weight, description, image, frame)) = row else {
            return (phrase("missing-item", lang), Vec::new(), None);
        };
        let name = stack(lang, &en, &zh);
        let lines = self.item_lines(code, lang, price, weight);
        let note = self.item_note(self.item_desc_gts(code), &description, lang);
        let mut text = name.clone();
        for (label, value) in &lines {
            text.push_str(&format!("\n{label}  {value}"));
        }
        if !note.is_empty() {
            text.push_str(&format!("\n{note}"));
        }
        let links = self.item_relations(code, lang);
        let icon = self.icon_path(&image, frame);
        (text, links, icon)
    }

    pub fn item_sheet(&self, code: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String)>, String)> {
        let row = self.conn.query_row(
            "SELECT i.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''), price, weight, description, IFNULL(desc_gts, 0)
             FROM items i
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE i.code = ?1",
            [code],
            |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        );
        let Ok((en, zh, price, weight, description, desc_gts)) = row else {
            return None;
        };
        Some((
            lead(lang, &en, &zh),
            String::new(),
            format!("#{code}"),
            self.item_lines(code, lang, price, weight),
            self.item_note(desc_gts, &description, lang),
        ))
    }

    fn item_desc_gts(&self, code: i64) -> i64 {
        self.conn
            .query_row("SELECT IFNULL(desc_gts, 0) FROM items WHERE code = ?1", [code], |row| row.get(0))
            .unwrap_or(0)
    }

    fn item_note(&self, desc_gts: i64, _korean: &str, lang: &str) -> String {
        if desc_gts <= 0 {
            return String::new();
        }
        let zh = self.online_line(desc_gts, "zh");
        let en = self.online_line(desc_gts, "en");
        lead(lang, &en, &zh)
    }

    fn online_line(&self, id: i64, lang: &str) -> String {
        self.conn
            .query_row(
                "SELECT text FROM strings WHERE object = 'OnlineText.dat' AND lang = ?1 AND id = printf('%08d', ?2)",
                (lang, id),
                |row| row.get(0),
            )
            .unwrap_or_default()
    }

    fn item_lines(&self, code: i64, lang: &str, price: i64, weight: i64) -> Vec<(String, String)> {
        let mut lines = vec![(phrase("code", lang), code.to_string())];
        let vendor = price / 3;
        if vendor > 0 {
            lines.push((phrase("vendor", lang), commas(vendor)));
        }
        if weight > 0 {
            lines.push((phrase("weight", lang), commas(weight)));
        }
        if let Some(item) = self.index.items.iter().find(|item| item.code_str == code.to_string()) {
            let mut shown = item_category_name(&item.category, lang);
            if !item.subtype.is_empty() {
                shown = format!("{shown} · {}", weapon_family_name(&item.subtype, lang));
            }
            lines.push((phrase("item-sort", lang), shown));
        }
        lines.extend(self.gear_lines(code, lang));
        lines.extend(self.affix_lines(code, lang));
        lines.extend(self.set_lines(code, lang));
        lines.extend(self.enhance_lines(code, lang));
        lines
    }

    fn set_rows(&self, item_code: i64) -> Vec<(i64, i64, i64, i64)> {
        if !self.has_table("item_sets") {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT s.code, s.kind, s.pieces, m.enhance
             FROM item_set_members m
             JOIN item_sets s ON s.code = m.set_code
             WHERE m.item_code = ?1
             ORDER BY s.kind, s.code",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([item_code], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    fn set_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        let mut lines = Vec::new();
        for (set_code, kind, _pieces, enhance) in self.set_rows(code) {
            let name = self.set_name(set_code, lang);
            let shown = if enhance > 0 { format!("{name}  +{enhance}") } else { name };
            lines.push((set_kind_label(kind, lang), shown));
            lines.extend(self.set_effect_lines(set_code, lang));
        }
        lines
    }

    fn set_name(&self, code: i64, lang: &str) -> String {
        self.index
            .sets
            .iter()
            .find(|set| set.code == code)
            .map(|set| set_title_lang(lang, &set.ko, &set.en, &set.zh))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("#{code}"))
    }

    fn gear_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        let mut bag = StatBag::default();
        bag.add_gear(&self.gear_values(code), lang);
        if let Some((base, _)) = self.base_link(code) {
            if base != code {
                bag.add_gear(&self.gear_values(base), lang);
            }
        }
        bag.rows(lang)
    }

    fn affix_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        let Some(text) = self.affix_text(code, lang) else {
            return Vec::new();
        };
        vec![(phrase("item-fx", lang), text)]
    }

    fn gear_values(&self, code: i64) -> [i64; 10] {
        if !self.has_table("item_gear") {
            return [0; 10];
        }
        self.conn
            .query_row(
                "SELECT strength, dexterity, vitality, intelligence, hp_max, mp_max, attack, armor, hit_resist, magic_resist
                 FROM item_gear WHERE item_code = ?1",
                [code],
                |row| {
                    Ok([
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                    ])
                },
            )
            .unwrap_or([0; 10])
    }

    fn affix_values(&self, code: i64) -> Option<[i64; 10]> {
        if !self.has_table("item_affix") {
            return None;
        }
        self.conn
            .query_row(
                "SELECT strength, dexterity, vitality, intelligence, element, land, attack, reduce, crit, crit_damage
                 FROM item_affix WHERE item_code = ?1",
                [code],
                |row| {
                    Ok([
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                    ])
                },
            )
            .ok()
    }

    fn affix_text(&self, code: i64, lang: &str) -> Option<String> {
        let values = self.affix_values(code)?;
        let mut bag = StatBag::default();
        bag.add_affix(&values, lang);
        let text = bag.text(lang);
        if text.is_empty() { None } else { Some(text) }
    }

    fn set_breakdown(&self, code: i64, lang: &str) -> Vec<(String, String, String, String)> {
        let samples = self.set_piece_samples(code);
        let special = samples.iter().any(|(_, item)| self.affix_values(*item).is_some());
        let mut lines = Vec::new();
        let mut total = StatBag::default();
        if !samples.is_empty() {
            lines.push((phrase("item-fx", lang), String::new(), "fold:item".into(), String::new()));
        }
        for (slot, item) in samples {
            let copies = if slot == "R" { 2 } else { 1 };
            let mut name = self.item_name(item, lang);
            if copies == 2 {
                name = format!("{name} ×2");
            }
            lines.push((String::new(), name, format!("item:{item}"), "item".into()));
            if !special {
                continue;
            }
            let mut bag = self.enhance_bag(item, lang);
            bag.add_gear(&self.gear_values(item), lang);
            if let Some((base, _)) = self.base_link(item) {
                bag.add_gear(&self.gear_values(base), lang);
            }
            if let Some(values) = self.affix_values(item) {
                bag.add_affix(&values, lang);
            }
            if bag.is_empty() {
                continue;
            }
            lines.extend(bag.rows(lang).into_iter().map(|(label, value)| (label, value, String::new(), "item".into())));
            total.absorb(&bag, copies);
        }
        let effects = self.set_effect_rows(code);
        if !effects.is_empty() {
            lines.push((phrase("set-fx", lang), String::new(), "fold:set".into(), String::new()));
        }
        for (pieces, stat, value, magic) in effects {
            let mut rows = Vec::new();
            if let Some(name) = set_stat_name(&stat, lang) {
                if value != 0 {
                    rows.push((name.clone(), format!("+{value}")));
                    total.add(&name, value);
                }
            }
            if magic > 0 {
                if damage_percent_magic(magic) {
                    let pct = self.index.set_magic.get(&magic).map(|row| row.value1).unwrap_or(0);
                    if pct != 0 {
                        rows.push((phrase("damage", lang), format!("+{pct}%")));
                        total.add_damage_pct(pct);
                    }
                } else {
                    let note = self.magic_line(magic, lang);
                    if !note.is_empty() {
                        rows.push((note.clone(), String::new()));
                        total.note(note);
                    }
                }
            }
            if rows.is_empty() {
                continue;
            }
            lines.push((set_piece_label(pieces, lang), String::new(), String::new(), "set".into()));
            lines.extend(rows.into_iter().map(|(label, value)| (label, value, String::new(), "set".into())));
        }
        if special && !total.is_empty() {
            lines.push((phrase("total-fx", lang), String::new(), "fold:total".into(), String::new()));
            lines.extend(total.rows(lang).into_iter().map(|(label, value)| (label, value, String::new(), "total".into())));
        }
        lines
    }

    fn set_piece_samples(&self, code: i64) -> Vec<(String, i64)> {
        let mut seen = HashSet::new();
        let mut samples = Vec::new();
        for (slot, item, _) in self.set_member_slots(code) {
            if seen.insert(slot.clone()) {
                samples.push((slot, item));
            }
        }
        samples
    }

    fn enhance_bag(&self, code: i64, lang: &str) -> StatBag {
        let mut bag = StatBag::default();
        if !self.has_table("enhance") {
            return bag;
        }
        let row = self.conn.query_row(
            "SELECT dmin, dmax, move, atk_speed, magic_resist, hit_resist, element_damage, crit, mana, sight,
                    air, ground, cost, merc_def, merc_atk, spell, reflect, invuln
             FROM enhance WHERE item_code = ?1 ORDER BY plus LIMIT 1",
            [code],
            |row| {
                Ok(EnhanceRow {
                    plus: 0,
                    dmin: row.get(0)?,
                    dmax: row.get(1)?,
                    move_speed: row.get(2)?,
                    atk_speed: row.get(3)?,
                    magic_resist: row.get(4)?,
                    hit_resist: row.get(5)?,
                    element: String::new(),
                    element_damage: row.get(6)?,
                    attack_range: 0,
                    skill_range: 0,
                    crit: row.get(7)?,
                    mana: row.get(8)?,
                    sight: row.get(9)?,
                    air: row.get(10)?,
                    ground: row.get(11)?,
                    cost: row.get(12)?,
                    merc_def: row.get(13)?,
                    merc_atk: row.get(14)?,
                    spell: row.get(15)?,
                    reflect: row.get(16)?,
                    invuln: row.get(17)?,
                })
            },
        );
        if let Ok(row) = row {
            if row.dmin != 0 || row.dmax != 0 {
                bag.dmin += row.dmin;
                bag.dmax += row.dmax;
            }
            bag.add(&phrase("move-speed", lang), row.move_speed);
            bag.add(&phrase("atk-speed", lang), row.atk_speed);
            bag.add(&phrase("magic-resist", lang), row.magic_resist);
            bag.add(&phrase("hit-resist", lang), row.hit_resist);
            bag.add(&phrase("ela", lang), row.element_damage);
            bag.add(&phrase("crit", lang), row.crit);
            bag.add(&phrase("mana-cut", lang), row.mana);
            bag.add(&phrase("sight", lang), row.sight);
            bag.add(&phrase("air-dmg", lang), row.air);
            bag.add(&phrase("ground-dmg", lang), row.ground);
            bag.add(&phrase("stigma", lang), row.cost);
            bag.add(&phrase("merc-def", lang), row.merc_def);
            bag.add(&phrase("merc-atk", lang), row.merc_atk);
            bag.add(&phrase("spell-rate", lang), row.spell);
            bag.add(&phrase("reflect", lang), row.reflect);
            bag.add(&phrase("invuln", lang), row.invuln);
        }
        bag
    }

    fn set_effect_rows(&self, code: i64) -> Vec<(i64, String, i64, i64)> {
        if !self.has_table("item_set_effects") {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT pieces, stat, value, magic FROM item_set_effects WHERE set_code = ?1 ORDER BY pieces",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([code], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    fn set_effect_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        if !self.has_table("item_set_effects") {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT pieces, stat, value, magic FROM item_set_effects WHERE set_code = ?1 ORDER BY pieces",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([code], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
        })
        .map(|rows| {
            rows.filter_map(Result::ok)
                .filter_map(|(pieces, stat, value, magic)| {
                    let text = self.effect_text(&stat, value, magic, lang);
                    if text.is_empty() {
                        None
                    } else {
                        Some((set_piece_label(pieces, lang), text))
                    }
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn effect_text(&self, stat: &str, value: i64, magic: i64, lang: &str) -> String {
        let mut parts = Vec::new();
        if let Some(text) = set_stat_line(stat, value, lang) {
            parts.push(text);
        }
        if magic > 0 {
            let text = self.magic_line(magic, lang);
            if !text.is_empty() {
                parts.push(text);
            }
        }
        parts.join("、")
    }

    fn magic_line(&self, code: i64, lang: &str) -> String {
        let Some(magic) = self.index.set_magic.get(&code) else {
            return String::new();
        };
        let named = set_magic_line(code, magic.value1, magic.value2, magic.percent, lang);
        if named.is_empty() { magic.note.clone() } else { named }
    }

    fn item_set_links(&self, code: i64, lang: &str) -> Vec<Hit> {
        self.set_rows(code)
            .into_iter()
            .map(|(set_code, kind, pieces, _enhance)| Hit {
                title: self.set_name(set_code, lang),
                subtitle: set_piece_label(pieces, lang),
                key: format!("set:{set_code}"),
                icon: self.index.sets.iter().find(|set| set.code == set_code).and_then(|set| set.icon.clone()),
                solo: false,
                aside: if kind == 0 { String::new() } else { set_kind_label(kind, lang) },
                badge: None,
                chip: String::new(),
            })
            .collect()
    }

    pub fn set_sheet(&self, code: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String, String, String)>, String)> {
        let set = self.index.sets.iter().find(|set| set.code == code)?;
        let mut lines = Vec::new();
        if set.kind != 0 {
            lines.push((phrase("kind", lang), set_kind_label(set.kind, lang), String::new(), String::new()));
        }
        if set.pieces > 0 {
            lines.push((phrase("set-size", lang), set.pieces.to_string(), String::new(), String::new()));
        }
        let slots = self.set_slot_line(code, lang);
        if !slots.is_empty() {
            lines.push((phrase("slots", lang), slots, String::new(), String::new()));
        }
        lines.extend(self.set_breakdown(code, lang));
        Some((
            set_title_lang(lang, &set.ko, &set.en, &set.zh),
            String::new(),
            format!("#{code}"),
            lines,
            String::new(),
        ))
    }

    fn open_set(&self, code: i64, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let Some((name, alt, id, lines, _)) = self.set_sheet(code, lang) else {
            return (phrase("missing-set", lang), Vec::new(), None);
        };
        let mut text = name.clone();
        if !alt.is_empty() {
            text.push_str(&format!("\n{alt}"));
        }
        text.push_str(&format!("\n{id}"));
        for (label, value, _, _) in &lines {
            text.push_str(&format!("\n{label}  {value}"));
        }
        let icon = self.index.sets.iter().find(|set| set.code == code).and_then(|set| set.icon.clone());
        (text, self.set_piece_hits(code, lang), icon)
    }

    fn set_member_slots(&self, code: i64) -> Vec<(String, i64, i64)> {
        if !self.has_table("item_set_members") {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT m.item_code, m.enhance, IFNULL(i.category, ''), IFNULL(i.name_ko, ''),
                    IFNULL(i.name_gts, 0), IFNULL(i.large_image, '')
             FROM item_set_members m
             LEFT JOIN items i ON i.code = m.item_code
             WHERE m.set_code = ?1",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let mut rows = stmt
            .query_map([code], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        rows.sort_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));
        let extras = load_set_extra();
        let mut grouped: HashMap<String, Vec<(i64, i64, bool, bool)>> = HashMap::new();
        for (item, enhance, category, name, name_gts, image) in rows {
            let slot = piece_slot(&category, &name);
            let shell = name_gts <= 0 && image.is_empty();
            let extra = set_extra_name(&name);
            grouped.entry(slot).or_default().push((item, enhance, shell, extra));
        }
        let mut kept = Vec::new();
        for (slot, items) in grouped {
            let named = items.iter().any(|(_, _, shell, extra)| !*shell && !*extra);
            for (item, enhance, shell, extra) in items {
                if extra && !extras {
                    continue;
                }
                if shell && named {
                    continue;
                }
                kept.push((slot.clone(), item, enhance));
            }
        }
        kept.sort_by(|left, right| set_slot_order(&left.0).cmp(&set_slot_order(&right.0)).then(left.2.cmp(&right.2)).then(left.1.cmp(&right.1)));
        kept
    }

    fn set_slot_line(&self, code: i64, lang: &str) -> String {
        let mut slots: Vec<String> = self.set_member_slots(code).into_iter().map(|(slot, _, _)| slot).collect();
        slots.sort_by_key(|slot| set_slot_order(slot));
        slots.dedup();
        slots
            .into_iter()
            .map(|slot| set_slot_label(&slot, lang))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn set_piece_hits(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut grouped: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
        for (slot, item, enhance) in self.set_member_slots(code) {
            grouped.entry(slot).or_default().push((item, enhance));
        }
        let mut slots: Vec<String> = grouped.keys().cloned().collect();
        slots.sort_by_key(|slot| set_slot_order(slot));
        let mut hits = Vec::new();
        for slot in slots {
            let Some(items) = grouped.remove(&slot) else { continue };
            hits.push(Hit {
                title: set_slot_label(&slot, lang),
                subtitle: items.len().to_string(),
                key: format!("section:{slot}"),
                icon: items.first().and_then(|(item, _)| self.item_icon(*item)),
                solo: false,
                aside: String::new(),
                badge: None,
                chip: "section".into(),
            });
            for (item, enhance) in items {
                hits.push(Hit {
                    title: self.item_name(item, lang),
                    subtitle: format!("#{item}"),
                    key: format!("item:{item}"),
                    icon: self.item_icon(item),
                    solo: false,
                    aside: if enhance > 0 { format!("+{enhance}") } else { String::new() },
                    badge: None,
                    chip: String::new(),
                });
            }
        }
        hits
    }

    fn enhance_lines(&self, code: i64, lang: &str) -> Vec<(String, String)> {
        if !self.has_table("enhance") {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT plus, dmin, dmax, move, atk_speed, magic_resist, hit_resist, element, element_damage,
                    attack_range, skill_range, crit, mana, sight, air, ground, cost,
                    merc_def, merc_atk, spell, reflect, invuln
             FROM enhance
             WHERE base_key = (SELECT base_key FROM enhance WHERE item_code = ?1 LIMIT 1)
             ORDER BY plus, item_code",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt
            .query_map([code], |row| {
                Ok(EnhanceRow {
                    plus: row.get(0)?,
                    dmin: row.get(1)?,
                    dmax: row.get(2)?,
                    move_speed: row.get(3)?,
                    atk_speed: row.get(4)?,
                    magic_resist: row.get(5)?,
                    hit_resist: row.get(6)?,
                    element: row.get(7)?,
                    element_damage: row.get(8)?,
                    attack_range: row.get(9)?,
                    skill_range: row.get(10)?,
                    crit: row.get(11)?,
                    mana: row.get(12)?,
                    sight: row.get(13)?,
                    air: row.get(14)?,
                    ground: row.get(15)?,
                    cost: row.get(16)?,
                    merc_def: row.get(17)?,
                    merc_atk: row.get(18)?,
                    spell: row.get(19)?,
                    reflect: row.get(20)?,
                    invuln: row.get(21)?,
                })
            })
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        if rows.is_empty() || rows.iter().all(|row| row.is_blank()) {
            return Vec::new();
        }
        rows.into_iter()
            .enumerate()
            .map(|(index, row)| {
                let label = if index == 0 { phrase("enhance", lang) } else { String::new() };
                (label, row.summary(lang))
            })
            .collect()
    }

    fn item_droppers(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT m.code, m.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''), MIN(d.slot),
                        IFNULL(pe.text, ''), IFNULL(pz.text, '')
                 FROM monster_drops d
                 JOIN monsters m ON m.code = d.monster_code
                 LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                    AND m.name_gts > 0 AND se.id = printf('%08d', m.name_gts)
                 LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                    AND m.name_gts > 0 AND sz.id = printf('%08d', m.name_gts)
                 LEFT JOIN monster_locations ml ON ml.monster_code = m.code
                    AND ml.location_gts = (
                        SELECT MIN(location_gts) FROM monster_locations WHERE monster_code = m.code
                    )
                 LEFT JOIN strings pe ON pe.object = 'OnlineText.dat' AND pe.lang = 'en'
                    AND pe.id = printf('%08d', ml.location_gts)
                 LEFT JOIN strings pz ON pz.object = 'OnlineText.dat' AND pz.lang = 'zh'
                    AND pz.id = printf('%08d', ml.location_gts)
                 WHERE d.item_code = ?1
                 GROUP BY m.code",
            )
            .expect("item droppers");
        let mut rows: Vec<Hit> = stmt
            .query_map([code], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .expect("dropper rows")
            .filter_map(Result::ok)
            .map(|(id, en, zh, slot, place_en, place_zh)| {
                let solo = slot >= 9;
                let place = lead(lang, &place_en, &place_zh);
                let mut meta = if place.is_empty() { id.clone() } else { format!("{place}  {id}") };
                if solo {
                    meta.push_str(&format!("  {}", phrase("solo-drop", lang)));
                }
                Hit {
                    title: lead(lang, &en, &zh),
                    subtitle: caption(&trail(lang, &en, &zh), meta),
                    key: format!("monster:{id}"),
                    icon: self.sprite(&id),
                    solo,
                    aside: String::new(),
                badge: None,
                    chip: String::new(),
                }
            })
            .collect();
        rows.sort_by(|a, b| a.title.cmp(&b.title));
        rows
    }

    fn item_sellers(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        for town in &self.index.towns {
            let Some(good) = town.goods.iter().find(|good| good.item == code) else { continue };
            hits.push(Hit {
                title: lead(lang, &town.en, &town.zh),
                subtitle: caption(&trail(lang, &town.en, &town.zh), self.realm_name(&town.region, lang)),
                key: format!("town:{}", town.code),
                icon: None,
                solo: false,
                aside: if good.specialty { phrase("specialty", lang) } else { String::new() },
                badge: None,
                chip: String::new(),
            });
        }
        hits.sort_by(|a, b| b.aside.len().cmp(&a.aside.len()).then(a.title.cmp(&b.title)));
        hits
    }

    fn open_chart(&self, code: i64, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
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

    fn item_relations(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        self.push_section(&mut hits, lang, "sold-in", self.item_sellers(code, lang));
        self.push_section(&mut hits, lang, "dropped-by", self.item_droppers(code, lang));
        self.push_section(&mut hits, lang, "craft-how", self.recipes_for(code, lang));
        self.push_section(&mut hits, lang, "make", self.production_for(code, lang));
        let mut uses = self.item_quest_uses(code, lang);
        uses.extend(self.recipes_using(code, lang));
        self.push_section(&mut hits, lang, "used-for", uses);
        self.push_section(&mut hits, lang, "also-from", self.item_quest_gains(code, lang));
        self.push_section(&mut hits, lang, "set", self.item_set_links(code, lang));
        self.push_section(&mut hits, lang, "roster", self.index.roster_hits(code, lang));
        hits
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

    fn production_for(&self, code: i64, lang: &str) -> Vec<Hit> {
        let formula = self.index.formulas.get(&code);
        let mut places = self.factory_places(code, lang);
        if places.is_empty() {
            if let Some(formula) = formula {
                if let Some(place) = self.inferred_place(formula, lang) {
                    places.push(place);
                }
            }
        }
        let mut hits = Vec::new();
        for place in &places {
            hits.push(Hit {
                title: place.facility.clone(),
                subtitle: place.building.clone(),
                key: place.key.clone(),
                icon: place.icon.clone(),
                solo: false,
                aside: place.region.clone(),
                badge: None,
                chip: String::new(),
            });
        }
        let Some(formula) = formula else { return hits };
        if formula.labor > 0 || formula.yield_n > 0 {
            let mut yield_text = String::new();
            if formula.yield_n > 0 {
                yield_text = format!("{} {}", phrase("yield", lang), commas(formula.yield_n));
                if formula.yield_max > formula.yield_n {
                    yield_text.push_str(&format!("–{}", commas(formula.yield_max)));
                }
            }
            hits.push(Hit {
                title: if formula.labor > 0 {
                    format!("{} {}", phrase("labor", lang), commas(formula.labor))
                } else {
                    yield_text.clone()
                },
                subtitle: if formula.labor > 0 { yield_text } else { String::new() },
                key: String::new(),
                icon: None,
                solo: false,
                aside: String::new(),
                badge: None,
                chip: String::new(),
            });
        }
        for part in &formula.parts {
            let tool = part.kind == "tool";
            hits.push(Hit {
                title: self.item_name(part.item, lang),
                subtitle: if part.qty > 0 { format!("× {}", commas(part.qty)) } else { String::new() },
                key: format!("item:{}", part.item),
                icon: self.item_icon(part.item),
                solo: false,
                aside: if tool { phrase("tool", lang) } else { String::new() },
                badge: None,
                chip: String::new(),
            });
        }
        hits
    }

    fn factory_places(&self, code: i64, lang: &str) -> Vec<MakePlace> {
        let mut places = Vec::new();
        for building in &self.index.works {
            let building_name = lead(lang, &building.en, &building.zh);
            for facility in &building.facilities {
                let Some(yard) = self.index.yards.get(facility) else { continue };
                let Some(product) = yard.products.iter().find(|product| product.item == code) else { continue };
                let facility_name = lead(lang, &yard.en, &yard.zh);
                places.push(MakePlace {
                    region: work_lands(&product.nations, lang),
                    building: building_name.clone(),
                    facility: if facility_name.is_empty() { yard.code.clone() } else { facility_name },
                    key: format!("bay:{}", yard.code),
                    icon: yard.icon.clone(),
                });
            }
        }
        places
    }

    fn inferred_place(&self, formula: &ItemFormula, lang: &str) -> Option<MakePlace> {
        if formula.plant.is_empty() {
            return None;
        }
        let yard = self.index.yards.get(&formula.plant)?;
        let building = self.index.works.iter().find(|row| row.facilities.iter().any(|code| code == &yard.code));
        let facility = lead(lang, &yard.en, &yard.zh);
        Some(MakePlace {
            region: formula_lands(formula.country, lang),
            building: building.map(|row| lead(lang, &row.en, &row.zh)).unwrap_or_default(),
            facility: if facility.is_empty() { yard.code.clone() } else { facility },
            key: format!("bay:{}", yard.code),
            icon: yard.icon.clone(),
        })
    }

    fn item_quest_uses(&self, code: i64, lang: &str) -> Vec<Hit> {
        self.item_quests(code, lang, &["requirement", "objective"], "use-quest")
    }

    fn item_quest_gains(&self, code: i64, lang: &str) -> Vec<Hit> {
        self.item_quests(code, lang, &["reward"], "")
    }

    fn item_quests(&self, code: i64, lang: &str, roles: &[&str], tag: &str) -> Vec<Hit> {
        let mut stmt = match self.conn.prepare(
            "SELECT quest_id, role FROM quest_steps
             WHERE lower(kind) = 'item' AND code = ?1
             ORDER BY quest_id",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let wanted: Vec<String> = roles.iter().map(|role| role.to_string()).collect();
        let rows = stmt
            .query_map([code.to_string()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut seen = HashSet::new();
        let mut hits = Vec::new();
        for (id, role) in rows {
            if !wanted.iter().any(|wanted| wanted.eq_ignore_ascii_case(&role)) || !seen.insert(id) {
                continue;
            }
            let tag = if tag.is_empty() { String::new() } else { phrase(tag, lang) };
            if let Some(hit) = self.quest_link_hit(id, lang, &tag) {
                hits.push(hit);
            }
        }
        hits
    }

    fn recipes_using(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut stmt = match self.conn.prepare(
            "SELECT r.id, r.source, r.result_item, m.qty
             FROM recipe_materials m
             JOIN recipes r ON r.id = m.recipe_id
             WHERE m.item_code = ?1
             ORDER BY r.id",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([code], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
        })
        .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .take(24)
        .map(|(id, source, result, qty)| Hit {
            title: self.item_name(result, lang),
            subtitle: caption(&label(&source, lang), format!("× {}", commas(qty))),
            key: format!("recipe:{id}"),
            icon: self.item_icon(result),
            solo: false,
            aside: phrase("use-craft", lang),
            badge: None,
            chip: String::new(),
        })
        .collect()
    }

    fn open_recipe(&self, id: i64, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let row = self.conn.query_row(
            "SELECT source, result_item, result_qty, fee, learn_fee FROM recipes WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        );
        let Ok((source, result, qty, fee, learn)) = row else {
            return (phrase("missing-recipe", lang), Vec::new(), None);
        };
        let name = self.item_name(result, lang);
        let mut text = format!(
            "{name}\n{}  {} {}\n{} {}",
            label(&source, lang),
            phrase("qty", lang),
            commas(qty),
            phrase("fee", lang),
            commas(fee)
        );
        if let Some(learn) = learn.filter(|value| *value > 0) {
            text.push_str(&format!("\n{} {}", phrase("learn", lang), commas(learn)));
        }
        text.push_str(&format!("\n\n{}\n", phrase("materials", lang)));
        text.push_str(&self.material_tree(id, 3, &mut vec![id], lang));
        let links = self.recipe_material_hits(id, lang);
        let icon = self.item_icon(result);
        (text, links, icon)
    }

    pub fn recipe_sheet(&self, id: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String)>)> {
        let row = self.conn.query_row(
            "SELECT source, result_item, result_qty, fee, learn_fee, level_min, level_max, success_rate
             FROM recipes WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            },
        );
        let Ok((source, result, qty, fee, learn, level_min, _level_max, success)) = row else {
            return None;
        };
        let mut lines = vec![
            (phrase("kind", lang), label(&source, lang)),
            (phrase("code", lang), result.to_string()),
            (phrase("qty", lang), commas(qty)),
            (phrase("fee", lang), commas(fee)),
        ];
        if let Some(learn) = learn.filter(|value| *value > 0) {
            lines.push((phrase("learn", lang), commas(learn)));
        }
        if let Some(level) = level_min.filter(|value| *value > 0) {
            lines.push((phrase("min-level", lang), commas(level)));
        }
        if let Some(rate) = success.filter(|value| *value > 0) {
            lines.push((phrase("success", lang), format!("{}%", commas(rate))));
        }
        let (en, zh) = self.item_pair(result);
        Some((lead(lang, &en, &zh), trail(lang, &en, &zh), format!("#{result}"), lines))
    }

    fn material_tree(&self, recipe_id: i64, depth: usize, seen: &mut Vec<i64>, lang: &str) -> String {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT item_code, qty FROM recipe_materials WHERE recipe_id = ?1 ORDER BY slot",
            )
            .expect("materials");
        let rows: Vec<(i64, i64)> = stmt
            .query_map([recipe_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("material rows")
            .filter_map(Result::ok)
            .collect();
        let mut out = String::new();
        let indent = "  ".repeat(3 - depth.min(3));
        for (code, qty) in rows {
            let name = self.item_name(code, lang);
            out.push_str(&format!("{indent}{name} × {}\n", commas(qty)));
            if depth > 0 {
                if let Some(child) = self.preferred_recipe(code, Some(recipe_id)) {
                    if !seen.contains(&child) {
                        seen.push(child);
                        out.push_str(&self.material_tree(child, depth - 1, seen, lang));
                    }
                }
            }
        }
        out
    }

    fn recipe_material_hits(&self, recipe_id: i64, lang: &str) -> Vec<Hit> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_code, qty, enhance FROM recipe_materials WHERE recipe_id = ?1 ORDER BY slot")
            .expect("link materials");
        stmt.query_map([recipe_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        })
        .expect("link rows")
        .filter_map(Result::ok)
        .map(|(code, qty, enhance)| {
            let child = self.preferred_recipe(code, Some(recipe_id));
            let mut subtitle = format!("× {}", commas(qty));
            if enhance > 0 {
                subtitle.push_str(&format!("  +{}", commas(enhance)));
            }
            if let Some(child) = child {
                subtitle.push_str(&format!("  {}", label(&self.recipe_source(child), lang)));
            }
            Hit {
                title: self.item_name(code, lang),
                subtitle,
                key: match child {
                    Some(id) => format!("recipe:{id}"),
                    None => format!("item:{code}"),
                },
                icon: self.item_icon(code),
                solo: false,
                aside: String::new(),
                badge: None,
                    chip: String::new(),
            }
        })
        .collect()
    }

    fn recipes_for(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source FROM recipes WHERE result_item = ?1")
            .expect("makers");
        let rows: Vec<(i64, String)> = stmt
            .query_map([code], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("maker rows")
            .filter_map(Result::ok)
            .collect();
        let mut best: HashMap<Vec<(i64, i64)>, (i64, String, usize)> = HashMap::new();
        for (id, source) in rows {
            let mut signature = self.recipe_signature(id);
            if signature.is_empty() {
                signature.push((-id, 0));
            }
            let rank = pref_rank(&source);
            match best.get(&signature) {
                Some((_, _, old)) if *old <= rank => {}
                _ => {
                    best.insert(signature, (id, source, rank));
                }
            }
        }
        let mut kept: Vec<(i64, String, usize)> = best.into_values().collect();
        kept.sort_by_key(|(_, _, rank)| *rank);
        kept.into_iter()
            .take(12)
            .map(|(id, source, _)| Hit {
                title: label(&source, lang),
                subtitle: phrase("open-recipe", lang),
                key: format!("recipe:{id}"),
                icon: self.item_icon(code),
                    solo: false,
                    aside: String::new(),
                badge: None,
                    chip: String::new(),
            })
            .collect()
    }

    fn recipe_signature(&self, id: i64) -> Vec<(i64, i64)> {
        let mut stmt = match self.conn.prepare(
            "SELECT item_code, qty FROM recipe_materials WHERE recipe_id = ?1 ORDER BY item_code, qty",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    fn preferred_recipe(&self, code: i64, skip: Option<i64>) -> Option<i64> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source FROM recipes WHERE result_item = ?1")
            .ok()?;
        let mut rows: Vec<(i64, String)> = stmt
            .query_map([code], |row| Ok((row.get(0)?, row.get(1)?)))
            .ok()?
            .filter_map(Result::ok)
            .filter(|(id, _)| Some(*id) != skip)
            .collect();
        rows.sort_by_key(|(_, source)| pref_rank(source));
        rows.into_iter().next().map(|(id, _)| id)
    }

    fn recipe_source(&self, id: i64) -> String {
        self.conn
            .query_row("SELECT source FROM recipes WHERE id = ?1", [id], |row| row.get(0))
            .unwrap_or_default()
    }

    fn monster_text(&self, code: &str, lang: &str) -> String {
        let row = self.conn.query_row(
            "SELECT m.code, m.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''),
                    m.hp, m.mp, m.hit_resist, m.magic_resist, m.atk_min, m.atk_max, m.exp,
                    m.element, m.element_gts, m.is_boss, m.unit_type
             FROM monsters m
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND m.name_gts > 0 AND se.id = printf('%08d', m.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND m.name_gts > 0 AND sz.id = printf('%08d', m.name_gts)
             WHERE m.code = ?1",
            [code],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    pick(lang, &row.get::<_, String>(1)?, &row.get::<_, String>(2)?, &row.get::<_, String>(3)?),
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, i64>(14)?,
                ))
            },
        );
        let Ok((id, name, hp, mp, hit, magic, atk_min, atk_max, exp, element, ela, boss, unit)) = row else {
            return phrase("missing-monster", lang);
        };
        let lines = self.monster_lines(code, lang, hp, mp, hit, magic, atk_min, atk_max, exp, element, ela, boss, unit);
        let mut text = format!("{name}\n{id}");
        for (label, value) in lines {
            if label.is_empty() {
                text.push_str(&format!("\n{value}"));
            } else {
                text.push_str(&format!("\n{label}  {value}"));
            }
        }
        text
    }

    pub fn monster_sheet(&self, code: &str, lang: &str) -> Option<(String, String, String, Vec<(String, String)>)> {
        let row = self.conn.query_row(
            "SELECT m.code, m.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''),
                    m.hp, m.mp, m.hit_resist, m.magic_resist, m.atk_min, m.atk_max, m.exp,
                    m.element, m.element_gts, m.is_boss, m.unit_type
             FROM monsters m
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND m.name_gts > 0 AND se.id = printf('%08d', m.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND m.name_gts > 0 AND sz.id = printf('%08d', m.name_gts)
             WHERE m.code = ?1",
            [code],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, i64>(14)?,
                ))
            },
        );
        let Ok((id, en, zh, hp, mp, hit, magic, atk_min, atk_max, exp, element, ela, boss, unit)) = row else {
            return None;
        };
        Some((
            lead(lang, &en, &zh),
            trail(lang, &en, &zh),
            id.clone(),
            self.monster_lines(&id, lang, hp, mp, hit, magic, atk_min, atk_max, exp, element, ela, boss, unit),
        ))
    }

    fn monster_lines(
        &self,
        code: &str,
        lang: &str,
        hp: i64,
        mp: i64,
        hit: i64,
        magic: i64,
        atk_min: i64,
        atk_max: i64,
        exp: i64,
        element: i64,
        ela: i64,
        boss: i64,
        unit: i64,
    ) -> Vec<(String, String)> {
        let kind = if boss == 1 { phrase("boss", lang) } else { phrase("normal", lang) };
        let unit_name = if unit == 1 { phrase("air", lang) } else { phrase("ground", lang) };
        let mut lines = vec![
            (phrase("kind", lang), kind),
            (phrase("unit", lang), unit_name),
            (phrase("element", lang), element_name(element, lang)),
        ];
        if ela > 0 {
            lines.push((phrase("ela", lang), commas(ela)));
        }
        lines.extend([
            (phrase("hp", lang), commas(hp)),
            (phrase("mp", lang), commas(mp)),
            (phrase("hit-resist", lang), commas(hit)),
            (phrase("magic-resist", lang), commas(magic)),
            (phrase("atk", lang), format!("{}-{}", commas(atk_min), commas(atk_max))),
            (phrase("exp", lang), commas(exp)),
        ]);
        let places = self.place_names(code, lang);
        if places.is_empty() {
            lines.push((phrase("spawn-place", lang), "—".into()));
        } else {
            for (index, place) in places.into_iter().enumerate() {
                let label = if index == 0 { phrase("spawn-place", lang) } else { String::new() };
                lines.push((label, place));
            }
        }
        lines
    }
}

impl Catalog {
    fn place_names(&self, code: &str, lang: &str) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT IFNULL(lz.text, ''), IFNULL(le.text, '')
                 FROM monster_locations ml
                 LEFT JOIN strings lz ON lz.object = 'OnlineText.dat' AND lz.lang = 'zh'
                    AND lz.id = printf('%08d', ml.location_gts)
                 LEFT JOIN strings le ON le.object = 'OnlineText.dat' AND le.lang = 'en'
                    AND le.id = printf('%08d', ml.location_gts)
                 WHERE ml.monster_code = ?1",
            )
            .expect("monster places");
        stmt.query_map([code], |row| Ok(pick_place(lang, &row.get::<_, String>(1)?, &row.get::<_, String>(0)?)))
            .expect("place rows")
            .filter_map(Result::ok)
            .filter(|name| !name.is_empty())
            .collect()
    }

    fn monster_drops(&self, code: &str, lang: &str) -> Vec<Hit> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT d.slot, d.item_code, i.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, ''),
                        i.large_image, i.large_frame
                 FROM monster_drops d
                 JOIN items i ON i.code = d.item_code
                 LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                    AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
                 LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                    AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
                 WHERE d.monster_code = ?1
                 ORDER BY d.slot",
            )
            .expect("drops");
        let rows: Vec<(i64, i64, String, String, String, i64)> = stmt
            .query_map([code], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })
            .expect("drop rows")
            .filter_map(Result::ok)
            .collect();
        rows.into_iter()
            .map(|(slot, item, en, zh, image, frame)| {
                let solo = slot >= 9;
                let name = lead(lang, &en, &zh);
                Hit {
                    title: if solo {
                        format!("{name}  {}", phrase("solo-drop", lang))
                    } else {
                        name
                    },
                    subtitle: caption(&trail(lang, &en, &zh), format!("#{item}")),
                    key: format!("item:{item}"),
                    icon: self.icon_path(&image, frame),
                    solo,
                    aside: String::new(),
                badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    pub fn monster_quests(&self, code: &str, lang: &str) -> Vec<Hit> {
        let code = code.trim();
        if code.is_empty() {
            return Vec::new();
        }
        let mut stmt = match self.conn.prepare(
            "SELECT s.quest_id, lower(s.kind), s.code, s.qty
             FROM quest_steps s
             WHERE s.role IN ('objective', 'requirement')
               AND (
                    (lower(s.kind) = 'monster' AND (',' || replace(s.code, ' ', '') || ',') LIKE '%,' || ?1 || ',%')
                    OR (lower(s.kind) = 'item' AND s.code IN (
                        SELECT CAST(item_code AS TEXT) FROM monster_drops WHERE monster_code = ?1
                    ))
               )
             ORDER BY s.quest_id, s.slot",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt
            .query_map([code], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut order = Vec::new();
        let mut reasons: HashMap<i64, (String, i64, String)> = HashMap::new();
        for (id, kind, step_code, qty) in rows {
            let entry = reasons.entry(id).or_insert_with(|| {
                order.push(id);
                (String::new(), 0, String::new())
            });
            if kind == "monster" && entry.0.is_empty() {
                entry.0 = qty;
            } else if kind == "item" && entry.1 == 0 {
                if let Ok(item) = step_code.parse::<i64>() {
                    entry.1 = item;
                    entry.2 = qty;
                }
            }
        }
        order.sort_by_key(|id| {
            let quest = self.index.quests.iter().find(|quest| quest.id == *id);
            (
                quest.map(|quest| quest_source_rank(&quest.source)).unwrap_or(9),
                quest.map(|quest| quest.code.clone()).unwrap_or_default(),
            )
        });
        order
            .into_iter()
            .filter_map(|id| {
                let (kill_qty, item, item_qty) = reasons.get(&id)?;
                let aside = if !kill_qty.is_empty() && kill_qty != "0" {
                    format!("× {}", commas_text(kill_qty))
                } else if *item > 0 {
                    let name = self.named_item(*item, lang, "");
                    if item_qty.is_empty() || item_qty == "0" || name.is_empty() {
                        name
                    } else {
                        format!("{name} × {}", commas_text(item_qty))
                    }
                } else {
                    String::new()
                };
                let mut hit = self.quest_link_hit(id, lang, &aside)?;
                hit.icon = self
                    .index
                    .quests
                    .iter()
                    .find(|quest| quest.id == id)
                    .and_then(|quest| self.index.npc_face(quest.npc));
                Some(hit)
            })
            .collect()
    }

    fn quest_text(&self, id: i64, lang: &str) -> String {
        let row = self.conn.query_row(
            "SELECT source, code, title, detail, prev_code FROM quests WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        );
        let Ok((source, code, title, detail, prev)) = row else {
            return phrase("missing-quest", lang);
        };
        let heading = self.quest_title(id, &title, &code, lang);
        let body = self.quest_body(id, &detail, lang);
        let mut text = format!("{heading}\n{}  {code}", label(&source, lang));
        if !body.is_empty() {
            text.push_str(&format!("\n{body}"));
        }
        if !prev.is_empty() && prev != "0" {
            text.push_str(&format!("\n{} {prev}", phrase("prev", lang)));
        }
        let mut stmt = self
            .conn
            .prepare("SELECT role, kind, code, qty, label FROM quest_steps WHERE quest_id = ?1 ORDER BY slot")
            .expect("steps");
        let steps: Vec<(String, String, String, String, String)> = stmt
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
            .expect("step rows")
            .filter_map(Result::ok)
            .collect();
        for (role, heading) in [
            ("requirement", phrase("req", lang)),
            ("objective", phrase("done", lang)),
            ("reward", phrase("reward", lang)),
        ] {
            let mine: Vec<_> = steps.iter().filter(|step| step.0 == role).collect();
            if mine.is_empty() {
                continue;
            }
            text.push_str(&format!("\n\n{heading}"));
            for (_, kind, code, qty, name) in mine {
                let shown = if code.chars().all(|ch| ch.is_ascii_digit()) && !code.is_empty() {
                    self.item_name(code.parse().unwrap_or(0), lang)
                } else if !name.is_empty() {
                    format!("{name} ({code})")
                } else {
                    code.clone()
                };
                text.push_str(&format!("\n{} {shown} × {}", kind_name(kind, lang), commas_text(qty)));
            }
        }
        text
    }

    fn item_pair(&self, code: i64) -> (String, String) {
        self.conn
            .query_row(
                "SELECT IFNULL(se.text, ''), IFNULL(sz.text, '')
                 FROM items i
                 LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                    AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
                 LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                    AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
                 WHERE i.code = ?1",
                [code],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap_or_default()
    }

    fn item_name(&self, code: i64, lang: &str) -> String {
        self.item_name_linked(code, lang, true)
    }

    fn item_name_linked(&self, code: i64, lang: &str, follow_base: bool) -> String {
        let found = self.conn.query_row(
            "SELECT i.name_ko, IFNULL(se.text, ''), IFNULL(sz.text, '')
             FROM items i
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE i.code = ?1",
            [code],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        );
        let Ok((ko, en, zh)) = found else {
            return code.to_string();
        };
        let shown = lead(lang, &en, &zh);
        if !shown.is_empty() {
            return shown;
        }
        if follow_base {
            if let Some((base, prefix_gts)) = self.base_link(code) {
                let base_name = self.item_name_linked(base, lang, false);
                let base_ko = self.conn
                    .query_row("SELECT name_ko FROM items WHERE code = ?1", [base], |row| row.get::<_, String>(0))
                    .unwrap_or_default();
                let prefix = self.prefix_name(prefix_gts, &ko, &base_ko, lang);
                if !base_name.is_empty() {
                    return if prefix.is_empty() { base_name } else { format!("{prefix} {base_name}") };
                }
            }
        }
        if ko.is_empty() { code.to_string() } else { ko }
    }

    fn base_link(&self, code: i64) -> Option<(i64, i64)> {
        if !self.has_table("item_bases") {
            return None;
        }
        let prefixed = self.conn.query_row(
            "SELECT 1 FROM pragma_table_info('item_bases') WHERE name = 'prefix_gts'",
            [],
            |row| row.get::<_, i64>(0),
        ).is_ok();
        if prefixed {
            self.conn
                .query_row(
                    "SELECT base_code, IFNULL(prefix_gts, 0) FROM item_bases WHERE item_code = ?1",
                    [code],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .ok()
        } else {
            self.conn
                .query_row("SELECT base_code FROM item_bases WHERE item_code = ?1", [code], |row| {
                    Ok((row.get::<_, i64>(0)?, 0))
                })
                .ok()
        }
    }

    fn prefix_name(&self, prefix_gts: i64, ko: &str, base_ko: &str, lang: &str) -> String {
        if prefix_gts > 0 {
            let en = self.online_line(prefix_gts, "en");
            let zh = self.online_line(prefix_gts, "zh");
            let shown = lead(lang, &en, &zh);
            if !shown.is_empty() {
                return shown;
            }
        }
        piece_prefix(ko, base_ko, lang)
    }

    fn quest_title(&self, id: i64, _stored: &str, code: &str, lang: &str) -> String {
        let (en, zh) = self.quest_names(id);
        let shown = stack(lang, &en, &zh);
        if shown.is_empty() { code.to_string() } else { shown }
    }

    pub fn hero_sheet(&self, code: &str, lang: &str) -> Option<(String, String, String, Vec<(String, String)>, String)> {
        let hero = self.index.heroes.iter().find(|hero| hero.code == code)?;
        Some(hero_sheet_parts(hero, lang))
    }

    pub fn work_building_name(&self, code: &str, lang: &str) -> String {
        self.index.work_building_name(code, lang)
    }

    pub fn work_facility_name(&self, code: &str, lang: &str) -> String {
        self.index.work_facility_name(code, lang)
    }

    pub fn work_parent(&self, facility: &str) -> String {
        self.index.work_parent(facility)
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

    pub fn hero_mark(&self) -> Option<(PathBuf, i32)> {
        self.index.heroes.iter().find(|hero| hero.code == "U25").and_then(|hero| hero.icon.clone())
    }

    fn open_hero(&self, code: &str, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let Some(hero) = self.index.heroes.iter().find(|hero| hero.code == code) else {
            return (phrase("missing-hero", lang), Vec::new(), None);
        };
        let (name, alt, heading, facts, note) = hero_sheet_parts(hero, lang);
        let links = self.index.hero_chain(code, lang);
        let mut text = name.clone();
        if !alt.is_empty() {
            text.push_str(&format!("\n{alt}"));
        }
        if !heading.is_empty() {
            text.push_str(&format!("\n{heading}"));
        }
        for (label, value) in &facts {
            text.push_str(&format!("\n{label}  {value}"));
        }
        if !note.is_empty() {
            text.push_str(&format!("\n{note}"));
        }
        (text, links, hero.icon.clone())
    }

    pub fn npc_sheet(&self, code: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String)>, String)> {
        let npc = self.index.npcs.iter().find(|npc| npc.code == code)?;
        Some(npc_sheet_parts(npc, lang))
    }

    fn open_npc(&self, code: i64, lang: &str) -> (String, Vec<Hit>, Option<(PathBuf, i32)>) {
        let Some(npc) = self.index.npcs.iter().find(|npc| npc.code == code) else {
            return (phrase("missing-npc", lang), Vec::new(), None);
        };
        let (name, alt, map, facts, note) = npc_sheet_parts(npc, lang);
        let mut text = name.clone();
        if !alt.is_empty() {
            text.push_str(&format!("\n{alt}"));
        }
        if !map.is_empty() {
            text.push_str(&format!("\n{map}"));
        }
        for (label, value) in &facts {
            text.push_str(&format!("\n{label}  {value}"));
        }
        if !note.is_empty() {
            text.push_str(&format!("\n{note}"));
        }
        (text, self.quests_for_npc(code, lang), npc.icon.clone())
    }

    pub fn quest_sheet(&self, id: i64, lang: &str) -> Option<(String, String, String, Vec<(String, String, String)>, String)> {
        let (source, code, title, detail, kind, repeat) = self.quest_row(id)?;
        let (en, zh) = self.quest_names(id);
        let mut name = lead(lang, &en, &zh);
        if name.is_empty() {
            name = if title.is_empty() { code.clone() } else { title };
        }
        Some((
            name,
            trail(lang, &en, &zh),
            format!("{}  {code}", label(&source, lang)),
            self.quest_facts(id, lang, &kind, &repeat),
            self.quest_body(id, &detail, lang),
        ))
    }

    pub fn quest_reward_lines(&self, id: i64, lang: &str) -> Vec<(String, String)> {
        let source = self.quest_source(id);
        let mut lines = Vec::new();
        for step in self.quest_steps(id).into_iter().filter(|step| step.role == "reward") {
            if self.step_link(&source, &step, lang, &HashSet::new()).is_some() {
                continue;
            }
            let Some(value) = reward_amount(&step) else { continue };
            let label = kind_name(&step.kind, lang);
            if label.is_empty() {
                continue;
            }
            lines.push((label, value));
        }
        lines
    }

    pub fn quest_chain(&self, id: i64, lang: &str) -> (Option<Hit>, Vec<Hit>) {
        let Some((source, code, _title, _detail, _kind, _repeat, prev, next)) = self.quest_chain_row(id) else {
            return (None, Vec::new());
        };
        let prev_hit = self
            .quest_id_by_code(&source, &prev)
            .and_then(|prev_id| self.quest_link_hit(prev_id, lang, ""));
        let mut nexts = Vec::new();
        let mut seen = HashSet::new();
        if let Some(next_id) = self.quest_id_by_code(&source, &next) {
            if next_id != id {
                if let Some(hit) = self.quest_link_hit(next_id, lang, "") {
                    seen.insert(next_id);
                    nexts.push(hit);
                }
            }
        }
        for follow_id in self.follower_ids(&source, &code, id) {
            if follow_id != id && seen.insert(follow_id) {
                if let Some(hit) = self.quest_link_hit(follow_id, lang, "") {
                    nexts.push(hit);
                }
            }
        }
        (prev_hit, nexts)
    }

    pub fn quest_giver(&self, id: i64) -> String {
        self.index
            .quests
            .iter()
            .find(|quest| quest.id == id && quest.npc > 0)
            .map(|quest| format!("npc:{}", quest.npc))
            .unwrap_or_default()
    }

    fn quest_links(&self, id: i64, lang: &str) -> Vec<Hit> {
        let source = self.quest_source(id);
        let mut hits = Vec::new();
        for step in self.quest_steps(id).into_iter().filter(|step| step.role == "reward") {
            if let Some(mut hit) = self.step_link(&source, &step, lang, &HashSet::new()) {
                hit.aside.clear();
                hits.push(hit);
            }
        }
        hits
    }

    fn quest_npc_icon(&self, id: i64) -> Option<(PathBuf, i32)> {
        self.index
            .quests
            .iter()
            .find(|quest| quest.id == id)
            .and_then(|quest| self.index.npc_face(quest.npc))
    }

    fn quests_for_npc(&self, code: i64, lang: &str) -> Vec<Hit> {
        self.index
            .quests
            .iter()
            .filter(|quest| quest.npc == code)
            .map(|quest| {
                Hit {
                    title: lead(lang, &quest.title_en, &quest.title_zh),
                    subtitle: caption(
                        &trail(lang, &quest.title_en, &quest.title_zh),
                        format!("{}  {}", label(&quest.source, lang), quest.code),
                    ),
                    key: format!("quest:{}", quest.id),
                    icon: None,
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    fn quest_facts(&self, id: i64, lang: &str, kind: &str, repeat: &str) -> Vec<(String, String, String)> {
        let mut facts = Vec::new();
        if !kind.is_empty() && !kind.chars().all(|ch| ch.is_ascii_digit()) {
            facts.push((phrase("kind", lang), kind.to_string(), String::new()));
        }
        if !repeat.is_empty() && repeat != "0" {
            facts.push((phrase("repeat", lang), commas_text(repeat), String::new()));
        }
        let source = self.quest_source(id);
        let prev = self
            .quest_chain_row(id)
            .map(|row| row.6)
            .unwrap_or_default();
        let prev_known = self.quest_id_by_code(&source, &prev).is_some();
        for step in self.quest_steps(id) {
            if step.role != "requirement" && step.role != "objective" {
                continue;
            }
            if prev_known && step.kind.eq_ignore_ascii_case("quest") && step.code == prev {
                continue;
            }
            if let Some(fact) = self.task_fact(&source, &step, lang) {
                facts.push(fact);
            }
        }
        facts
    }

    fn task_fact(&self, source: &str, step: &QuestStep, lang: &str) -> Option<(String, String, String)> {
        let kind = step.kind.to_ascii_lowercase();
        if kind.is_empty() || kind == "0" {
            return None;
        }
        let bucket = match kind.as_str() {
            "monster" => "task-kill",
            "item" | "wearitem" | "useitem" | "book" => "task-item",
            "npc" => "task-talk",
            _ => "task-other",
        };
        let (value, key) = self.task_detail(source, step, lang, bucket)?;
        if value.is_empty() {
            return None;
        }
        Some((phrase(bucket, lang), value, key))
    }

    fn task_detail(&self, source: &str, step: &QuestStep, lang: &str, bucket: &str) -> Option<(String, String)> {
        let kind = step.kind.to_ascii_lowercase();
        match bucket {
            "task-kill" => {
                if step.code.is_empty() || step.code == "0" {
                    return None;
                }
                let name = self.named_monster(&step.code, lang, &step.label);
                Some((with_count(name, &step.qty), format!("monster:{}", step.code)))
            }
            "task-item" => {
                let code = step.code.parse::<i64>().ok().filter(|code| *code > 0);
                let name = match code {
                    Some(code) => self.named_item(code, lang, &step.label),
                    None => step_hint(&step.label),
                };
                if name.is_empty() {
                    return None;
                }
                let key = code.map(|code| format!("item:{code}")).unwrap_or_default();
                Some((with_count(name, &step.qty), key))
            }
            "task-talk" => {
                let code = step.code.parse::<i64>().ok().filter(|code| *code > 0)?;
                Some((self.named_npc(code, lang), format!("npc:{code}")))
            }
            _ if kind.starts_with("level") => numbered(kind_name(&step.kind, lang), &step.qty),
            _ if kind.starts_with("credit") => numbered(kind_name(&step.kind, lang), &step.qty),
            _ if kind == "quest" => {
                let quest_id = self.quest_id_by_code(source, &step.code)?;
                let hit = self.quest_link_hit(quest_id, lang, "")?;
                Some((hit.title, format!("quest:{quest_id}")))
            }
            _ => {
                let named = kind_name(&step.kind, lang);
                let amount = step_value(&step.code, &step.qty);
                let hint = step_hint(&step.label);
                let value = match (amount.is_empty(), hint.is_empty()) {
                    (true, true) => return None,
                    (false, true) => format!("{named} {amount}"),
                    (true, false) => format!("{named} {hint}"),
                    (false, false) => format!("{named} {hint} {amount}"),
                };
                Some((value, String::new()))
            }
        }
    }

    fn named_item(&self, code: i64, lang: &str, fallback: &str) -> String {
        let (en, zh) = self.item_pair(code);
        let title = lead(lang, &en, &zh);
        if !title.is_empty() {
            title
        } else {
            let hint = step_hint(fallback);
            if hint.is_empty() { code.to_string() } else { hint }
        }
    }

    fn named_monster(&self, code: &str, lang: &str, fallback: &str) -> String {
        let title = self
            .monster_pair(code)
            .map(|(en, zh)| lead(lang, &en, &zh))
            .unwrap_or_default();
        if !title.is_empty() {
            title
        } else {
            let hint = step_hint(fallback);
            if hint.is_empty() { code.to_string() } else { hint }
        }
    }

    fn named_npc(&self, code: i64, lang: &str) -> String {
        self.index
            .npc(code)
            .map(|npc| npc_names(lang, npc).0)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| code.to_string())
    }

    fn step_link(&self, source: &str, step: &QuestStep, lang: &str, seen: &HashSet<i64>) -> Option<Hit> {
        let aside = role_phrase(&step.role, lang);
        match step.kind.to_ascii_lowercase().as_str() {
            "item" => {
                let code = step.code.parse::<i64>().ok().filter(|code| *code > 0)?;
                let (en, zh) = self.item_pair(code);
                let (title, alt) = named_lead(lang, &en, &zh, &step.label, &step.code);
                Some(self.entity_hit(title, alt, &step.qty, format!("item:{code}"), self.item_icon(code), aside))
            }
            "monster" => {
                if step.code.is_empty() || step.code == "0" {
                    return None;
                }
                let (en, zh) = self.monster_pair(&step.code)?;
                let (title, alt) = named_lead(lang, &en, &zh, &step.label, &step.code);
                Some(self.entity_hit(title, alt, &step.qty, format!("monster:{}", step.code), self.sprite(&step.code), aside))
            }
            "quest" => {
                let quest_id = self.quest_id_by_code(source, &step.code)?;
                if seen.contains(&quest_id) {
                    return None;
                }
                self.quest_link_hit(quest_id, lang, &aside)
            }
            _ => None,
        }
    }

    fn entity_hit(
        &self,
        title: String,
        alt: String,
        qty: &str,
        key: String,
        icon: Option<(PathBuf, i32)>,
        aside: String,
    ) -> Hit {
        let amount = if qty.is_empty() || qty == "0" {
            String::new()
        } else {
            format!("× {}", commas_text(qty))
        };
        Hit {
            title,
            subtitle: caption(&alt, amount),
            key,
            icon,
            solo: false,
            aside,
            badge: None,
            chip: String::new(),
        }
    }

    fn quest_link_hit(&self, id: i64, lang: &str, aside: &str) -> Option<Hit> {
        let (code, stored) = self.conn.query_row(
            "SELECT code, title FROM quests WHERE id = ?1",
            [id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).ok()?;
        let heading = self.quest_title(id, &stored, &code, lang);
        let mut lines = heading.lines();
        let title = lines.next().unwrap_or(&code).to_string();
        let alt = lines.next().unwrap_or("").to_string();
        Some(Hit {
            title,
            subtitle: caption(&alt, code),
            key: format!("quest:{id}"),
            icon: None,
            solo: false,
            aside: aside.to_string(),
            badge: None,
            chip: String::new(),
        })
    }

    fn quest_names(&self, id: i64) -> (String, String) {
        (
            self.line_text(id, QUEST_TITLE_ROLES, "en").unwrap_or_default(),
            self.line_text(id, QUEST_TITLE_ROLES, "zh").unwrap_or_default(),
        )
    }

    fn quest_row(&self, id: i64) -> Option<(String, String, String, String, String, String)> {
        self.conn
            .query_row(
                "SELECT source, code, title, detail, kind, repeat_count FROM quests WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .ok()
    }

    fn quest_chain_row(&self, id: i64) -> Option<(String, String, String, String, String, String, String, String)> {
        self.conn
            .query_row(
                "SELECT source, code, title, detail, kind, repeat_count, prev_code, next_code FROM quests WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .ok()
    }

    fn quest_source(&self, id: i64) -> String {
        self.conn
            .query_row("SELECT source FROM quests WHERE id = ?1", [id], |row| row.get(0))
            .unwrap_or_default()
    }

    fn quest_id_by_code(&self, source: &str, code: &str) -> Option<i64> {
        if code.is_empty() || code == "0" {
            return None;
        }
        self.conn
            .query_row(
                "SELECT id FROM quests WHERE source = ?1 AND code = ?2",
                (source, code),
                |row| row.get(0),
            )
            .ok()
    }

    fn follower_ids(&self, source: &str, code: &str, self_id: i64) -> Vec<i64> {
        let mut stmt = match self.conn.prepare(
            "SELECT id FROM quests WHERE source = ?1 AND prev_code = ?2 AND id != ?3 ORDER BY CAST(code AS INTEGER), code",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map((source, code, self_id), |row| row.get(0))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    fn quest_steps(&self, id: i64) -> Vec<QuestStep> {
        let mut stmt = match self
            .conn
            .prepare("SELECT role, kind, code, qty, label FROM quest_steps WHERE quest_id = ?1 ORDER BY slot")
        {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([id], |row| {
            Ok(QuestStep {
                role: row.get(0)?,
                kind: row.get(1)?,
                code: row.get(2)?,
                qty: row.get(3)?,
                label: row.get(4)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    fn monster_pair(&self, code: &str) -> Option<(String, String)> {
        self.conn
            .query_row(
                "SELECT IFNULL(se.text, ''), IFNULL(sz.text, '')
                 FROM monsters m
                 LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                    AND m.name_gts > 0 AND se.id = printf('%08d', m.name_gts)
                 LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                    AND m.name_gts > 0 AND sz.id = printf('%08d', m.name_gts)
                 WHERE m.code = ?1",
                [code],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok()
    }

    fn quest_body(&self, id: i64, _stored: &str, lang: &str) -> String {
        let zh = self.line_text(id, &["detail", "explain", "info"], "zh").unwrap_or_default();
        let en = self.line_text(id, &["detail", "explain", "info"], "en").unwrap_or_default();
        let main = lead(lang, &en, &zh);
        let alt = trail(lang, &en, &zh);
        if alt.is_empty() { main } else { format!("{main}\n\n{alt}") }
    }

    fn line_text(&self, quest_id: i64, roles: &[&str], lang: &str) -> Option<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT role, gts_id, text_object FROM quest_lines WHERE quest_id = ?1 ORDER BY slot")
            .ok()?;
        let rows: Vec<(String, i64, String)> = stmt
            .query_map([quest_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .ok()?
            .filter_map(Result::ok)
            .collect();
        for role in roles {
            let Some((_, gts, object)) = rows.iter().find(|(name, _, _)| name == role) else {
                continue;
            };
            if let Some(text) = self.gts_text(object, *gts, lang) {
                return Some(text);
            }
        }
        None
    }

    fn gts_text(&self, object: &str, gts: i64, lang: &str) -> Option<String> {
        if gts <= 0 {
            return None;
        }
        let id = format!("{gts:08}");
        self.conn
            .query_row(
                "SELECT text FROM strings WHERE object = ?1 AND lang = ?2 AND id = ?3",
                (object, lang, id),
                |row| row.get(0),
            )
            .ok()
            .filter(|text: &String| !text.is_empty())
            .map(game_breaks)
    }

    fn item_icon(&self, code: i64) -> Option<(PathBuf, i32)> {
        self.index.item_icons.get(&code).cloned()
    }

    pub fn linked_icon(&self, key: &str) -> Option<(PathBuf, i32)> {
        if let Some(code) = key.strip_prefix("monster:") {
            return self.sprite(code);
        }
        if let Some(code) = key.strip_prefix("item:") {
            return code.parse().ok().and_then(|id| self.item_icon(id));
        }
        None
    }

    fn icon_path(&self, image: &str, frame: i64) -> Option<(PathBuf, i32)> {
        if image.is_empty() || frame < 0 {
            return None;
        }
        let folder = self.client_root.join("Assets").join("Textures").join("Items");
        let wanted = Path::new(image).file_name()?.to_string_lossy().to_lowercase();
        let entry = std::fs::read_dir(&folder).ok()?.filter_map(Result::ok).find(|entry| {
            entry.file_name().to_string_lossy().to_lowercase() == wanted
        })?;
        Some((entry.path(), frame as i32))
    }
}

struct EnhanceRow {
    plus: i64,
    dmin: i64,
    dmax: i64,
    move_speed: i64,
    atk_speed: i64,
    magic_resist: i64,
    hit_resist: i64,
    element: String,
    element_damage: i64,
    attack_range: i64,
    skill_range: i64,
    crit: i64,
    mana: i64,
    sight: i64,
    air: i64,
    ground: i64,
    cost: i64,
    merc_def: i64,
    merc_atk: i64,
    spell: i64,
    reflect: i64,
    invuln: i64,
}

impl EnhanceRow {
    fn is_blank(&self) -> bool {
        self.dmin == 0
            && self.dmax == 0
            && self.move_speed == 0
            && self.atk_speed == 0
            && self.magic_resist == 0
            && self.hit_resist == 0
            && self.element.is_empty()
            && self.element_damage == 0
            && self.attack_range == 0
            && self.skill_range == 0
            && self.crit == 0
            && self.mana == 0
            && self.sight == 0
            && self.air == 0
            && self.ground == 0
            && self.cost == 0
            && self.merc_def == 0
            && self.merc_atk == 0
            && self.spell == 0
            && self.reflect == 0
            && self.invuln == 0
    }

    fn summary(&self, lang: &str) -> String {
        let mut bits = vec![format!("+{}", self.plus)];
        if self.dmin != 0 || self.dmax != 0 {
            let damage = if self.dmin == self.dmax {
                commas(self.dmin)
            } else {
                format!("{}-{}", commas(self.dmin), commas(self.dmax))
            };
            bits.push(format!("{} {damage}", phrase("damage", lang)));
        }
        Self::push(&mut bits, lang, "move-speed", self.move_speed);
        Self::push(&mut bits, lang, "atk-speed", self.atk_speed);
        Self::push(&mut bits, lang, "magic-resist", self.magic_resist);
        Self::push(&mut bits, lang, "hit-resist", self.hit_resist);
        if !self.element.is_empty() || self.element_damage != 0 {
            let name = element_letter(&self.element, lang);
            if self.element_damage == 0 {
                bits.push(name);
            } else if name.is_empty() {
                bits.push(commas(self.element_damage));
            } else {
                bits.push(format!("{name} {}", commas(self.element_damage)));
            }
        }
        Self::push(&mut bits, lang, "atk-range", self.attack_range);
        Self::push(&mut bits, lang, "skill-range", self.skill_range);
        Self::push(&mut bits, lang, "crit", self.crit);
        Self::push(&mut bits, lang, "mana-cut", self.mana);
        Self::push(&mut bits, lang, "sight", self.sight);
        Self::push(&mut bits, lang, "air-dmg", self.air);
        Self::push(&mut bits, lang, "ground-dmg", self.ground);
        Self::push(&mut bits, lang, "stigma", self.cost);
        Self::push(&mut bits, lang, "merc-def", self.merc_def);
        Self::push(&mut bits, lang, "merc-atk", self.merc_atk);
        Self::push(&mut bits, lang, "spell-rate", self.spell);
        Self::push(&mut bits, lang, "reflect", self.reflect);
        Self::push(&mut bits, lang, "invuln", self.invuln);
        if bits.len() == 1 {
            bits.push("—".into());
        }
        bits.join("  ")
    }

    fn push(bits: &mut Vec<String>, lang: &str, key: &str, value: i64) {
        if value != 0 {
            bits.push(format!("{} {}", phrase(key, lang), commas(value)));
        }
    }
}

fn element_letter(letter: &str, lang: &str) -> String {
    let (en, zh) = match letter {
        "F" => ("Fire", "火"),
        "W" => ("Water", "水"),
        "A" => ("Wind", "風"),
        "T" => ("Thunder", "雷"),
        "E" => ("Earth", "地"),
        _ => return letter.to_string(),
    };
    chrome_text(lang, zh, en)
}

fn source_names(source: &str) -> (&'static str, &'static str) {
    SOURCE_LABEL
        .iter()
        .find(|(key, _, _, _)| *key == source)
        .map(|(_, zh, en, _)| (*zh, *en))
        .unwrap_or(("", ""))
}

fn label(source: &str, lang: &str) -> String {
    let (zh, en) = source_names(source);
    if zh.is_empty() && en.is_empty() {
        source.to_string()
    } else {
        chrome_text(lang, zh, en)
    }
}

fn hit_version(key: &str) -> &str {
    key.strip_prefix("patch:")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("")
}

fn patch_number(version: &str) -> i64 {
    version
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn dotted_version(version: &str) -> String {
    let number = patch_number(version);
    let text = number.to_string();
    if text.len() >= 5 {
        format!("V{}.{}", &text[..1], &text[1..])
    } else if version.is_empty() {
        String::new()
    } else {
        version.to_string()
    }
}

pub fn client_label() -> String {
    let text = std::fs::read_to_string(client_version_file()).unwrap_or_else(|_| "V3.4220".into());
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "V3.4220".into()
    } else {
        dotted_version(trimmed)
    }
}

fn client_build() -> i64 {
    patch_number(&client_label())
}

fn client_version_file() -> PathBuf {
    find_db()
        .ok()
        .and_then(|db| db.parent().map(|dir| dir.join("client-version.txt")))
        .unwrap_or_else(|| PathBuf::from("data/client-version.txt"))
}

fn ship_label(version: &str, lang: &str) -> String {
    let key = if patch_number(version) <= client_build() {
        "shipped"
    } else {
        "unshipped"
    };
    phrase(key, lang)
}

fn notes_placeholder(lang: &str) -> Vec<Hit> {
    vec![Hit {
        title: phrase("notes-empty-title", lang),
        subtitle: phrase("notes-empty-sub", lang),
        key: "note:empty".into(),
        icon: None,
                    solo: false,
                    aside: String::new(),
                badge: None,
                    chip: String::new(),
    }]
}

fn server_name(server: &str, lang: &str) -> String {
    let key = match server {
        "live" => "server-live",
        "test" => "server-test",
        _ => "server-all",
    };
    phrase(key, lang)
}

fn english_first(lang: &str) -> bool {
    matches!(lang, "en-zh" | "en")
}

fn chrome(lang: &str) -> &'static str {
    if english_first(lang) { "en" } else { "zh" }
}

fn chrome_text(lang: &str, zh: &str, en: &str) -> String {
    if chrome(lang) == "en" { en } else { zh }.to_string()
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn first_name(primary: &str, fallback: &str) -> String {
    let primary = primary.trim();
    if primary.is_empty() { fallback.trim().to_string() } else { primary.to_string() }
}

fn lead(lang: &str, en: &str, zh: &str) -> String {
    if english_first(lang) { first_name(en, zh) } else { first_name(zh, en) }
}

fn trail(lang: &str, en: &str, zh: &str) -> String {
    let en = en.trim();
    let zh = zh.trim();
    if en.is_empty() || zh.is_empty() || same_name(en, zh) {
        return String::new();
    }
    if english_first(lang) { zh.to_string() } else { en.to_string() }
}

fn show(lang: &str, en: &str, zh: &str) -> String {
    let main = lead(lang, en, zh);
    let alt = trail(lang, en, zh);
    if alt.is_empty() { main } else { format!("{main} · {alt}") }
}

fn stack(lang: &str, en: &str, zh: &str) -> String {
    let main = lead(lang, en, zh);
    let alt = trail(lang, en, zh);
    if alt.is_empty() { main } else { format!("{main}\n{alt}") }
}

fn caption(alt: &str, rest: String) -> String {
    if alt.is_empty() {
        rest
    } else if rest.is_empty() {
        alt.to_string()
    } else {
        format!("{alt} · {rest}")
    }
}

fn pick(lang: &str, _ko: &str, en: &str, zh: &str) -> String {
    show(lang, en, zh)
}

fn pick_place(lang: &str, en: &str, zh: &str) -> String {
    show(lang, en, zh)
}

struct ListedPlace {
    count: i64,
    sort: String,
    gts: i64,
    hit: Hit,
}

fn place_hit(lang: &str, en: &str, zh: &str, count: i64, key: String) -> Hit {
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

fn group_key(place: &PlaceRow) -> String {
    let source = if place.zh.trim().is_empty() { place.en.as_str() } else { place.zh.as_str() };
    let base = place_split(source).0;
    if base.is_empty() { format!("gts:{}", place.gts) } else { base }
}

fn bases_of(members: &[&PlaceRow]) -> (String, String) {
    let en = majority(members.iter().map(|place| place_split(&place.en).0));
    let zh = majority(members.iter().map(|place| place_split(&place.zh).0));
    (en, zh)
}

fn member_gts(members: &[&PlaceRow]) -> Vec<i64> {
    members.iter().map(|place| place.gts).collect()
}

fn majority(values: impl Iterator<Item = String>) -> String {
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

fn all_floor_names(en: &str, zh: &str) -> (String, String) {
    let en_base = if en.is_empty() { zh } else { en };
    let zh_base = if zh.is_empty() { en } else { zh };
    (format!("{en_base} - All"), format!("{zh_base} - 所有"))
}

fn floor_sort_label(lang: &str, en: &str, zh: &str) -> String {
    let shown = lead(lang, en, zh);
    let (_, floor) = place_split(&shown);
    if floor.is_empty() { shown } else { floor }
}

fn place_split(name: &str) -> (String, String) {
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

fn is_dash(mark: char) -> bool {
    matches!(mark, '-' | '－' | '–' | '—' | '―')
}

fn split_attached_floor(name: &str) -> Option<(String, String)> {
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

fn commas(value: i64) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let body: String = out.chars().rev().collect();
    if negative { format!("-{body}") } else { body }
}

fn game_breaks(text: String) -> String {
    if !text.contains('\\') {
        return text;
    }
    text.replace("\\r\\n", "\n").replace("\\n", "\n").replace("\\r", "")
}

fn commas_text(text: &str) -> String {
    text.parse::<i64>().map(commas).unwrap_or_else(|_| text.to_string())
}

fn kind_name(kind: &str, lang: &str) -> String {
    let (zh, en) = match kind.to_ascii_lowercase().as_str() {
        "item" | "book" => ("道具", "Item"),
        "wearitem" => ("裝備", "Equip"),
        "useitem" => ("使用", "Use"),
        "monster" => ("怪物", "Monster"),
        "exp" | "expall" => ("經驗", "Exp"),
        "money" => ("金錢", "Money"),
        "credit" | "credit1" | "credit2" | "credit3" | "credit4" | "credit5" => ("信用", "Credit"),
        "quest" => ("任務", "Quest"),
        "level" | "level1" | "level2" | "level3" | "level4" | "level5" => ("等級", "Level"),
        "map" => ("地圖", "Map"),
        "satiety" => ("飽食度", "Satiety"),
        "npc" => ("人物", "NPC"),
        "work" => ("勞動", "Labor"),
        "key" => ("按鍵", "Key"),
        "state" => ("狀態", "State"),
        "user" => ("角色", "Character"),
        "pet" => ("寵物", "Pet"),
        "nation" => ("國籍", "Nation"),
        _ => return kind.to_string(),
    };
    chrome_text(lang, zh, en)
}

fn role_phrase(role: &str, lang: &str) -> String {
    match role {
        "requirement" => phrase("req", lang),
        "objective" => phrase("done", lang),
        "reward" => phrase("reward", lang),
        _ => role.to_string(),
    }
}

fn reward_amount(step: &QuestStep) -> Option<String> {
    let qty = step.qty.trim();
    if !qty.is_empty() && qty != "0" {
        return Some(commas_text(qty));
    }
    let code = step.code.trim();
    if !code.is_empty() && code != "0" {
        return Some(commas_text(code));
    }
    None
}

fn with_count(name: String, qty: &str) -> String {
    if qty.is_empty() || qty == "0" {
        name
    } else {
        format!("{name} × {}", commas_text(qty))
    }
}

fn numbered(label: String, qty: &str) -> Option<(String, String)> {
    if qty.is_empty() || qty == "0" {
        return None;
    }
    Some((format!("{label} {}", commas_text(qty)), String::new()))
}

fn step_hint(label: &str) -> String {
    let trimmed = label.trim();
    if trimmed.is_empty() || trimmed == "0" || trimmed == "설명" {
        String::new()
    } else {
        trimmed.to_string()
    }
}

fn step_value(code: &str, qty: &str) -> String {
    let qty_ok = !qty.is_empty() && qty != "0";
    let code_ok = !code.is_empty() && code != "0";
    match (qty_ok, code_ok) {
        (true, true) => format!("{}  {code}", commas_text(qty)),
        (true, false) => commas_text(qty),
        (false, true) => code.to_string(),
        (false, false) => String::new(),
    }
}

fn named_lead(lang: &str, en: &str, zh: &str, label: &str, code: &str) -> (String, String) {
    let title = lead(lang, en, zh);
    if title.is_empty() {
        let fallback = if label.is_empty() { code.to_string() } else { label.to_string() };
        (fallback, String::new())
    } else {
        (title, trail(lang, en, zh))
    }
}

fn phrase(key: &str, lang: &str) -> String {
    let (zh, en) = match key {
        "code" => ("代碼", "Code"),
        "vendor" => ("扔店價", "Vendor"),
        "weight" => ("重量", "Weight"),
        "item-sort" => ("分類", "Category"),
        "pieces" => ("件", "items"),
        "sets" => ("套", "sets"),
        "set" => ("套裝", "Set"),
        "set-size" => ("件數", "Pieces"),
        "set-charm" => ("咒術", "Charm"),
        "set-unique" => ("專用", "Exclusive"),
        "slots" => ("部位", "Slots"),
        "item-fx" => ("道具", "Item"),
        "set-fx" => ("套裝", "Set"),
        "total-fx" => ("合計", "Total"),
        "crit-dmg" => ("暴擊傷害", "Crit damage"),
        "dmg-cut" => ("減傷", "Damage cut"),
        "missing-set" => ("找不到這套裝備", "Set not found"),
        "enhance" => ("強化", "Enhance"),
        "damage" => ("傷害", "Damage"),
        "move-speed" => ("移速", "Move"),
        "atk-speed" => ("攻速", "Atk speed"),
        "atk-range" => ("射程", "Range"),
        "skill-range" => ("技距", "Skill range"),
        "crit" => ("暴擊", "Crit"),
        "mana-cut" => ("減魔", "Mana"),
        "sight" => ("視野", "Sight"),
        "air-dmg" => ("對空", "Air"),
        "ground-dmg" => ("對地", "Ground"),
        "stigma" => ("聖痕", "Stigma"),
        "merc-def" => ("傭防", "Merc def"),
        "merc-atk" => ("傭攻", "Merc atk"),
        "spell-rate" => ("咒術", "Spell"),
        "reflect" => ("反射", "Reflect"),
        "invuln" => ("無敵", "Invuln"),
        "qty" => ("數量", "Qty"),
        "fee" => ("製作費", "Craft fee"),
        "learn" => ("學習費", "Learn fee"),
        "materials" => ("材料", "Materials"),
        "craft-need" => ("合成條件", "Requirements"),
        "related" => ("相關", "Related"),
        "credit-level" => ("信用等級", "Credit"),
        "pick-one" => ("任選", "Choice"),
        "combine" => ("組合", "Combine"),
        "awaken" => ("覺醒", "Awaken"),
        "remodel" => ("改造", "Remodel"),
        "promote" => ("轉職", "Promote"),
        "upgrade" => ("升級", "Upgrade"),
        "roster" => ("角色", "Roster"),
        "src-roster" => ("角色", "Roster"),
        "open-recipe" => ("打開配方", "Open recipe"),
        "spawn" => ("出沒", "Found in"),
        "kind" => ("類型", "Type"),
        "min-level" => ("最低等級", "Min level"),
        "success" => ("成功率", "Success"),
        "recipes" => ("張", "recipes"),
        "unit" => ("單位", "Unit"),
        "spawn-place" => ("出沒地", "Found in"),
        "hp" => ("體力", "HP"),
        "atk" => ("攻擊", "ATK"),
        "exp" => ("經驗", "EXP"),
        "prev" => ("前置", "Requires"),
        "chain-prev" => ("前置", "Prev"),
        "chain-next" => ("下一", "Next"),
        "repeat" => ("可重複", "Repeats"),
        "quest-count" => ("則", "quests"),
        "starter" => ("新手", "Starter"),
        "combat" => ("戰鬥", "Combat"),
        "repeat-type" => ("重複", "Repeat"),
        "timed" => ("限時", "Timed"),
        "giver" => ("人物", "NPC"),
        "req" => ("條件", "Requires"),
        "done" => ("完成", "Complete"),
        "reward" => ("獎勵", "Reward"),
        "task-kill" => ("打怪物", "Kill"),
        "task-item" => ("要物品", "Need item"),
        "task-talk" => ("對話", "Talk"),
        "task-other" => ("其他", "Other"),
        "missing" => ("找不到", "Not found"),
        "missing-item" => ("找不到這個道具", "Item not found"),
        "missing-recipe" => ("找不到這張配方", "Recipe not found"),
        "missing-quest" => ("找不到這個任務", "Quest not found"),
        "missing-monster" => ("找不到這隻怪物", "Monster not found"),
        "missing-npc" => ("找不到這個 NPC", "NPC not found"),
        "missing-hero" => ("找不到這個角色", "Character not found"),
        "job-chain" => ("轉職", "Class line"),
        "job-base" => ("原本", "Base"),
        "job-first" => ("一轉", "1st"),
        "job-second" => ("二轉", "2nd"),
        "job-remodel" => ("改造", "Remodel"),
        "hire" => ("雇用", "Hire"),
        "skill" => ("技能", "Skill"),
        "heroes" => ("人", "characters"),
        "credit" => ("信用", "Credit"),
        "goods" => ("種", "goods"),
        "nation" => ("國籍", "Nation"),
        "map" => ("地圖", "Map"),
        "coord" => ("座標", "Position"),
        "monsters" => ("隻", "monsters"),
        "mp" => ("魔力", "MP"),
        "hit-resist" => ("打擊抗性", "Hit resist"),
        "magic-resist" => ("魔法抗性", "Magic resist"),
        "element" => ("屬性", "Element"),
        "ela" => ("屬性值", "ELA"),
        "solo-drop" => ("單人限定", "Solo only"),
        "sold-in" => ("在賣", "Sold in"),
        "specialty" => ("特產", "Specialty"),
        "people-here" => ("人物", "People"),
        "lurking" => ("出沒", "Found"),
        "dropped-by" => ("會掉落的怪物", "Dropped by"),
        "craft-how" => ("合成", "Craft"),
        "make" => ("生產", "Produce"),
        "src-sold-in" => ("商店", "Shops"),
        "src-dropped-by" => ("掉落", "Drops"),
        "src-craft-how" => ("合成", "Craft"),
        "src-make" => ("生產", "Produce"),
        "src-used-for" => ("用途", "Uses"),
        "src-also-from" => ("其他", "Other"),
        "use-quest" => ("任務", "Quest"),
        "use-craft" => ("合成", "Craft"),
        "yield" => ("產量", "Yield"),
        "yield-max" => ("上限", "Max"),
        "labor" => ("勞動", "Labor"),
        "tool" => ("工具", "Tool"),
        "used-for" => ("用途", "Used for"),
        "also-from" => ("其他獲取", "Also from"),
        "all-regions" => ("所有地區", "All regions"),
        "land-c" => ("中國", "China"),
        "land-tw" => ("台灣", "Taiwan"),
        "land-k" => ("朝鮮", "Joseon"),
        "land-j" => ("日本", "Japan"),
        "land-ct" => ("中國·台灣", "China · Taiwan"),
        "land-m" => ("蒙古", "Mongolia"),
        "no-droppers" => ("沒有怪物掉落", "No monsters drop this"),
        "craft" => ("合成", "Craft"),
        "detail" => ("說明", "Note"),
        "boss" => ("首領", "Boss"),
        "normal" => ("一般", "Normal"),
        "air" => ("空中", "Air"),
        "ground" => ("地面", "Ground"),
        "drops" => ("掉落", "Drops"),
        "notes-empty-title" => ("尚無更新日誌", "No patch notes"),
        "notes-empty-sub" => ("點開看怎麼更新", "Open for how to update"),
        "notes-help" => (
            "把公告存成 data/notices 裡的 .txt。\n.txt 和 .gts 都換好之後，執行 python -m gersang update，\n或雙擊 update.bat，然後重新開啟這個視窗。",
            "Save notices as .txt files in data/notices.\nAfter the .txt and .gts files are in place, run python -m gersang update\nor double-click update.bat, then reopen this window.",
        ),
        "server-live" => ("本服", "Live"),
        "server-test" => ("測試服", "Test"),
        "server-all" => ("全服", "All"),
        "shipped" => ("已實裝", "In"),
        "unshipped" => ("未實裝", "Not in"),
        _ => (key, key),
    };
    chrome_text(lang, zh, en)
}

pub fn load_favourites() -> HashSet<String> {
    std::fs::read_to_string(favourites_file())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn save_favourites(codes: &HashSet<String>) {
    let mut lines: Vec<&str> = codes.iter().map(String::as_str).collect();
    lines.sort_unstable();
    let _ = std::fs::write(favourites_file(), lines.join("\n"));
}

fn favourites_file() -> PathBuf {
    data_dir().join("favourites.txt")
}

pub fn load_lang() -> String {
    let text = std::fs::read_to_string(lang_file()).unwrap_or_default();
    match text.trim() {
        "en-zh" | "en" => "en-zh".into(),
        _ => "zh-en".into(),
    }
}

pub fn save_lang(lang: &str) {
    if matches!(lang, "zh-en" | "en-zh") {
        let _ = std::fs::write(lang_file(), lang);
    }
}

fn set_extra_file() -> PathBuf {
    data_dir().join("set-extra.txt")
}

pub fn load_set_extra() -> bool {
    std::fs::read_to_string(set_extra_file())
        .ok()
        .is_some_and(|text| text.trim() == "1")
}

pub fn save_set_extra(on: bool) {
    let _ = std::fs::write(set_extra_file(), if on { "1" } else { "0" });
}

fn set_extra_name(name: &str) -> bool {
    if name.contains("[이벤트]") || name.contains("[활동]") {
        return true;
    }
    let chars: Vec<char> = name.chars().collect();
    chars.windows(2).any(|pair| pair[0].is_ascii_digit() && pair[1] == '일')
}

pub fn load_opacity() -> f32 {
    let text = std::fs::read_to_string(opacity_file()).unwrap_or_default();
    let percent = text.trim().parse::<f32>().unwrap_or(100.0);
    (percent / 100.0).clamp(0.25, 1.0)
}

pub fn save_opacity(value: f32) {
    let percent = (value.clamp(0.25, 1.0) * 100.0).round() as i32;
    let _ = std::fs::write(opacity_file(), percent.to_string());
}

pub struct DockPref {
    pub vertical: bool,
    pub show: [bool; 9],
    pub order: [i32; 9],
    pub width: f32,
    pub height: f32,
}

pub fn load_dock() -> DockPref {
    let mut pref = DockPref {
        vertical: false,
        show: [true; 9],
        order: [0, 1, 2, 3, 4, 5, 6, 7, 8],
        width: 580.0,
        height: 84.0,
    };
    let text = std::fs::read_to_string(data_dir().join("dock.txt")).unwrap_or_default();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("v=") {
            pref.vertical = value.trim() == "1";
        } else if let Some(value) = line.strip_prefix("show=") {
            for (index, bit) in value.trim().chars().take(9).enumerate() {
                pref.show[index] = bit != '0';
            }
        } else if let Some(value) = line.strip_prefix("size=") {
            let mut parts = value.split(',');
            let width = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
            let height = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
            if let (Some(width), Some(height)) = (width, height) {
                if width >= 48.0 && height >= 48.0 {
                    pref.width = width;
                    pref.height = height;
                }
            }
        } else if let Some(value) = line.strip_prefix("order=") {
            let nums: Vec<i32> = value.split(',').filter_map(|part| part.trim().parse().ok()).collect();
            if let Some(order) = permutation(&nums) {
                pref.order = order;
            }
        }
    }
    pref
}

pub fn save_dock(pref: &DockPref) {
    let show: String = pref.show.iter().map(|on| if *on { '1' } else { '0' }).collect();
    let order = pref.order.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",");
    let text = format!(
        "v={}\nshow={show}\norder={order}\nsize={},{}\n",
        if pref.vertical { 1 } else { 0 },
        pref.width,
        pref.height
    );
    let _ = std::fs::write(data_dir().join("dock.txt"), text);
}

pub struct HomePref {
    pub size: i32,
    pub size_chosen: bool,
    pub order: [i32; 9],
}

pub fn load_home() -> HomePref {
    let mut pref = HomePref {
        size: 1,
        size_chosen: false,
        order: [0, 1, 2, 3, 4, 5, 6, 7, 8],
    };
    let text = std::fs::read_to_string(data_dir().join("home.txt")).unwrap_or_default();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("size=") {
            if let Ok(size) = value.trim().parse::<i32>() {
                if (0..=2).contains(&size) {
                    pref.size = size;
                    pref.size_chosen = true;
                }
            }
        } else if let Some(value) = line.strip_prefix("order=") {
            let nums: Vec<i32> = value.split(',').filter_map(|part| part.trim().parse().ok()).collect();
            if let Some(order) = permutation(&nums) {
                pref.order = order;
            }
        }
    }
    pref
}

pub fn save_home(pref: &HomePref) {
    let order = pref.order.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",");
    let text = format!("size={}\norder={order}\n", pref.size.clamp(0, 2));
    let _ = std::fs::write(data_dir().join("home.txt"), text);
}

pub struct WindowPref {
    pub width: f32,
    pub height: f32,
    pub x: i32,
    pub y: i32,
}

pub fn load_window() -> Option<WindowPref> {
    let text = std::fs::read_to_string(data_dir().join("window.txt")).ok()?;
    let mut width = None;
    let mut height = None;
    let mut x = None;
    let mut y = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("size=") {
            let mut parts = value.split(',');
            width = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
            height = parts.next().and_then(|part| part.trim().parse::<f32>().ok());
        } else if let Some(value) = line.strip_prefix("pos=") {
            let mut parts = value.split(',');
            x = parts.next().and_then(|part| part.trim().parse::<i32>().ok());
            y = parts.next().and_then(|part| part.trim().parse::<i32>().ok());
        }
    }
    let (Some(width), Some(height), Some(x), Some(y)) = (width, height, x, y) else {
        return None;
    };
    if width < 280.0 || height < 360.0 {
        return None;
    }
    Some(WindowPref { width, height, x, y })
}

pub fn save_window(pref: &WindowPref) {
    if pref.width < 280.0 || pref.height < 360.0 {
        return;
    }
    let text = format!("size={},{}\npos={},{}\n", pref.width, pref.height, pref.x, pref.y);
    let _ = std::fs::write(data_dir().join("window.txt"), text);
}

fn permutation(nums: &[i32]) -> Option<[i32; 9]> {
    if nums.len() != 9 {
        return None;
    }
    let mut seen = [false; 9];
    for n in nums {
        if *n < 0 || *n > 8 || seen[*n as usize] {
            return None;
        }
        seen[*n as usize] = true;
    }
    let mut order = [0; 9];
    order.copy_from_slice(nums);
    Some(order)
}

fn data_dir() -> PathBuf {
    find_db()
        .ok()
        .and_then(|db| db.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn lang_file() -> PathBuf {
    data_dir().join("ui-lang.txt")
}

fn opacity_file() -> PathBuf {
    data_dir().join("opacity.txt")
}

fn pref_rank(source: &str) -> usize {
    PREF.iter().position(|key| *key == source).unwrap_or(99)
}

pub fn nation_label(id: &str, lang: &str) -> String {
    nation_name(id.parse().unwrap_or(0), lang)
}

pub fn source_label(source: &str, lang: &str) -> String {
    label(source, lang)
}

pub fn item_category_label(key: &str, lang: &str) -> String {
    item_category_name(key, lang)
}

pub fn weapon_family_label(key: &str, lang: &str) -> String {
    weapon_family_name(key, lang)
}

pub fn hero_rank_label(rank: &str, lang: &str) -> String {
    let (en, zh) = hero_rank_pair(rank.parse().unwrap_or(-1));
    if en.is_empty() && zh.is_empty() {
        rank.to_string()
    } else {
        chrome_text(lang, zh, en)
    }
}

pub fn hero_land_label(mask: &str, lang: &str) -> String {
    let (en, zh) = hero_land_pair(mask.parse().unwrap_or(-1));
    chrome_text(lang, zh, en)
}

pub fn hero_tier_label(rank: &str, lang: &str) -> String {
    let (en, zh) = hero_tier_pair(rank.parse().unwrap_or(-1));
    if en.is_empty() && zh.is_empty() {
        rank.to_string()
    } else {
        chrome_text(lang, zh, en)
    }
}

fn nation_pair(id: i64) -> (&'static str, &'static str) {
    match id {
        1 => ("Joseon", "朝鮮"),
        2 => ("Japan", "日本"),
        3 => ("China", "中國"),
        4 => ("Taiwan", "台灣"),
        5 => ("India", "印度"),
        6 => ("Special", "特殊"),
        _ => ("Other", "其他"),
    }
}

fn nation_name(id: i64, lang: &str) -> String {
    let (en, zh) = nation_pair(id);
    show(lang, en, zh)
}

fn element_name(id: i64, lang: &str) -> String {
    let (en, zh) = match id {
        1 => ("Fire", "火"),
        2 => ("Water", "水"),
        3 => ("Wind", "風"),
        4 => ("Thunder", "雷"),
        5 => ("Earth", "地"),
        _ => ("None", "無"),
    };
    show(lang, en, zh)
}

fn element_file(element: i64) -> Option<&'static str> {
    Some(match element {
        1 => "Assets/Textures/UI/Icons/elementfire.AGF",
        2 => "Assets/Textures/UI/Icons/elementwater.AGF",
        3 => "Assets/Textures/UI/Icons/elementair.AGF",
        4 => "Assets/Textures/UI/Icons/elementthun.AGF",
        5 => "Assets/Textures/UI/Icons/elementearth.AGF",
        _ => return None,
    })
}

fn load_portraits(conn: &Connection) -> HashMap<String, (String, i32)> {
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

fn choose_pictures(
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

fn load_sprites(conn: &Connection) -> std::collections::HashMap<String, String> {
    let mut stmt = match conn.prepare("SELECT code, path FROM monster_sprites") {
        Ok(stmt) => stmt,
        Err(_) => return std::collections::HashMap::new(),
    };
    stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

struct Index {
    monsters: Vec<MonsterRow>,
    nations: Vec<(i64, i64)>,
    places: Vec<PlaceRow>,
    items: Vec<ItemRow>,
    item_icons: HashMap<i64, (PathBuf, i32)>,
    recipes: Vec<RecipeRow>,
    quests: Vec<QuestRow>,
    npcs: Vec<NpcRow>,
    heroes: Vec<HeroRow>,
    chains: HashMap<String, Vec<ChainStep>>,
    combos: HashMap<String, Vec<Combo>>,
    roster_uses: HashMap<i64, Vec<RosterUse>>,
    works: Vec<WorkBuilding>,
    yards: HashMap<String, WorkFacility>,
    formulas: HashMap<i64, ItemFormula>,
    sets: Vec<SetRow>,
    set_magic: HashMap<i64, SetMagic>,
    towns: Vec<TownRow>,
    charts: Vec<ChartRow>,
    aliases: RefCell<HashMap<String, HashSet<String>>>,
    active_alias: Cell<bool>,
    active_codes: RefCell<HashSet<String>>,
}

struct MonsterRow {
    code: String,
    nation: i64,
    en: String,
    zh: String,
    place_en: String,
    place_zh: String,
    places: Vec<i64>,
    names: String,
    icon: Option<(PathBuf, i32)>,
    badge: Option<PathBuf>,
}

struct PlaceRow {
    nation: i64,
    gts: i64,
    en: String,
    zh: String,
    count: i64,
    names: String,
    sort: String,
}

struct ItemRow {
    code_str: String,
    en: String,
    zh: String,
    category: String,
    subtype: String,
    names: String,
}

struct SetRow {
    code: i64,
    kind: i64,
    pieces: i64,
    ko: String,
    en: String,
    zh: String,
    names: String,
    icon: Option<(PathBuf, i32)>,
}

struct SetMagic {
    value1: i64,
    value2: i64,
    percent: bool,
    note: String,
}

struct RecipeRow {
    id: i64,
    source: String,
    result_str: String,
    en: String,
    zh: String,
    names: String,
    level_min: Option<i64>,
    icon: Option<(PathBuf, i32)>,
}

struct QuestRow {
    id: i64,
    source: String,
    code: String,
    kind: String,
    npc: i64,
    title_en: String,
    title_zh: String,
    names: String,
}

#[derive(Clone)]
struct ChainStep {
    slot: i64,
    code: String,
    en: String,
    zh: String,
    materials: Vec<(i64, i64)>,
}

#[derive(Clone)]
struct Combo {
    result: String,
    parts: Vec<String>,
    en: String,
    zh: String,
    materials: Vec<(i64, i64)>,
}

#[derive(Clone)]
struct RosterUse {
    qty: i64,
    target: String,
    source: String,
    combo: bool,
    note: String,
}

struct ChainBook {
    lines: HashMap<String, Vec<ChainStep>>,
    combos: HashMap<String, Vec<Combo>>,
    uses: HashMap<i64, Vec<RosterUse>>,
}

#[derive(Clone)]
struct HeroSkill {
    name: String,
    en: String,
    zh: String,
}

struct HeroRow {
    code: String,
    nation: i64,
    rank: i64,
    ko: String,
    en: String,
    zh: String,
    nation_en: String,
    nation_zh: String,
    desc_en: String,
    desc_zh: String,
    hire_en: String,
    hire_zh: String,
    skills: Vec<HeroSkill>,
    names: String,
    icon: Option<(PathBuf, i32)>,
}

struct NpcRow {
    code: i64,
    ko: String,
    en: String,
    zh: String,
    nation_en: String,
    nation_zh: String,
    map_en: String,
    map_zh: String,
    map_index: i64,
    x: i64,
    y: i64,
    place_en: String,
    place_zh: String,
    names: String,
    icon: Option<(PathBuf, i32)>,
}

struct QuestLine {
    role: String,
    slot: i64,
    texts: HashMap<String, String>,
}

struct QuestStep {
    role: String,
    kind: String,
    code: String,
    qty: String,
    label: String,
}

const QUEST_TITLE_ROLES: &[&str] = &["title", "subject", "start-subject"];

#[derive(Clone)]
#[allow(dead_code)]
struct WorkUpgradeName {
    en: String,
    zh: String,
}

struct WorkProduct {
    item: i64,
    nations: String,
}

struct FormulaPart {
    kind: String,
    item: i64,
    qty: i64,
}

struct ItemFormula {
    country: i64,
    labor: i64,
    yield_n: i64,
    yield_max: i64,
    plant: String,
    parts: Vec<FormulaPart>,
}

struct WorkFacility {
    code: String,
    en: String,
    zh: String,
    names: String,
    #[allow(dead_code)]
    upgrades: Vec<WorkUpgradeName>,
    products: Vec<WorkProduct>,
    icon: Option<(PathBuf, i32)>,
}

#[derive(Clone)]
struct TownGood {
    item: i64,
    specialty: bool,
}

struct TownRow {
    code: String,
    en: String,
    zh: String,
    region: String,
    x: i64,
    y: i64,
    names: String,
    goods: Vec<TownGood>,
}

struct ChartRow {
    code: i64,
    en: String,
    zh: String,
}

struct WorkBuilding {
    code: String,
    en: String,
    zh: String,
    cost: i64,
    credit: i64,
    names: String,
    facilities: Vec<String>,
    icon: Option<(PathBuf, i32)>,
}

impl Index {
    fn load(
        conn: &Connection,
        client_root: &Path,
        pictures: &HashMap<String, (PathBuf, i32)>,
        data_dir: &Path,
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
        let aliases = RefCell::new(parse_alias_book(&alias_editor_text_in(data_dir)));
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

    fn prepare_aliases(&self, folded: &str) {
        let codes = alias_codes(&self.aliases.borrow(), folded);
        self.active_alias.set(!codes.is_empty());
        *self.active_codes.borrow_mut() = codes;
    }

    fn name_hit(&self, query: &str, folded: &str, exact: &str, names: &str) -> bool {
        if name_match(query, folded, exact, names) {
            return true;
        }
        self.active_alias.get() && self.active_codes.borrow().contains(exact)
    }

    fn nation_hits(&self, lang: &str) -> Vec<Hit> {
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

    fn place_hits(&self, nation: &str, folded: &str, lang: &str) -> Vec<Hit> {
        let Ok(nation) = nation.parse::<i64>() else { return Vec::new() };
        let mut buckets: HashMap<String, Vec<&PlaceRow>> = HashMap::new();
        for place in self.places.iter().filter(|place| place.nation == nation) {
            buckets.entry(group_key(place)).or_default().push(place);
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

    fn floor_hits(&self, nation: &str, area: &str, folded: &str, lang: &str) -> Vec<Hit> {
        let members = self.group_members(nation, area);
        if members.is_empty() {
            return Vec::new();
        }
        let (en, zh) = bases_of(&members);
        let mut floors = Vec::new();
        for place in &members {
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

    fn group_members(&self, nation: &str, area: &str) -> Vec<&PlaceRow> {
        let Ok(nation) = nation.parse::<i64>() else { return Vec::new() };
        self.places
            .iter()
            .filter(|place| place.nation == nation && group_key(place) == area)
            .collect()
    }

    fn group_bases(&self, nation: &str, area: &str) -> Option<(String, String)> {
        let members = self.group_members(nation, area);
        if members.len() < 2 {
            return None;
        }
        Some(bases_of(&members))
    }

    fn group_gts(&self, nation: &str, area: &str) -> Vec<i64> {
        member_gts(&self.group_members(nation, area))
    }

    fn count_at(&self, gts: &[i64]) -> i64 {
        self.monsters
            .iter()
            .filter(|monster| monster.places.iter().any(|place| gts.contains(place)))
            .count() as i64
    }

    fn monster_hits(&self, query: &str, folded: &str, lang: &str, places: &[i64], limit: Option<usize>) -> Vec<Hit> {
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

    fn item_category_hits(&self, lang: &str) -> Vec<Hit> {
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

    fn set_hits(&self, query: &str, folded: &str, lang: &str, group: &str) -> Vec<Hit> {
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

    fn item_has_subtypes(&self, category: &str) -> bool {
        let kind = if category == "other" { "" } else { category };
        let mut seen = HashSet::new();
        for item in &self.items {
            if item.category == kind && !item.subtype.is_empty() {
                seen.insert(item.subtype.as_str());
            }
        }
        seen.len() > 1
    }

    fn item_subtype_hits(&self, category: &str, lang: &str) -> Vec<Hit> {
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

    fn item_group_icon(&self, category: &str, subtype: &str) -> Option<(PathBuf, i32)> {
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

    fn item_hits(&self, query: &str, folded: &str, lang: &str, category: &str, subtype: &str) -> Vec<Hit> {
        let set = set_query(query);
        let limit = if set.is_some() {
            usize::MAX
        } else if query.is_empty() {
            80
        } else {
            40
        };
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

    fn source_hits(&self, lang: &str) -> Vec<Hit> {
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

    fn recipe_hits(&self, query: &str, folded: &str, lang: &str, source: &str) -> Vec<Hit> {
        let listed = |recipe: &RecipeRow| source.is_empty() || recipe.source == source;
        let mut matched: Vec<&RecipeRow> = self
            .recipes
            .iter()
            .filter(|recipe| listed(recipe) && self.name_hit(query, folded, &recipe.result_str, &recipe.names))
            .collect();
        if query.is_empty() {
            matched.sort_by_key(|recipe| (recipe.level_min.unwrap_or(i64::MAX), recipe.id));
        }
        let limit = if query.is_empty() { 80 } else { 40 };
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

    fn quest_source_hits(&self, lang: &str) -> Vec<Hit> {
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

    fn quest_hits(&self, query: &str, folded: &str, lang: &str, source: &str, kind: &str) -> Vec<Hit> {
        let limit = if query.is_empty() { 80 } else { 40 };
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

    fn npc(&self, code: i64) -> Option<&NpcRow> {
        if code <= 0 {
            return None;
        }
        self.npcs.iter().find(|npc| npc.code == code)
    }

    fn npc_face(&self, code: i64) -> Option<(PathBuf, i32)> {
        self.npc(code).and_then(|npc| npc.icon.clone())
    }

    fn npc_hits(&self, query: &str, folded: &str, lang: &str) -> Vec<Hit> {
        let limit = if query.is_empty() { 80 } else { 40 };
        let mut picked = Vec::new();
        for npc in &self.npcs {
            if !self.name_hit(query, folded, &npc.code.to_string(), &npc.names) {
                continue;
            }
            picked.push(npc);
            if picked.len() == limit {
                break;
            }
        }
        if picked.len() < limit {
            picked.extend(fuzzy_rows(&self.npcs, folded, limit - picked.len(), |npc| {
                !self.name_hit(query, folded, &npc.code.to_string(), &npc.names)
            }, |npc| &npc.names));
        }
        picked
            .into_iter()
            .map(|npc| {
                let (title, alt) = npc_names(lang, npc);
                Hit {
                    title,
                    subtitle: caption(&alt, lead(lang, &npc.map_en, &npc.map_zh)),
                    key: format!("npc:{}", npc.code),
                    icon: npc.icon.clone(),
                    solo: false,
                    aside: lead(lang, &npc.nation_en, &npc.nation_zh),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    fn hero_rank_hits(&self, lang: &str) -> Vec<Hit> {
        let mut counts: HashMap<i64, i64> = HashMap::new();
        for hero in &self.heroes {
            *counts.entry(hero.rank).or_default() += 1;
        }
        let mut ranks: Vec<i64> = counts.keys().copied().collect();
        ranks.sort_by_key(|rank| (hero_rank_order(*rank), *rank));
        ranks
            .into_iter()
            .map(|rank| {
                let (en, zh) = hero_rank_pair(rank);
                let count = format!("{} {}", commas(counts[&rank]), phrase("heroes", lang));
                let title = lead(lang, en, zh);
                Hit {
                    title: if title.is_empty() { rank.to_string() } else { title },
                    subtitle: caption(&trail(lang, en, zh), count),
                    key: format!("rank:{rank}"),
                    icon: None,
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    fn hero_land_hits(&self, lang: &str) -> Vec<Hit> {
        let mut counts: HashMap<i64, i64> = HashMap::new();
        for hero in &self.heroes {
            *counts.entry(hero.nation).or_default() += 1;
        }
        let mut masks: Vec<i64> = counts.keys().copied().collect();
        masks.sort_by_key(|mask| hero_land_order(*mask));
        masks
            .into_iter()
            .map(|mask| {
                let (en, zh) = hero_land_pair(mask);
                let count = format!("{} {}", commas(counts[&mask]), phrase("heroes", lang));
                Hit {
                    title: lead(lang, en, zh),
                    subtitle: caption(&trail(lang, en, zh), count),
                    key: format!("land:{mask}"),
                    icon: None,
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    fn hero_tier_hits(&self, nation: &str, lang: &str) -> Vec<Hit> {
        let Ok(nation) = nation.parse::<i64>() else { return Vec::new() };
        let mut counts: HashMap<i64, i64> = HashMap::new();
        for hero in self.heroes.iter().filter(|hero| hero.nation == nation) {
            *counts.entry(hero.rank).or_default() += 1;
        }
        let mut ranks: Vec<i64> = counts.keys().copied().collect();
        ranks.sort_by_key(|rank| (hero_rank_order(*rank), *rank));
        ranks
            .into_iter()
            .map(|rank| {
                let (en, zh) = hero_tier_pair(rank);
                let count = format!("{} {}", commas(counts[&rank]), phrase("heroes", lang));
                let title = lead(lang, en, zh);
                Hit {
                    title: if title.is_empty() { rank.to_string() } else { title },
                    subtitle: caption(&trail(lang, en, zh), count),
                    key: format!("band:{rank}"),
                    icon: None,
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    fn hero_hits(&self, query: &str, folded: &str, lang: &str, rank: &str, nation: &str) -> Vec<Hit> {
        let wanted = rank.parse::<i64>().ok();
        let land = nation.parse::<i64>().ok();
        let limit = if query.is_empty() { 80 } else { 40 };
        let listed = |hero: &HeroRow| {
            wanted.is_none_or(|rank| hero.rank == rank) && land.is_none_or(|nation| hero.nation == nation)
        };
        let mut picked = Vec::new();
        for hero in &self.heroes {
            if !listed(hero) || !self.name_hit(query, folded, &hero.code, &hero.names) {
                continue;
            }
            picked.push(hero);
            if picked.len() == limit {
                break;
            }
        }
        if picked.len() < limit {
            picked.extend(fuzzy_rows(&self.heroes, folded, limit - picked.len(), |hero| {
                listed(hero) && !self.name_hit(query, folded, &hero.code, &hero.names)
            }, |hero| &hero.names));
        }
        picked
            .into_iter()
            .map(|hero| {
                let (title, alt) = hero_names(lang, hero);
                let (en, zh) = hero_rank_pair(hero.rank);
                let rank_name = if wanted.is_some() { String::new() } else { lead(lang, en, zh) };
                Hit {
                    title,
                    subtitle: caption(&alt, rank_name),
                    key: format!("hero:{}", hero.code),
                    icon: hero.icon.clone(),
                    solo: false,
                    aside: lead(lang, &hero.nation_en, &hero.nation_zh),
                    badge: None,
                    chip: String::new(),
                }
            })
            .collect()
    }

    fn hero_chain(&self, code: &str, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        if let Some(steps) = self.chains.get(code) {
            if steps.len() >= 2 {
                for step in steps {
                    let Some(hero) = self.heroes.iter().find(|hero| hero.code == step.code) else { continue };
                    let (title, alt) = hero_names(lang, hero);
                    let mut note = scrub(&tidy(&lead(lang, &step.en, &step.zh)));
                    note = drop_leading_name(&note, &hero.zh);
                    note = drop_leading_name(&note, &hero.en);
                    note = drop_leading_name(&note, &hero.ko);
                    if note.chars().count() < 2 {
                        note.clear();
                    }
                    hits.push(Hit {
                        title,
                        subtitle: caption(&alt, note),
                        key: format!("hero:{}", step.code),
                        icon: hero.icon.clone(),
                        solo: false,
                        aside: chain_role(step.slot, lang),
                        badge: None,
                        chip: String::new(),
                    });
                }
            }
        }
        let mut blocks = Vec::new();
        self.push_fixed_beast(&mut blocks, code, lang);
        if let Some(steps) = self.chains.get(code) {
            for (index, step) in steps.iter().enumerate() {
                let Some(next) = steps.get(index + 1) else { continue };
                if next.code != code {
                    continue;
                }
                let source = self.heroes.iter().find(|hero| hero.code == step.code);
                let mut heading = scrub(&tidy(&lead(lang, &step.en, &step.zh)));
                if let Some(hero) = source {
                    heading = drop_leading_name(&heading, &hero.zh);
                    heading = drop_leading_name(&heading, &hero.en);
                    heading = drop_leading_name(&heading, &hero.ko);
                }
                if step.materials.is_empty() && heading.chars().count() < 2 {
                    continue;
                }
                if heading.chars().count() < 2 {
                    heading = phrase("upgrade", lang);
                }
                let from = source.map(|hero| hero_names(lang, hero).0).unwrap_or_default();
                if step.materials.is_empty() {
                    blocks.push(Hit {
                        title: heading,
                        subtitle: from,
                        key: String::new(),
                        icon: None,
                        solo: false,
                        aside: String::new(),
                        badge: None,
                        chip: "label".into(),
                    });
                } else {
                    self.push_material_block(&mut blocks, lang, heading, from, &step.materials);
                }
            }
        }
        if blocks.is_empty() {
            if let Some(hero) = self.heroes.iter().find(|hero| hero.code == code) {
                if matches!(hero.rank, 4 | 5 | 11) {
                    let hire = scrub(&tidy(&lead(lang, &hero.hire_en, &hero.hire_zh)));
                    if hire.chars().count() >= 2 {
                        blocks.push(Hit {
                            title: hire,
                            subtitle: String::new(),
                            key: String::new(),
                            icon: None,
                            solo: false,
                            aside: String::new(),
                            badge: None,
                            chip: "label".into(),
                        });
                    }
                }
            }
        }
        if let Some(recipes) = self.combos.get(code) {
            let mut routed = HashSet::new();
            for recipe in recipes.iter().filter(|recipe| recipe.result == code) {
                let result = self.heroes.iter().find(|hero| hero.code == recipe.result);
                let result_name = result.map(|hero| hero_names(lang, hero).0).unwrap_or_else(|| recipe.result.clone());
                let note = credit_note(&lead(lang, &recipe.en, &recipe.zh));
                blocks.push(Hit {
                    title: format!("{} · {result_name}", phrase("combine", lang)),
                    subtitle: note,
                    key: format!("hero:{}", recipe.result),
                    icon: result.and_then(|hero| hero.icon.clone()),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: "label".into(),
                });
                for part in &recipe.parts {
                    let Some(hero) = self.heroes.iter().find(|hero| hero.code == *part) else { continue };
                    let (title, alt) = hero_names(lang, hero);
                    blocks.push(Hit {
                        title,
                        subtitle: alt,
                        key: format!("hero:{part}"),
                        icon: hero.icon.clone(),
                        solo: false,
                        aside: phrase("combine", lang),
                        badge: None,
                        chip: String::new(),
                    });
                }
                for (item_code, qty) in &recipe.materials {
                    let item = self.find_item(*item_code);
                    let (name, alt) = item
                        .map(|item| (lead(lang, &item.en, &item.zh), trail(lang, &item.en, &item.zh)))
                        .unwrap_or_default();
                    blocks.push(Hit {
                        title: if name.is_empty() { item_code.to_string() } else { name },
                        subtitle: alt,
                        key: format!("item:{item_code}"),
                        icon: self.item_icons.get(item_code).cloned(),
                        solo: false,
                        aside: format!("× {}", commas(*qty)),
                        badge: None,
                        chip: String::new(),
                    });
                }
            }
            let mut routes = Vec::new();
            for recipe in recipes.iter().filter(|recipe| recipe.result != code) {
                if !routed.insert(recipe.result.clone()) {
                    continue;
                }
                let result = self.heroes.iter().find(|hero| hero.code == recipe.result);
                let result_name = result.map(|hero| hero_names(lang, hero).0).unwrap_or_else(|| recipe.result.clone());
                routes.push(Hit {
                    title: result_name,
                    subtitle: phrase("combine", lang),
                    key: format!("hero:{}", recipe.result),
                    icon: result.and_then(|hero| hero.icon.clone()),
                    solo: false,
                    aside: String::new(),
                    badge: None,
                    chip: String::new(),
                });
            }
            if !blocks.is_empty() {
                hits.push(hero_section("need", lang));
                hits.extend(blocks);
            }
            if !routes.is_empty() {
                hits.push(hero_section("related", lang));
                hits.extend(routes);
            }
        } else if !blocks.is_empty() {
            hits.push(hero_section("need", lang));
            hits.extend(blocks);
        }
        hits
    }

    fn push_material_block(&self, hits: &mut Vec<Hit>, lang: &str, title: String, subtitle: String, materials: &[(i64, i64)]) {
        hits.push(Hit {
            title,
            subtitle,
            key: String::new(),
            icon: None,
            solo: false,
            aside: String::new(),
            badge: None,
            chip: "label".into(),
        });
        for (code, qty) in materials {
            let item = self.find_item(*code);
            let (name, alt) = item
                .map(|item| (lead(lang, &item.en, &item.zh), trail(lang, &item.en, &item.zh)))
                .unwrap_or_default();
            hits.push(Hit {
                title: if name.is_empty() { code.to_string() } else { name },
                subtitle: alt,
                key: format!("item:{code}"),
                icon: self.item_icons.get(code).cloned(),
                solo: false,
                aside: format!("× {}", commas(*qty)),
                badge: None,
                chip: String::new(),
            });
        }
    }

    fn roster_hits(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        if let Some(uses) = self.roster_uses.get(&code) {
            hits.extend(uses.iter().map(|use_| {
                let target = self.heroes.iter().find(|hero| hero.code == use_.target);
                let source = self.heroes.iter().find(|hero| hero.code == use_.source);
                let target_name = target.map(|hero| hero_names(lang, hero).0).unwrap_or_else(|| use_.target.clone());
                let source_name = source.map(|hero| hero_names(lang, hero).0).unwrap_or_default();
                let kind = roster_kind(&use_.note, target.map(|hero| hero.zh.as_str()).unwrap_or(""), use_.combo, lang);
                let subtitle = if source_name.is_empty() || source_name == target_name {
                    kind
                } else {
                    format!("{kind} · {source_name}")
                };
                Hit {
                    title: target_name,
                    subtitle,
                    key: format!("hero:{}", use_.target),
                    icon: target.and_then(|hero| hero.icon.clone()),
                    solo: false,
                    aside: format!("× {}", commas(use_.qty)),
                    badge: None,
                    chip: String::new(),
                }
            }));
        }
        for (hero_code, qty) in fixed_beast_items(code) {
            let target = self.heroes.iter().find(|hero| hero.code == hero_code);
            let target_name = target.map(|hero| hero_names(lang, hero).0).unwrap_or_else(|| hero_code.to_string());
            hits.push(Hit {
                title: target_name,
                subtitle: phrase("combine", lang),
                key: format!("hero:{hero_code}"),
                icon: target.and_then(|hero| hero.icon.clone()),
                solo: false,
                aside: if qty == 0 { phrase("pick-one", lang) } else { format!("× {}", commas(qty)) },
                badge: None,
                chip: String::new(),
            });
        }
        hits
    }

    fn push_fixed_beast(&self, blocks: &mut Vec<Hit>, code: &str, lang: &str) {
        let Some(parts) = fixed_beast(code) else { return };
        let hero = self.heroes.iter().find(|hero| hero.code == code);
        let name = hero.map(|hero| hero_names(lang, hero).0).unwrap_or_else(|| code.to_string());
        let credit = fixed_credit(code);
        blocks.push(Hit {
            title: format!("{} · {name}", phrase("combine", lang)),
            subtitle: if credit > 0 { format!("{} {credit}", phrase("credit-level", lang)) } else { String::new() },
            key: format!("hero:{code}"),
            icon: hero.and_then(|hero| hero.icon.clone()),
            solo: false,
            aside: String::new(),
            badge: None,
            chip: "label".into(),
        });
        for part in parts {
            if part.item == 0 {
                blocks.push(Hit {
                    title: lead(lang, part.en, part.zh).to_string(),
                    subtitle: lead(lang, part.hint_en, part.hint_zh).to_string(),
                    key: String::new(),
                    icon: None,
                    solo: false,
                    aside: format!("× {}", commas(part.qty)),
                    badge: None,
                    chip: String::new(),
                });
                continue;
            }
            let item = self.find_item(part.item);
            let (title, alt) = item
                .map(|item| (lead(lang, &item.en, &item.zh), trail(lang, &item.en, &item.zh)))
                .unwrap_or_else(|| (part.item.to_string(), String::new()));
            let alt = if part.hint_zh.is_empty() { alt } else { lead(lang, part.hint_en, part.hint_zh).to_string() };
            blocks.push(Hit {
                title: if title.is_empty() { part.item.to_string() } else { title },
                subtitle: alt,
                key: format!("item:{}", part.item),
                icon: self.item_icons.get(&part.item).cloned(),
                solo: false,
                aside: if part.qty == 0 { phrase("pick-one", lang) } else { format!("× {}", commas(part.qty)) },
                badge: None,
                chip: String::new(),
            });
        }
    }

    fn listed_building(&self, code: &str) -> bool {
        !matches!(code, "LMB" | "MIN")
    }

    fn listed_facility(&self, code: &str) -> bool {
        self.works
            .iter()
            .any(|row| self.listed_building(&row.code) && row.facilities.iter().any(|slot| slot == code))
    }

    fn work_hits(&self, query: &str, folded: &str, lang: &str, building: &str, facility: &str) -> Vec<Hit> {
        if !facility.is_empty() {
            return self.work_products(facility, query, folded, lang);
        }
        if !building.is_empty() {
            return self.work_facilities(building, query, folded, lang);
        }
        if query.is_empty() {
            return self.work_buildings(lang);
        }
        let mut hits = self
            .works
            .iter()
            .filter(|row| self.listed_building(&row.code) && self.name_hit(query, folded, &row.code, &row.names))
            .map(|row| self.work_building_hit(row, lang))
            .collect::<Vec<_>>();
        for yard in self.yards.values() {
            if !self.listed_facility(&yard.code) {
                continue;
            }
            if self.name_hit(query, folded, &yard.code, &yard.names) {
                hits.push(self.work_facility_hit(yard, lang));
            }
        }
        for yard in self.yards.values() {
            if !self.listed_facility(&yard.code) {
                continue;
            }
            for product in &yard.products {
                let Some(item) = self.find_item(product.item) else { continue };
                if !self.name_hit(query, folded, &item.code_str, &item.names) {
                    continue;
                }
                let Some(hit) = self.work_product_hit(product, lang) else { continue };
                if hits.iter().any(|row| row.key == hit.key) {
                    continue;
                }
                hits.push(hit);
                if hits.len() == 80 {
                    return hits;
                }
            }
        }
        hits
    }

    fn work_buildings(&self, lang: &str) -> Vec<Hit> {
        self.works
            .iter()
            .filter(|row| self.listed_building(&row.code))
            .map(|row| self.work_building_hit(row, lang))
            .collect()
    }

    fn work_building_hit(&self, row: &WorkBuilding, lang: &str) -> Hit {
        let credit = format!("{} {}", phrase("credit", lang), commas(row.credit));
        Hit {
            title: lead(lang, &row.en, &row.zh),
            subtitle: caption(&trail(lang, &row.en, &row.zh), format!("{} · {credit}", commas(row.cost))),
            key: format!("yard:{}", row.code),
            icon: row.icon.clone(),
            solo: false,
            aside: String::new(),
            badge: None,
            chip: String::new(),
        }
    }

    fn work_facilities(&self, building: &str, query: &str, folded: &str, lang: &str) -> Vec<Hit> {
        if !self.listed_building(building) {
            return Vec::new();
        }
        let Some(row) = self.works.iter().find(|row| row.code == building) else {
            return Vec::new();
        };
        row.facilities
            .iter()
            .filter_map(|code| self.yards.get(code))
            .filter(|yard| self.name_hit(query, folded, &yard.code, &yard.names))
            .map(|yard| self.work_facility_hit(yard, lang))
            .collect()
    }

    fn work_facility_hit(&self, yard: &WorkFacility, lang: &str) -> Hit {
        let count = format!("{} {}", commas(yard.products.len() as i64), phrase("goods", lang));
        Hit {
            title: lead(lang, &yard.en, &yard.zh),
            subtitle: caption(&trail(lang, &yard.en, &yard.zh), count),
            key: format!("bay:{}", yard.code),
            icon: yard.icon.clone(),
            solo: false,
            aside: String::new(),
            badge: None,
            chip: String::new(),
        }
    }

    fn work_products(&self, facility: &str, query: &str, folded: &str, lang: &str) -> Vec<Hit> {
        if !self.listed_facility(facility) {
            return Vec::new();
        }
        let Some(yard) = self.yards.get(facility) else {
            return Vec::new();
        };
        yard.products
            .iter()
            .filter_map(|product| {
                let item = self.find_item(product.item)?;
                if !self.name_hit(query, folded, &item.code_str, &item.names) {
                    return None;
                }
                self.work_product_hit(product, lang)
            })
            .collect()
    }

    fn work_product_hit(&self, product: &WorkProduct, lang: &str) -> Option<Hit> {
        let item = self.find_item(product.item)?;
        let (title, alt) = (lead(lang, &item.en, &item.zh), trail(lang, &item.en, &item.zh));
        let title = if title.is_empty() { product.item.to_string() } else { title };
        Some(Hit {
            title,
            subtitle: alt,
            key: format!("item:{}", product.item),
            icon: self.item_icons.get(&product.item).cloned(),
            solo: false,
            aside: work_lands(&product.nations, lang),
            badge: None,
            chip: String::new(),
        })
    }

    fn find_item(&self, code: i64) -> Option<&ItemRow> {
        let key = code.to_string();
        self.items.iter().find(|item| item.code_str == key)
    }

    pub fn work_building_name(&self, code: &str, lang: &str) -> String {
        self.works
            .iter()
            .find(|row| row.code == code)
            .map(|row| lead(lang, &row.en, &row.zh))
            .unwrap_or_else(|| code.to_string())
    }

    pub fn work_facility_name(&self, code: &str, lang: &str) -> String {
        self.yards
            .get(code)
            .map(|row| lead(lang, &row.en, &row.zh))
            .unwrap_or_else(|| code.to_string())
    }

    pub fn work_parent(&self, facility: &str) -> String {
        self.works
            .iter()
            .find(|row| row.facilities.iter().any(|code| code == facility))
            .map(|row| row.code.clone())
            .unwrap_or_default()
    }

    fn atlas_hits(&self, query: &str, folded: &str, lang: &str, region: &str, town: &str) -> Vec<Hit> {
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

    fn realm_hits(&self, lang: &str) -> Vec<Hit> {
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

    fn town_hits(&self, region: &str, query: &str, folded: &str, lang: &str) -> Vec<Hit> {
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

    fn town_goods(&self, code: &str, query: &str, folded: &str, lang: &str) -> Vec<Hit> {
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

fn realm_pair(id: &str) -> (&'static str, &'static str) {
    match id {
        "joseon" => ("Joseon", "朝鮮"),
        "japan" => ("Japan", "日本"),
        "china" => ("China", "中國"),
        "taiwan" => ("Taiwan", "台灣"),
        "india" => ("India", "印度"),
        "mongolia" => ("Mongolia", "蒙古"),
        _ => ("", ""),
    }
}

struct MakePlace {
    region: String,
    building: String,
    facility: String,
    key: String,
    icon: Option<(PathBuf, i32)>,
}


fn formula_lands(code: i64, lang: &str) -> String {
    if code <= 0 {
        return String::new();
    }
    let flags = [(1, "land-k"), (10, "land-j"), (100, "land-c"), (1000, "land-tw")];
    let present = flags
        .into_iter()
        .filter(|(unit, _)| (code / unit) % 10 == 1)
        .map(|(_, key)| phrase(key, lang))
        .collect::<Vec<_>>();
    if present.len() >= 3 {
        return phrase("all-regions", lang);
    }
    present.join(" · ")
}

pub fn relation_label(key: &str, lang: &str) -> String {
    if let Some(label) = set_slot_heading(key, lang) {
        return label;
    }
    let phrase_key = match key {
        "sold-in" => "src-sold-in",
        "dropped-by" => "src-dropped-by",
        "craft-how" => "src-craft-how",
        "make" => "src-make",
        "used-for" => "src-used-for",
        "also-from" => "src-also-from",
        "roster" => "src-roster",
        "set" => "set",
        _ => return key.to_string(),
    };
    phrase(phrase_key, lang)
}

fn work_lands(nations: &str, lang: &str) -> String {
    let korea = nations.contains('K');
    let japan = nations.contains('J');
    let china = nations.contains('C');
    if korea && japan && china {
        return phrase("all-regions", lang);
    }
    let flags = [('K', "land-k"), ('J', "land-j"), ('C', "land-ct"), ('M', "land-m")];
    flags
        .into_iter()
        .filter(|(mark, _)| nations.contains(*mark))
        .map(|(_, key)| phrase(key, lang))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn chain_role(slot: i64, lang: &str) -> String {
    let key = match slot {
        1 => "job-base",
        2 => "job-first",
        3 => "job-second",
        4 => "job-remodel",
        _ => return String::new(),
    };
    phrase(key, lang)
}

fn npc_sheet_parts(npc: &NpcRow, lang: &str) -> (String, String, String, Vec<(String, String)>, String) {
    let (title, alt) = npc_names(lang, npc);
    let map = lead(lang, &npc.map_en, &npc.map_zh);
    let mut facts = Vec::new();
    let nation = lead(lang, &npc.nation_en, &npc.nation_zh);
    if !nation.is_empty() {
        facts.push((phrase("nation", lang), nation));
    }
    if !map.is_empty() {
        facts.push((phrase("map", lang), map.clone()));
    }
    facts.push((phrase("coord", lang), format!("{}, {}", commas(npc.x), commas(npc.y))));
    (title, alt, map, facts, lead(lang, &npc.place_en, &npc.place_zh))
}

fn npc_names(lang: &str, npc: &NpcRow) -> (String, String) {
    let title = lead(lang, &npc.en, &npc.zh);
    let alt = trail(lang, &npc.en, &npc.zh);
    if title.is_empty() {
        let ko = plain_name(&npc.ko);
        if ko.is_empty() { (npc.code.to_string(), String::new()) } else { (ko, String::new()) }
    } else {
        (title, alt)
    }
}

fn plain_name(name: &str) -> String {
    let trimmed = name.trim();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return trimmed.to_string();
    };
    rest.split_once(']').map(|(_, tail)| tail.trim().to_string()).unwrap_or_else(|| trimmed.to_string())
}

const SET_NICKS: &[(&str, &str, &str)] = &[
    ("龜天", "多聞天王", "다문천왕"),
    ("雀天", "持國天王", "지국천왕"),
    ("龍天", "增長天王", "증장천왕"),
    ("虎天", "廣目天王", "광목천왕"),
];

const SLOT_NICKS: &[(&str, &str)] = &[
    ("手", "G"),
    ("腰", "B"),
    ("鞋", "S"),
    ("頭", "H"),
    ("甲", "A"),
    ("戒", "R"),
];

fn set_query(query: &str) -> Option<(&'static str, &'static str, Option<&'static str>)> {
    for &(nick, zh, ko) in SET_NICKS {
        let Some(rest) = query.strip_prefix(nick) else { continue };
        if rest.is_empty() {
            return Some((zh, ko, None));
        }
        if let Some(&(_, category)) = SLOT_NICKS.iter().find(|(slot, _)| *slot == rest) {
            return Some((zh, ko, Some(category)));
        }
        return None;
    }
    None
}

fn plain_piece(item: &ItemRow) -> bool {
    const MARKS: &[&str] = &[
        "(", "（", "[", "日", "外觀", "覺醒", "活動", "回歸", "測試", "製造", "交易", "-", " ",
    ];
    matches!(item.category.as_str(), "G" | "B" | "S" | "H" | "A" | "R")
        && !item.zh.is_empty()
        && MARKS.iter().all(|mark| !item.zh.contains(mark))
}

fn name_match(query: &str, folded: &str, exact: &str, names: &str) -> bool {
    query.is_empty() || exact.eq_ignore_ascii_case(query) || (!folded.is_empty() && names.contains(folded))
}

fn text_hit(folded: &str, names: &str) -> bool {
    if folded.is_empty() {
        return false;
    }
    if names.contains(folded) {
        return true;
    }
    let budget = fuzzy_budget(folded);
    budget > 0 && fuzzy_dist(folded, names, budget).is_some()
}

fn haystack(parts: &[&str]) -> String {
    let mut out = String::new();
    for part in parts {
        let folded = part.trim().to_lowercase();
        if folded.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&folded);
    }
    for part in parts {
        for alias in nicknames(part) {
            let alias = alias.to_lowercase();
            if alias.chars().count() < 2 || out.split('\n').any(|line| line == alias) {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&alias);
        }
    }
    out
}

fn nicknames(name: &str) -> Vec<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 48 || name.contains(['[', ']', '【', '】']) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(alias) = latin_acronym(name) {
        out.push(alias);
    }
    if let Some(alias) = paren_alias(name) {
        if !out.iter().any(|have| have == &alias) {
            out.push(alias);
        }
    }
    if let Some(alias) = cjk_initials(name) {
        if !out.iter().any(|have| have == &alias) {
            out.push(alias);
        }
    }
    out
}

fn latin_acronym(name: &str) -> Option<String> {
    let words: Vec<&str> = name
        .split(|ch: char| !ch.is_ascii_alphabetic())
        .filter(|word| word.len() >= 2)
        .collect();
    if !(3..=5).contains(&words.len()) {
        return None;
    }
    Some(words.iter().filter_map(|word| word.chars().next()).map(|ch| ch.to_ascii_lowercase()).collect())
}

fn paren_alias(name: &str) -> Option<String> {
    let (body, inner) = paren_parts(name)?;
    if !short_cjk(inner) {
        return None;
    }
    let head = body.chars().find(|ch| is_cjk(*ch))?;
    Some(format!("{head}{inner}"))
}

fn cjk_initials(name: &str) -> Option<String> {
    if name.chars().any(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let body = match paren_parts(name) {
        Some((body, inner)) if short_cjk(inner) => body,
        _ => name,
    };
    let mut parts = Vec::new();
    let mut word = String::new();
    for ch in body.chars() {
        if matches!(ch, '之' | '的' | '與' | '和' | '及') {
            if !word.is_empty() {
                parts.push(std::mem::take(&mut word));
            }
        } else if is_cjk(ch) {
            word.push(ch);
        } else if !word.is_empty() {
            parts.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        parts.push(word);
    }
    if !(2..=4).contains(&parts.len()) || parts.iter().any(|part| part.chars().count() > 6) {
        return None;
    }
    let alias: String = parts.iter().filter_map(|part| part.chars().next()).collect();
    if (2..=4).contains(&alias.chars().count()) {
        Some(alias)
    } else {
        None
    }
}

fn paren_parts(name: &str) -> Option<(&str, &str)> {
    let chars: Vec<(usize, char)> = name.char_indices().collect();
    let mut open = None;
    let mut close = None;
    for (index, (_, ch)) in chars.iter().enumerate() {
        if *ch == '(' || *ch == '（' {
            open = Some(index);
        } else if (*ch == ')' || *ch == '）') && open.is_some() {
            close = Some(index);
        }
    }
    let open = open?;
    let close = close.filter(|close| *close > open)?;
    let body = name[..chars[open].0].trim();
    let inner_start = chars[open].0 + chars[open].1.len_utf8();
    let inner = name[inner_start..chars[close].0].trim();
    Some((body, inner))
}

fn short_cjk(text: &str) -> bool {
    let count = text.chars().count();
    (1..=2).contains(&count) && text.chars().all(is_cjk)
}

fn is_cjk(ch: char) -> bool {
    matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}')
}

fn fuzzy_budget(query: &str) -> u8 {
    match query.chars().count() {
        0..=2 => 0,
        3..=8 => 1,
        9..=24 => 2,
        _ => 0,
    }
}

fn fuzzy_rows<'a, T>(
    rows: &'a [T],
    folded: &str,
    room: usize,
    take: impl Fn(&T) -> bool,
    names: impl Fn(&T) -> &str,
) -> Vec<&'a T> {
    let budget = fuzzy_budget(folded);
    if budget == 0 || room == 0 {
        return Vec::new();
    }
    let query: Vec<char> = folded.chars().collect();
    let mut found = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if !take(row) {
            continue;
        }
        let names = names(row);
        if !query.iter().any(|ch| names.contains(*ch)) {
            continue;
        }
        let Some(dist) = fuzzy_dist_chars(&query, names, budget) else {
            continue;
        };
        found.push((dist, index, row));
    }
    found.sort_by_key(|(dist, index, _)| (*dist, *index));
    found.into_iter().take(room).map(|(_, _, row)| row).collect()
}

fn fuzzy_dist(query: &str, names: &str, budget: u8) -> Option<u8> {
    if budget == 0 {
        return None;
    }
    let chars: Vec<char> = query.chars().collect();
    fuzzy_dist_chars(&chars, names, budget)
}

fn fuzzy_dist_chars(query: &[char], names: &str, budget: u8) -> Option<u8> {
    let mut best: Option<u8> = None;
    for line in names.split('\n') {
        if line.is_empty() {
            continue;
        }
        let limit = best.map(|have| have.saturating_sub(1)).unwrap_or(budget);
        relax(&mut best, bounded_edit(query, line, limit));
        if best == Some(1) {
            return best;
        }
        if !line.is_ascii() {
            continue;
        }
        for word in line.split(|ch: char| !ch.is_ascii_alphabetic()) {
            if word.len() < 3 {
                continue;
            }
            let limit = best.map(|have| have.saturating_sub(1)).unwrap_or(budget);
            if limit == 0 {
                return best;
            }
            relax(&mut best, bounded_edit(query, word, limit));
            if best == Some(1) {
                return best;
            }
        }
    }
    best
}

fn relax(best: &mut Option<u8>, next: Option<u8>) {
    if let Some(dist) = next {
        if best.map(|have| dist < have).unwrap_or(true) {
            *best = Some(dist);
        }
    }
}

fn bounded_edit(query: &[char], text: &str, budget: u8) -> Option<u8> {
    if budget == 0 || query.len() > 48 {
        return None;
    }
    let width = query.len();
    let max = budget as usize;
    let mut prev = [0u8; 49];
    for index in 0..=width {
        prev[index] = index as u8;
    }
    let mut length = 0usize;
    for ch in text.chars() {
        length += 1;
        if length > width + max {
            return None;
        }
        let mut curr = [0u8; 49];
        curr[0] = length as u8;
        let mut row_min = curr[0];
        for index in 1..=width {
            let cost = if query[index - 1] == ch { 0 } else { 1 };
            let value = (prev[index] + 1).min(curr[index - 1] + 1).min(prev[index - 1] + cost);
            curr[index] = value;
            row_min = row_min.min(value);
        }
        if row_min > budget {
            return None;
        }
        prev = curr;
    }
    if (width as i32 - length as i32).unsigned_abs() > max as u32 {
        return None;
    }
    let dist = prev[width];
    if dist == 0 || dist > budget {
        None
    } else {
        Some(dist)
    }
}

const DEFAULT_ALIAS_LINE: &str = "SSA 6213 6214 6215 6216 6943";

pub fn alias_editor_text() -> String {
    alias_editor_text_in(&data_dir())
}

fn alias_editor_text_in(dir: &Path) -> String {
    let path = dir.join("aliases.txt");
    if !path.is_file() {
        let _ = std::fs::write(&path, format!("# 一行一個簡稱，後面寫道具編號。\n{DEFAULT_ALIAS_LINE}\n"));
        return DEFAULT_ALIAS_LINE.to_string();
    }
    alias_entry_text(&std::fs::read_to_string(&path).unwrap_or_default())
}

pub fn alias_rows() -> Vec<(String, String)> {
    alias_rows_from(&alias_editor_text())
}

fn alias_rows_from(text: &str) -> Vec<(String, String)> {
    let mut order = Vec::new();
    let mut shown: HashMap<String, String> = HashMap::new();
    let mut codes: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let tokens = alias_tokens(line);
        if tokens.len() < 2 {
            continue;
        }
        let rest_are_codes = tokens[1..].iter().all(|token| is_code_token(token));
        if !is_code_token(&tokens[0]) && rest_are_codes {
            remember_alias(&mut order, &mut shown, &mut codes, &tokens[0], &tokens[1..]);
        } else {
            let code = code_key(&tokens[0]);
            for alias in &tokens[1..] {
                remember_alias(&mut order, &mut shown, &mut codes, alias, &[code.clone()]);
            }
        }
    }
    order
        .into_iter()
        .map(|key| (shown.remove(&key).unwrap_or(key.clone()), codes.remove(&key).unwrap_or_default().join(" ")))
        .collect()
}

fn remember_alias(
    order: &mut Vec<String>,
    shown: &mut HashMap<String, String>,
    codes: &mut HashMap<String, Vec<String>>,
    alias: &str,
    incoming: &[String],
) {
    let display = alias.trim().trim_start_matches('#').to_string();
    let key = display.to_lowercase();
    if key.chars().count() < 2 {
        return;
    }
    if !shown.contains_key(&key) {
        order.push(key.clone());
        shown.insert(key.clone(), display);
    }
    let list = codes.entry(key).or_default();
    for code in incoming {
        let code = code_key(code);
        if !code.is_empty() && !list.contains(&code) {
            list.push(code);
        }
    }
}

fn alias_tokens(line: &str) -> Vec<String> {
    line.split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn alias_entry_text(raw: &str) -> String {
    raw.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn save_alias_text(text: &str) {
    let body = text.replace('\r', "").trim().to_string();
    let _ = std::fs::write(
        data_dir().join("aliases.txt"),
        format!("# 一行一個簡稱，後面寫道具編號。\n{body}\n"),
    );
}

fn parse_alias_book(text: &str) -> HashMap<String, HashSet<String>> {
    let mut book = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let tokens: Vec<String> = line
            .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_string)
            .collect();
        if tokens.len() < 2 {
            continue;
        }
        let rest_are_codes = tokens[1..].iter().all(|token| is_code_token(token));
        if !is_code_token(&tokens[0]) && rest_are_codes {
            let alias = tokens[0].trim_start_matches('#').to_string();
            for code in &tokens[1..] {
                add_alias(&mut book, &code_key(code), &alias);
            }
        } else {
            let code = code_key(&tokens[0]);
            for alias in &tokens[1..] {
                add_alias(&mut book, &code, alias);
            }
        }
    }
    book
}

fn is_code_token(token: &str) -> bool {
    let code = code_key(token);
    !code.is_empty() && code.chars().all(|ch| ch.is_ascii_digit())
}

fn code_key(token: &str) -> String {
    let token = token.trim().trim_start_matches('#');
    token.split_once(':').map(|(_, code)| code.trim()).unwrap_or(token).to_string()
}

fn add_alias(book: &mut HashMap<String, HashSet<String>>, code: &str, alias: &str) {
    let alias = alias.trim().trim_start_matches('#').to_lowercase();
    if code.is_empty() || alias.chars().count() < 2 {
        return;
    }
    book.entry(alias).or_default().insert(code.to_string());
}

fn alias_codes(book: &HashMap<String, HashSet<String>>, folded: &str) -> HashSet<String> {
    if folded.chars().count() < 2 {
        return HashSet::new();
    }
    let mut codes = HashSet::new();
    if let Some(found) = book.get(folded) {
        codes.extend(found.iter().cloned());
    }
    if folded.chars().count() >= 3 {
        for (alias, found) in book {
            if alias != folded && alias.starts_with(folded) {
                codes.extend(found.iter().cloned());
            }
        }
    }
    codes
}

fn hero_sheet_parts(hero: &HeroRow, lang: &str) -> (String, String, String, Vec<(String, String)>, String) {
    let (title, alt) = hero_names(lang, hero);
    let (en, zh) = hero_rank_pair(hero.rank);
    let rank = lead(lang, en, zh);
    let heading = if rank.is_empty() {
        hero.code.clone()
    } else {
        format!("{rank}  {}", hero.code)
    };
    let mut facts = Vec::new();
    let nation = lead(lang, &hero.nation_en, &hero.nation_zh);
    if !nation.is_empty() {
        facts.push((phrase("nation", lang), nation));
    }
    let hire = tidy(&lead(lang, &hero.hire_en, &hero.hire_zh));
    if !hire.is_empty() {
        facts.push((phrase("hire", lang), hire));
    }
    for skill in &hero.skills {
        let text = tidy(&lead(lang, &skill.en, &skill.zh));
        let shown = if text.is_empty() { skill.name.clone() } else { text };
        if shown.is_empty() {
            continue;
        }
        let label = if skill.name.is_empty() { phrase("skill", lang) } else { skill.name.clone() };
        facts.push((label, shown));
    }
    (title, alt, heading, facts, tidy(&lead(lang, &hero.desc_en, &hero.desc_zh)))
}

fn hero_names(lang: &str, hero: &HeroRow) -> (String, String) {
    let title = lead(lang, &hero.en, &hero.zh);
    let alt = trail(lang, &hero.en, &hero.zh);
    if title.is_empty() {
        let ko = plain_name(&hero.ko);
        if ko.is_empty() { (hero.code.clone(), String::new()) } else { (ko, String::new()) }
    } else {
        (title, alt)
    }
}

fn hero_rank_pair(rank: i64) -> (&'static str, &'static str) {
    match rank {
        0 => ("Mercenary", "傭兵"),
        1 => ("1st general", "一次將帥"),
        2 => ("2nd general", "二次將帥"),
        3 => ("Remodel", "改造"),
        4 => ("Monster", "怪物"),
        5 => ("Promoted", "轉職怪物"),
        6 => ("Summon", "幻獸"),
        7 => ("Fiend", "凶獸"),
        8 => ("Divine", "神獸"),
        9 => ("Four Kings", "四天王"),
        10 => ("Awakened", "覺醒"),
        11 => ("Legend", "傳說"),
        12 => ("Wisdom King", "明王"),
        _ => ("", ""),
    }
}

fn hero_tier_pair(rank: i64) -> (&'static str, &'static str) {
    match rank {
        1 => ("1st class", "一轉"),
        2 => ("2nd class", "二轉"),
        _ => hero_rank_pair(rank),
    }
}

fn hero_land_pair(mask: i64) -> (&'static str, &'static str) {
    match mask {
        1 => ("Joseon", "朝鮮"),
        10 => ("Japan", "日本"),
        100 => ("China", "中國"),
        1000 => ("Taiwan", "台灣"),
        10000 => ("India", "印度"),
        _ => ("Other", "其他"),
    }
}

fn hero_land_order(mask: i64) -> usize {
    match mask {
        1 => 0,
        10 => 1,
        100 => 2,
        1000 => 3,
        10000 => 4,
        _ => 5,
    }
}

fn hero_rank_order(rank: i64) -> usize {
    [0, 1, 2, 3, 10, 11, 12, 9, 6, 7, 8, 5, 4]
        .iter()
        .position(|item| *item == rank)
        .unwrap_or(50)
}

fn hero_nation(code: i64) -> (String, String) {
    let (en, zh) = hero_land_pair(code);
    (en.to_string(), zh.to_string())
}

fn tidy(text: &str) -> String {
    text.replace("\\n", " ").replace(['\r', '\n'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn drop_leading_name(text: &str, name: &str) -> String {
    let name = name.trim();
    if name.chars().count() < 2 {
        return text.to_string();
    }
    let Some(rest) = text.trim().strip_prefix(name) else {
        return text.to_string();
    };
    rest.trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '·' | '：' | ':' | ',' | '，' | '、'))
        .trim()
        .to_string()
}

fn requirement_line(raw: &str) -> String {
    let raw = raw.replace("\\n", "\n");
    let mut lines = raw.lines().map(str::trim).filter(|line| !line.is_empty());
    let head = lines.next().unwrap_or("");
    let mut text = head.to_string();
    for line in lines {
        if line.contains("信用") || line.to_lowercase().contains("credit") {
            text.push(' ');
            text.push_str(line);
        }
    }
    scrub(&tidy(&text))
}

fn scrub(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if matches!(ch, ',' | '，' | '、' | '[' | ']' | '［' | '］' | '<' | '>' | '＜' | '＞') {
            continue;
        }
        out.push(ch);
    }
    tidy(&out)
}

fn credit_note(text: &str) -> String {
    let text = text.replace("\\n", " ");
    let start = text.find('(').or_else(|| text.find('（'));
    let Some(start) = start else { return String::new() };
    let rest = &text[start..];
    let end = rest.find(')').or_else(|| rest.find('）')).map(|index| index + 1).unwrap_or(rest.len());
    let bit = scrub(&tidy(&rest[..end]));
    if bit.contains("信用") || bit.to_lowercase().contains("credit") { bit } else { String::new() }
}

struct FoundMats {
    items: Vec<(i64, i64)>,
}

struct NameNode {
    next: HashMap<char, usize>,
    owners: Vec<(i64, bool)>,
}

struct NameTrie {
    nodes: Vec<NameNode>,
}

impl NameTrie {
    fn from_items(items: &[ItemRow]) -> Self {
        let mut trie = Self { nodes: vec![NameNode { next: HashMap::new(), owners: Vec::new() }] };
        for item in items {
            let Ok(code) = item.code_str.parse::<i64>() else { continue };
            for raw in [&item.zh, &item.en] {
                let raw = raw.trim();
                if raw.chars().count() < 2 {
                    continue;
                }
                let full = fold_item_name(raw, false);
                let loose = fold_item_name(raw, true);
                if full.chars().count() >= 2 {
                    trie.insert(&full, code, true);
                }
                if loose.chars().count() >= 2 && loose != full {
                    trie.insert(&loose, code, false);
                }
            }
        }
        trie
    }

    fn insert(&mut self, key: &str, code: i64, full: bool) {
        let mut node = 0;
        for ch in key.chars() {
            if let Some(next) = self.nodes[node].next.get(&ch).copied() {
                node = next;
                continue;
            }
            let id = self.nodes.len();
            self.nodes.push(NameNode { next: HashMap::new(), owners: Vec::new() });
            self.nodes[node].next.insert(ch, id);
            node = id;
        }
        if !self.nodes[node].owners.iter().any(|(have, kind)| *have == code && *kind == full) {
            self.nodes[node].owners.push((code, full));
        }
    }

    fn find(&self, text: &str) -> FoundMats {
        let (folded, origin) = fold_note(text);
        let chars: Vec<char> = folded.chars().collect();
        let mut found = Vec::new();
        for start in 0..chars.len() {
            let mut node = 0;
            for (offset, ch) in chars[start..].iter().enumerate() {
                let Some(next) = self.nodes[node].next.get(ch).copied() else { break };
                node = next;
                if offset + 1 < 2 {
                    continue;
                }
                let Some(code) = choose_name(&self.nodes[node].owners) else { continue };
                found.push((start, start + offset + 1, code));
            }
        }
        found.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(a.0.cmp(&b.0)));
        let mut taken = vec![false; chars.len()];
        let mut picked = Vec::new();
        for (start, end, code) in found {
            if taken[start..end].iter().any(|flag| *flag) {
                continue;
            }
            let raw_start = origin[start];
            let raw_end = origin[end - 1] + text[origin[end - 1]..].chars().next().map(|ch| ch.len_utf8()).unwrap_or(0);
            let (qty, span_start, span_end) = if let Some((qty, qty_end)) = qty_after(text, raw_end) {
                (qty, raw_start, qty_end)
            } else if let Some((qty, qty_start)) = qty_before(text, raw_start) {
                (qty, qty_start, raw_end)
            } else {
                continue;
            };
            if qty <= 0 {
                continue;
            }
            taken[start..end].fill(true);
            picked.push((span_start, span_end, code, qty));
        }
        FoundMats {
            items: picked.into_iter().map(|(_, _, code, qty)| (code, qty)).collect(),
        }
    }
}

fn choose_name(owners: &[(i64, bool)]) -> Option<i64> {
    let mut exact: Vec<i64> = owners.iter().filter(|(_, full)| *full).map(|(code, _)| *code).collect();
    exact.sort_unstable();
    exact.dedup();
    if !exact.is_empty() {
        return exact.first().copied();
    }
    let mut loose: Vec<i64> = owners.iter().map(|(code, _)| *code).collect();
    loose.sort_unstable();
    loose.dedup();
    if loose.len() == 1 { loose.first().copied() } else { None }
}

fn qty_after(text: &str, end: usize) -> Option<(i64, usize)> {
    let rest = text.get(end..)?;
    let skipped = rest.len() - rest.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, ')' | '）' | ']' | '］' | '>' | '＞' | '】' | '」' | '』' | 'x' | 'X' | '×')
    }).len();
    let rest = &rest[skipped..];
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let mut used = skipped + digits.len();
    if rest[digits.len()..].starts_with('個') {
        used += '個'.len_utf8();
    }
    Some((digits.parse().ok()?, end + used))
}

fn qty_before(text: &str, start: usize) -> Option<(i64, usize)> {
    let head = text.get(..start)?;
    let chars: Vec<(usize, char)> = head.char_indices().collect();
    let mut index = chars.len();
    while index > 0 && chars[index - 1].1.is_whitespace() {
        index -= 1;
    }
    if index >= 2 && chars[index - 2].1 == '一' && matches!(chars[index - 1].1, '個' | '个') {
        return Some((1, chars[index - 2].0));
    }
    if index == 0 || chars[index - 1].1 != '個' {
        return None;
    }
    index -= 1;
    while index > 0 && chars[index - 1].1.is_whitespace() {
        index -= 1;
    }
    let digit_end = index;
    while index > 0 && chars[index - 1].1.is_ascii_digit() {
        index -= 1;
    }
    if index == digit_end {
        return None;
    }
    let digits: String = chars[index..digit_end].iter().map(|(_, ch)| *ch).collect();
    Some((digits.parse().ok()?, chars[index].0))
}

fn fold_note(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::new();
    let mut origin = Vec::new();
    for (byte, ch) in text.char_indices() {
        for piece in ch.nfkc() {
            let piece = if piece == '凶' { '兇' } else { piece };
            if piece.is_whitespace() || name_punct(piece) {
                continue;
            }
            folded.push(piece.to_ascii_lowercase());
            origin.push(byte);
        }
    }
    (folded, origin)
}

fn fold_item_name(text: &str, drop_paren: bool) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for ch in text.nfkc() {
        let ch = if ch == '凶' { '兇' } else { ch };
        if drop_paren && matches!(ch, '(' | '（') {
            depth += 1;
            continue;
        }
        if drop_paren && matches!(ch, ')' | '）') {
            if depth > 0 {
                depth -= 1;
            }
            continue;
        }
        if drop_paren && depth > 0 {
            continue;
        }
        if ch.is_whitespace() || name_punct(ch) {
            continue;
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

fn name_punct(ch: char) -> bool {
    matches!(
        ch,
        '的' | '之'
            | '<' | '>' | '＜' | '＞'
            | '[' | ']' | '［' | '］'
            | '(' | ')' | '（' | '）'
            | '`' | '\'' | '’' | '"' | '「' | '」' | '『' | '』'
            | '·'
    )
}

fn load_chains(conn: &Connection, texts: &HashMap<String, String>, items: &[ItemRow]) -> Result<ChainBook, String> {
    if !table_exists(conn, "job_steps") {
        return Ok(ChainBook { lines: HashMap::new(), combos: HashMap::new(), uses: HashMap::new() });
    }
    let names = NameTrie::from_items(items);
    let has_kind = conn
        .prepare("SELECT kind FROM job_steps LIMIT 1")
        .is_ok();
    let sql = if has_kind {
        "SELECT chain_id, slot, code, note_gts, kind FROM job_steps ORDER BY chain_id, slot"
    } else {
        "SELECT chain_id, slot, code, note_gts, 2 FROM job_steps ORDER BY chain_id, slot"
    };
    let mut stmt = conn.prepare(sql).map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut grouped: HashMap<i64, (i64, Vec<ChainStep>)> = HashMap::new();
    for row in rows {
        let (chain_id, slot, code, note, kind) = row.map_err(|err| err.to_string())?;
        let en_raw = online(texts, note, "en").to_string();
        let zh_raw = online(texts, note, "zh").to_string();
        let en_found = names.find(&en_raw);
        let zh_found = names.find(&zh_raw);
        let materials = if zh_found.items.is_empty() { en_found.items.clone() } else { zh_found.items.clone() };
        let en = if materials.is_empty() { en_raw } else { requirement_line(&en_raw) };
        let zh = if materials.is_empty() { zh_raw } else { requirement_line(&zh_raw) };
        grouped.entry(chain_id).or_insert_with(|| (kind, Vec::new())).1.push(ChainStep {
            slot,
            code,
            en,
            zh,
            materials,
        });
    }
    let extras = load_job_materials(conn)?;
    let mut lines = HashMap::new();
    let mut combos: HashMap<String, Vec<Combo>> = HashMap::new();
    let mut uses: HashMap<i64, Vec<RosterUse>> = HashMap::new();
    for (chain_id, (kind, mut steps)) in grouped {
        if steps.len() < 2 {
            continue;
        }
        if let Some(extra) = extras.get(&chain_id) {
            if let Some(last) = steps.last_mut() {
                last.materials.extend(extra.clone());
            }
        }
        if kind == 1 || kind == 3 {
            let result = steps.last().map(|step| step.code.clone()).unwrap_or_default();
            let parts: Vec<String> = steps.iter().take(steps.len() - 1).map(|step| step.code.clone()).collect();
            let owner = steps.iter().find(|step| !step.materials.is_empty()).unwrap_or(&steps[0]);
            let recipe = Combo {
                result: result.clone(),
                parts: parts.clone(),
                en: owner.en.clone(),
                zh: owner.zh.clone(),
                materials: owner.materials.clone(),
            };
            for code in parts.iter().chain(std::iter::once(&result)) {
                combos.entry(code.clone()).or_default().push(recipe.clone());
            }
            let source = parts.first().cloned().unwrap_or_default();
            for (item, qty) in &recipe.materials {
                remember_use(&mut uses, *item, *qty, result.clone(), source.clone(), true, recipe.zh.clone());
            }
            continue;
        }
        for (index, step) in steps.iter().enumerate() {
            if step.materials.is_empty() {
                continue;
            }
            let target = steps.get(index + 1).map(|next| next.code.clone()).unwrap_or_else(|| step.code.clone());
            for (item, qty) in &step.materials {
                remember_use(&mut uses, *item, *qty, target.clone(), step.code.clone(), false, step.zh.clone());
            }
        }
        for step in &steps {
            lines.insert(step.code.clone(), steps.clone());
        }
    }
    Ok(ChainBook { lines, combos, uses })
}

fn load_job_materials(conn: &Connection) -> Result<HashMap<i64, Vec<(i64, i64)>>, String> {
    let mut found: HashMap<i64, Vec<(i64, i64)>> = HashMap::new();
    if !table_exists(conn, "job_materials") {
        return Ok(found);
    }
    let mut stmt = conn
        .prepare("SELECT chain_id, item_code, qty FROM job_materials ORDER BY chain_id, slot")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)))
        .map_err(|err| err.to_string())?;
    for row in rows {
        let (chain_id, item, qty) = row.map_err(|err| err.to_string())?;
        found.entry(chain_id).or_default().push((item, qty));
    }
    Ok(found)
}

fn remember_use(uses: &mut HashMap<i64, Vec<RosterUse>>, item: i64, qty: i64, target: String, source: String, combo: bool, note: String) {
    let rows = uses.entry(item).or_default();
    if rows.iter().any(|row| row.target == target && row.qty == qty && row.combo == combo) {
        return;
    }
    rows.push(RosterUse { qty, target, source, combo, note });
}

#[derive(Clone, Copy)]
struct FixedPart {
    item: i64,
    qty: i64,
    zh: &'static str,
    en: &'static str,
    hint_zh: &'static str,
    hint_en: &'static str,
}

const fn part(item: i64, qty: i64, hint_zh: &'static str, hint_en: &'static str) -> FixedPart {
    FixedPart { item, qty, zh: "", en: "", hint_zh, hint_en }
}

fn fixed_beasts() -> &'static [(&'static str, &'static [FixedPart])] {
    const NOTE: FixedPart = FixedPart {
        item: 0,
        qty: 3,
        zh: "不同幻獸石像",
        en: "Different fey statues",
        hint_zh: "",
        hint_en: "",
    };
    const STATUES: &[FixedPart] = &[
        part(3817, 0, "", ""),
        part(3828, 0, "", ""),
        part(3839, 0, "", ""),
        part(3850, 0, "", ""),
    ];
    const TIGER: &[FixedPart] = &[NOTE, STATUES[0], STATUES[1], STATUES[2], STATUES[3], part(3895, 10, "", ""), part(3506, 3, "", "")];
    const BIRD: &[FixedPart] = &[NOTE, STATUES[0], STATUES[1], STATUES[2], STATUES[3], part(3896, 10, "", ""), part(3506, 3, "", "")];
    const TORTOISE: &[FixedPart] = &[NOTE, STATUES[0], STATUES[1], STATUES[2], STATUES[3], part(3897, 10, "", ""), part(3506, 3, "", "")];
    const DRAGON: &[FixedPart] = &[NOTE, STATUES[0], STATUES[1], STATUES[2], STATUES[3], part(3898, 10, "", ""), part(3506, 3, "", "")];
    const KIRIN: &[FixedPart] = &[
        part(3894, 1, "", ""),
        part(3861, 1, "", ""),
        part(3872, 1, "", ""),
        part(3883, 1, "", ""),
        part(3899, 10, "", ""),
        part(3506, 4, "", ""),
    ];
    &[
        ("E74", TIGER),
        ("E75", BIRD),
        ("E76", DRAGON),
        ("E77", TORTOISE),
        ("E85", KIRIN),
    ]
}

fn fixed_credit(code: &str) -> i64 {
    match code {
        "E85" => 100,
        "E74" | "E75" | "E76" | "E77" => 95,
        _ => 0,
    }
}

fn fixed_beast(code: &str) -> Option<&'static [FixedPart]> {
    fixed_beasts().iter().find(|(hero, _)| *hero == code).map(|(_, parts)| *parts)
}

fn fixed_beast_items(item: i64) -> Vec<(&'static str, i64)> {
    let mut found = Vec::new();
    for (hero, parts) in fixed_beasts() {
        for part in *parts {
            if part.item == item {
                found.push((*hero, part.qty));
            }
        }
    }
    found
}

fn hero_section(key: &str, lang: &str) -> Hit {
    Hit {
        title: if key == "related" { phrase("related", lang) } else { phrase("craft-need", lang) },
        subtitle: String::new(),
        key: key.to_string(),
        icon: None,
        solo: false,
        aside: String::new(),
        badge: None,
        chip: "section".into(),
    }
}

fn roster_kind(note: &str, target_zh: &str, combo: bool, lang: &str) -> String {
    let key = if combo {
        "combine"
    } else if note.contains("覺醒") || target_zh.contains("覺醒") {
        "awaken"
    } else if note.contains("改造") {
        "remodel"
    } else if note.contains("轉職") {
        "promote"
    } else {
        "upgrade"
    };
    phrase(key, lang)
}

fn load_heroes(
    conn: &Connection,
    client_root: &Path,
    texts: &HashMap<String, String>,
) -> Result<Vec<HeroRow>, String> {
    if !table_exists(conn, "characters") {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT code, nation, rank, name_ko, name_gts, desc_gts, hire_gts, portrait_path, portrait_frame, sprite_path
             FROM characters",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let skills = load_hero_skills(conn, texts)?;
    let mut heroes = Vec::new();
    for row in rows {
        let (code, nation, rank, ko, name_gts, desc_gts, hire_gts, portrait, frame, sprite) =
            row.map_err(|err| err.to_string())?;
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        let desc_en = online(texts, desc_gts, "en").to_string();
        let desc_zh = online(texts, desc_gts, "zh").to_string();
        let hire_en = online(texts, hire_gts, "en").to_string();
        let hire_zh = online(texts, hire_gts, "zh").to_string();
        let (nation_en, nation_zh) = hero_nation(nation);
        let (rank_en, rank_zh) = hero_rank_pair(rank);
        let mine = skills.get(&code).cloned().unwrap_or_default();
        let mut parts = vec![ko.clone(), plain_name(&ko), en.clone(), zh.clone(), nation_en.clone(), nation_zh.clone()];
        for skill in &mine {
            parts.push(skill.name.clone());
            parts.push(skill.en.clone());
            parts.push(skill.zh.clone());
        }
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).chain([rank_en, rank_zh]).collect();
        heroes.push(HeroRow {
            code: code.clone(),
            nation,
            rank,
            ko,
            en,
            zh,
            nation_en,
            nation_zh,
            desc_en,
            desc_zh,
            hire_en,
            hire_zh,
            names: haystack(&borrowed),
            skills: mine,
            icon: hero_icon(client_root, &portrait, frame, &sprite),
        });
    }
    heroes.sort_by(|a, b| a.zh.cmp(&b.zh).then(a.en.cmp(&b.en)).then(a.code.cmp(&b.code)));
    Ok(heroes)
}

fn load_works(
    conn: &Connection,
    client_root: &Path,
    texts: &HashMap<String, String>,
) -> Result<(Vec<WorkBuilding>, HashMap<String, WorkFacility>), String> {
    if !table_exists(conn, "work_buildings") {
        return Ok((Vec::new(), HashMap::new()));
    }
    let upgrades = load_work_upgrades(conn, texts)?;
    let mut facility_upgrades: HashMap<String, Vec<WorkUpgradeName>> = HashMap::new();
    if table_exists(conn, "work_facility_upgrades") {
        let mut stmt = conn
            .prepare("SELECT facility, upgrade FROM work_facility_upgrades ORDER BY facility, slot")
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|err| err.to_string())?;
        for row in rows {
            let (facility, upgrade) = row.map_err(|err| err.to_string())?;
            if let Some(name) = upgrades.get(&upgrade) {
                facility_upgrades.entry(facility).or_default().push(name.clone());
            }
        }
    }
    let mut products: HashMap<String, Vec<WorkProduct>> = HashMap::new();
    if table_exists(conn, "work_products") {
        let mut stmt = conn
            .prepare("SELECT facility, item_code, nations FROM work_products ORDER BY facility, slot")
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)))
            .map_err(|err| err.to_string())?;
        for row in rows {
            let (facility, item, nations) = row.map_err(|err| err.to_string())?;
            products.entry(facility).or_default().push(WorkProduct { item, nations });
        }
    }
    let mut yards = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT code, name_ko, name_gts FROM work_facilities")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))
        .map_err(|err| err.to_string())?;
    for row in rows {
        let (code, ko, gts) = row.map_err(|err| err.to_string())?;
        let en = online(texts, gts, "en").to_string();
        let zh = online(texts, gts, "zh").to_string();
        let mine = products.remove(&code).unwrap_or_default();
        let mut parts = vec![ko, code.clone(), en.clone(), zh.clone()];
        for product in &mine {
            parts.push(product.item.to_string());
        }
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        yards.insert(
            code.clone(),
            WorkFacility {
                code: code.clone(),
                en,
                zh,
                names: haystack(&borrowed),
                upgrades: facility_upgrades.remove(&code).unwrap_or_default(),
                products: mine,
                icon: facility_art(client_root, &code),
            },
        );
    }
    let mut slots: HashMap<String, Vec<String>> = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT building, facility FROM work_slots ORDER BY building, ord")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|err| err.to_string())?;
    for row in rows {
        let (building, facility) = row.map_err(|err| err.to_string())?;
        slots.entry(building).or_default().push(facility);
    }
    let mut stmt = conn
        .prepare("SELECT code, name_ko, name_gts, cost, credit FROM work_buildings ORDER BY ord")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut works = Vec::new();
    for row in rows {
        let (code, ko, gts, cost, credit) = row.map_err(|err| err.to_string())?;
        let en = online(texts, gts, "en").to_string();
        let zh = online(texts, gts, "zh").to_string();
        let facilities = slots.remove(&code).unwrap_or_default();
        let mut parts = vec![ko, code.clone(), en.clone(), zh.clone()];
        for facility in &facilities {
            if let Some(yard) = yards.get(facility) {
                parts.push(yard.en.clone());
                parts.push(yard.zh.clone());
            }
        }
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        let icon = building_art(client_root, &code);
        works.push(WorkBuilding {
            code,
            en,
            zh,
            cost,
            credit,
            names: haystack(&borrowed),
            facilities,
            icon,
        });
    }
    Ok((works, yards))
}

fn work_art(root: &Path, relative: &str, frame: i32) -> Option<(PathBuf, i32)> {
    let path = root.join(relative);
    path.is_file().then_some((path, frame))
}

fn building_art(root: &Path, code: &str) -> Option<(PathBuf, i32)> {
    let relative = match code {
        "FAR" => "Assets/Textures/UI/Village/Struct/Korea/KoreaFarm.AGF",
        "WEA" => "Assets/Textures/UI/Village/Struct/Korea/Koreaweapon.AGF",
        "RAN" => "Assets/Textures/UI/Village/Struct/Korea/Korearanch.AGF",
        "ARM" => "Assets/Textures/UI/Village/Struct/Korea/Koreaarmor.AGF",
        "FAC" => "Assets/Textures/UI/Village/Struct/Korea/KoreaFactory.AGF",
        _ => return None,
    };
    work_art(root, relative, 0)
}

fn facility_art(root: &Path, code: &str) -> Option<(PathBuf, i32)> {
    // SpriteFont column in the production facility table, drawn from Farm_Building.AGF.
    let frame = match code {
        "FRU" | "SWM" => 0,
        "LND" => 3,
        "MIL" => 6,
        "STG" => 9,
        "SWD" | "SPE" | "BAX" => 10,
        "ARR" | "RPL" | "THK" => 13,
        "SPC" | "CRW" | "WFA" | "BUD" => 16,
        "ARO" => 19,
        "GNB" => 22,
        "HEL" => 25,
        "BUT" => 28,
        "BAN" => 31,
        "ACC" => 34,
        "SMT" | "PIT" => 37,
        "KLN" => 40,
        "PHM" => 43,
        "WEA" => 46,
        _ => return None,
    };
    work_art(root, "Assets/Textures/UI/Village/Back/Farm_Building.AGF", frame)
}

fn load_formulas(conn: &Connection) -> Result<HashMap<i64, ItemFormula>, String> {
    let mut formulas = HashMap::new();
    if !table_exists(conn, "item_formulas") {
        return Ok(formulas);
    }
    let mut stmt = conn
        .prepare("SELECT item_code, country, labor, yield_n, yield_max, IFNULL(plant, '') FROM item_formulas")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    for row in rows {
        let (item, country, labor, yield_n, yield_max, plant) = row.map_err(|err| err.to_string())?;
        formulas.insert(
            item,
            ItemFormula {
                country,
                labor,
                yield_n,
                yield_max,
                plant,
                parts: Vec::new(),
            },
        );
    }
    if table_exists(conn, "item_formula_parts") {
        let mut parts = conn
            .prepare("SELECT item_code, kind, part_code, qty FROM item_formula_parts ORDER BY item_code, slot")
            .map_err(|err| err.to_string())?;
        let rows = parts
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|err| err.to_string())?;
        for row in rows {
            let (item, kind, part, qty) = row.map_err(|err| err.to_string())?;
            if let Some(formula) = formulas.get_mut(&item) {
                formula.parts.push(FormulaPart { kind, item: part, qty });
            }
        }
    }
    Ok(formulas)
}

fn load_work_upgrades(conn: &Connection, texts: &HashMap<String, String>) -> Result<HashMap<String, WorkUpgradeName>, String> {
    if !table_exists(conn, "work_upgrades") {
        return Ok(HashMap::new());
    }
    let mut stmt = conn
        .prepare("SELECT code, name_ko, name_gts, effect_gts FROM work_upgrades")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut upgrades = HashMap::new();
    for row in rows {
        let (code, ko, gts, effect) = row.map_err(|err| err.to_string())?;
        let en = first_name(online(texts, gts, "en"), &ko);
        let zh = first_name(online(texts, gts, "zh"), &ko);
        let effect_en = online(texts, effect, "en");
        let effect_zh = online(texts, effect, "zh");
        upgrades.insert(
            code,
            WorkUpgradeName {
                en: if effect_en.is_empty() { en } else { format!("{en} {effect_en}") },
                zh: if effect_zh.is_empty() { zh } else { format!("{zh} {effect_zh}") },
            },
        );
    }
    Ok(upgrades)
}

fn load_hero_skills(conn: &Connection, texts: &HashMap<String, String>) -> Result<HashMap<String, Vec<HeroSkill>>, String> {
    if !table_exists(conn, "character_skills") {
        return Ok(HashMap::new());
    }
    let mut stmt = conn
        .prepare("SELECT code, name_ko, desc_gts FROM character_skills ORDER BY code, slot")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))
        .map_err(|err| err.to_string())?;
    let mut skills: HashMap<String, Vec<HeroSkill>> = HashMap::new();
    for row in rows {
        let (code, name, gts) = row.map_err(|err| err.to_string())?;
        skills.entry(code).or_default().push(HeroSkill {
            name,
            en: online(texts, gts, "en").to_string(),
            zh: online(texts, gts, "zh").to_string(),
        });
    }
    Ok(skills)
}

fn hero_icon(client_root: &Path, portrait: &str, frame: i64, sprite: &str) -> Option<(PathBuf, i32)> {
    if !portrait.is_empty() && frame >= 0 {
        let path = client_root.join(portrait.replace('\\', "/"));
        if path.is_file() {
            return Some((path, frame as i32));
        }
    }
    if sprite.is_empty() {
        return None;
    }
    let path = client_root.join(sprite.replace('\\', "/"));
    path.is_file().then_some((path, 0))
}

fn load_towns(conn: &Connection, texts: &HashMap<String, String>) -> Result<Vec<TownRow>, String> {
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

fn load_charts(conn: &Connection, texts: &HashMap<String, String>) -> Result<Vec<ChartRow>, String> {
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

fn load_npcs(
    conn: &Connection,
    client_root: &Path,
    texts: &HashMap<String, String>,
) -> Result<Vec<NpcRow>, String> {
    if !table_exists(conn, "npcs") {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT n.code, n.nation, n.name_ko, n.name_gts, n.map_index, n.map_x, n.map_y, n.portrait, n.place_gts,
                    IFNULL(m.name_ko, ''), IFNULL(m.name_gts, 0)
             FROM npcs n
             LEFT JOIN maps m ON m.code = n.map_index
             ORDER BY n.code",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, i64>(10)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut npcs = Vec::new();
    for row in rows {
        let (code, nation, ko, name_gts, _map, x, y, portrait, place_gts, map_ko, map_gts) =
            row.map_err(|err| err.to_string())?;
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        let map_en = online(texts, map_gts, "en").to_string();
        let map_zh = online(texts, map_gts, "zh").to_string();
        let place_en = online(texts, place_gts, "en").to_string();
        let place_zh = online(texts, place_gts, "zh").to_string();
        let (nation_en, nation_zh) = npc_nation(&nation);
        let names = haystack(&[
            &ko,
            &plain_name(&ko),
            &en,
            &zh,
            &map_ko,
            &map_en,
            &map_zh,
            &place_en,
            &place_zh,
            nation_en,
            nation_zh,
        ]);
        npcs.push(NpcRow {
            code,
            ko,
            en,
            zh,
            nation_en: nation_en.to_string(),
            nation_zh: nation_zh.to_string(),
            map_en: if map_en.is_empty() && map_zh.is_empty() { map_ko } else { map_en },
            map_zh,
            map_index: _map,
            x,
            y,
            place_en,
            place_zh,
            names,
            icon: npc_portrait(client_root, portrait),
        });
    }
    npcs.sort_by(|a, b| a.zh.cmp(&b.zh).then(a.en.cmp(&b.en)).then(a.code.cmp(&b.code)));
    Ok(npcs)
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
        return false;
    };
    stmt.query_map([], |row| row.get::<_, String>(1))
        .map(|rows| rows.filter_map(Result::ok).any(|name| name == column))
        .unwrap_or(false)
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .is_ok()
}

fn npc_nation(code: &str) -> (&'static str, &'static str) {
    match code {
        "K" => ("Joseon", "朝鮮"),
        "J" => ("Japan", "日本"),
        "C" => ("China", "中國"),
        "T" => ("Taiwan", "台灣"),
        "I" => ("India", "印度"),
        "A" => ("All", "全區"),
        _ => ("", ""),
    }
}

fn npc_portrait(client_root: &Path, index: i64) -> Option<(PathBuf, i32)> {
    if index <= 0 {
        return None;
    }
    let relative = match index / 300 {
        0 => "Assets/Textures/UI/Portraits/portrait_npc.AGF",
        1 => "Assets/Textures/UI/Portraits/portrait_npc2.AGF",
        _ => return None,
    };
    let path = client_root.join(relative);
    path.is_file().then_some((path, (index % 300) as i32))
}

fn load_online(conn: &Connection) -> Result<HashMap<String, String>, String> {
    let mut stmt = conn
        .prepare("SELECT id, lang, text FROM strings WHERE object = 'OnlineText.dat'")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
        .map_err(|err| err.to_string())?;
    let mut texts = HashMap::new();
    for row in rows {
        let (id, lang, text) = row.map_err(|err| err.to_string())?;
        texts.insert(format!("{lang}:{id}"), text);
    }
    Ok(texts)
}

fn online<'a>(texts: &'a HashMap<String, String>, gts: i64, lang: &str) -> &'a str {
    if gts <= 0 {
        return "";
    }
    texts.get(&format!("{lang}:{gts:08}")).map(String::as_str).unwrap_or("")
}

fn load_locations(conn: &Connection) -> Result<HashMap<String, Vec<i64>>, String> {
    let mut stmt = conn
        .prepare("SELECT monster_code, location_gts FROM monster_locations ORDER BY monster_code, location_gts")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|err| err.to_string())?;
    let mut locations = HashMap::new();
    for row in rows {
        let (code, gts) = row.map_err(|err| err.to_string())?;
        locations.entry(code).or_insert_with(Vec::new).push(gts);
    }
    Ok(locations)
}

fn load_monsters(
    conn: &Connection,
    client_root: &Path,
    pictures: &HashMap<String, (PathBuf, i32)>,
    texts: &HashMap<String, String>,
    locations: &HashMap<String, Vec<i64>>,
) -> Result<Vec<MonsterRow>, String> {
    let mut stmt = conn
        .prepare("SELECT code, nation, name_ko, name_gts, element FROM monsters ORDER BY code")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut monsters = Vec::new();
    for row in rows {
        let (code, nation, ko, name_gts, element) = row.map_err(|err| err.to_string())?;
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        let places = locations.get(&code).cloned().unwrap_or_default();
        let mut place_en = Vec::new();
        let mut place_zh = Vec::new();
        for gts in &places {
            let en_name = online(texts, *gts, "en");
            let zh_name = online(texts, *gts, "zh");
            if !en_name.is_empty() {
                place_en.push(en_name);
            }
            if !zh_name.is_empty() {
                place_zh.push(zh_name);
            }
        }
        let badge = element_file(element)
            .map(|relative| client_root.join(relative))
            .filter(|path| path.is_file());
        monsters.push(MonsterRow {
            names: haystack(&[&ko, &en, &zh]),
            code: code.clone(),
            nation,
            en,
            zh,
            place_en: place_en.join(" | "),
            place_zh: place_zh.join(" | "),
            places,
            icon: pictures.get(&code).cloned(),
            badge,
        });
    }
    Ok(monsters)
}

fn nation_counts(monsters: &[MonsterRow]) -> Vec<(i64, i64)> {
    let mut counts = HashMap::<i64, i64>::new();
    for monster in monsters {
        *counts.entry(monster.nation).or_default() += 1;
    }
    let mut nations: Vec<(i64, i64)> = counts.into_iter().collect();
    nations.sort_by_key(|(nation, _)| *nation);
    nations
}

fn load_places(conn: &Connection, texts: &HashMap<String, String>) -> Result<Vec<PlaceRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT m.nation, ml.location_gts, m.code
             FROM monsters m
             JOIN monster_locations ml ON ml.monster_code = m.code",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)))
        .map_err(|err| err.to_string())?;
    let mut grouped: HashMap<(i64, i64), HashSet<String>> = HashMap::new();
    for row in rows {
        let (nation, gts, code) = row.map_err(|err| err.to_string())?;
        grouped.entry((nation, gts)).or_default().insert(code);
    }
    let mut places = Vec::new();
    for ((nation, gts), monsters) in grouped {
        let en = online(texts, gts, "en").to_string();
        let zh = online(texts, gts, "zh").to_string();
        let sort = if zh.is_empty() { en.clone() } else { zh.clone() };
        places.push(PlaceRow {
            nation,
            gts,
            names: haystack(&[&en, &zh]),
            count: monsters.len() as i64,
            en,
            zh,
            sort,
        });
    }
    Ok(places)
}

fn item_category_pair(key: &str) -> (&'static str, &'static str) {
    match key {
        "set" => ("Sets", "套裝"),
        "W" => ("Weapons", "武器"),
        "E" => ("Materials", "材料"),
        "H" => ("Helmets", "頭盔"),
        "A" => ("Armor", "盔甲"),
        "Y" => ("Orbs", "寶珠"),
        "I" => ("Consumables", "消耗"),
        "IL" => ("Scrolls", "傳送"),
        "II" => ("Boxes", "寶箱"),
        "S" => ("Shoes", "鞋子"),
        "G" => ("Gloves", "手套"),
        "B" => ("Belts", "腰帶"),
        "C" => ("Clothes", "衣服"),
        "R" => ("Rings", "戒指"),
        "J" => ("Bracelets", "手環"),
        "V" => ("Leggings", "綁腿"),
        "F" => ("Food", "食物"),
        "N" => ("Necklaces", "項鍊"),
        "P" => ("Earrings", "耳環"),
        "L" => ("Medicine", "藥品"),
        "T" => ("Tools", "工具"),
        "U" => ("Charms", "護符"),
        "Q" => ("Guardians", "武神"),
        "M" => ("Beasts", "神獸"),
        "O" => ("Animals", "動物"),
        "K" => ("Statues", "石像"),
        _ => ("Other", "其他"),
    }
}

fn item_category_order(key: &str) -> usize {
    [
        "W", "E", "H", "A", "Y", "I", "IL", "II", "S", "G", "B", "C", "R", "J", "V", "F", "N", "P", "L", "T", "U", "Q", "M", "O", "K", "",
    ]
    .iter()
    .position(|item| *item == key)
    .unwrap_or(80)
}

fn item_category_name(key: &str, lang: &str) -> String {
    let (en, zh) = item_category_pair(key);
    chrome_text(lang, zh, en)
}

fn set_kind_rank(kind: i64) -> i64 {
    match kind {
        0 => 0,
        2 => 1,
        _ => 2,
    }
}

fn set_kind_label(kind: i64, lang: &str) -> String {
    match kind {
        1 => phrase("set-charm", lang),
        2 => phrase("set-unique", lang),
        _ => phrase("set", lang),
    }
}

const PIECE_WORDS: &[(&str, &str)] = &[
    ("팔보호구", "G"),
    ("머리띠", "H"),
    ("전투화", "S"),
    ("목걸이", "N"),
    ("귀걸이", "P"),
    ("귀고리", "P"),
    ("가락지", "R"),
    ("팔찌", "J"),
    ("장갑", "G"),
    ("갑옷", "A"),
    ("예복", "A"),
    ("투구", "H"),
    ("요대", "B"),
    ("신발", "S"),
    ("반지", "R"),
    ("지환", "R"),
    ("각반", "V"),
    ("의복", "C"),
    ("복장", "C"),
    ("도복", "C"),
];

fn piece_prefix(full: &str, base: &str, lang: &str) -> String {
    let full = full.trim();
    let base = base.trim();
    let rest = if !base.is_empty() && full.ends_with(base) {
        full[..full.len() - base.len()].trim()
    } else {
        ""
    };
    match rest {
        "" => String::new(),
        "북두칠성" => chrome_text(lang, "北斗七星", "Big Dipper"),
        "천추" => chrome_text(lang, "天樞", "Dubhe"),
        "천선" => chrome_text(lang, "天璇", "Merak"),
        "천기" => chrome_text(lang, "天璣", "Phecda"),
        "천권" => chrome_text(lang, "天權", "Megrez"),
        "옥형" => chrome_text(lang, "玉衡", "Alioth"),
        "개양" => chrome_text(lang, "開陽", "Mizar"),
        "요광" => chrome_text(lang, "搖光", "Alkaid"),
        "홈이있는" => chrome_text(lang, "有孔", "Socketed"),
        other => other.to_string(),
    }
}

fn piece_slot(category: &str, name: &str) -> String {
    if !category.is_empty() {
        return category.to_string();
    }
    PIECE_WORDS
        .iter()
        .find(|(word, _)| name.contains(word))
        .map(|(_, slot)| (*slot).to_string())
        .unwrap_or_else(|| "other".into())
}

fn set_slot_order(slot: &str) -> usize {
    ["W", "H", "A", "G", "B", "S", "R", "N", "P", "J", "V", "C", "other"]
        .iter()
        .position(|item| *item == slot)
        .unwrap_or(90)
}

fn set_slot_pair(slot: &str) -> Option<(&'static str, &'static str)> {
    Some(match slot {
        "W" => ("Weapon", "武器"),
        "H" => ("Head", "頭"),
        "A" => ("Armor", "甲"),
        "G" => ("Hands", "手"),
        "B" => ("Waist", "腰"),
        "S" => ("Shoes", "鞋"),
        "R" => ("Ring ×2", "戒×2"),
        "N" => ("Necklace", "項鍊"),
        "P" => ("Earring", "耳環"),
        "J" => ("Bracelet", "手環"),
        "V" => ("Leggings", "綁腿"),
        "C" => ("Clothes", "衣服"),
        "other" => ("Other", "其他"),
        _ => return None,
    })
}

fn set_slot_label(slot: &str, lang: &str) -> String {
    set_slot_pair(slot).map(|(en, zh)| chrome_text(lang, zh, en)).unwrap_or_else(|| slot.to_string())
}

fn set_slot_heading(slot: &str, lang: &str) -> Option<String> {
    set_slot_pair(slot).map(|(en, zh)| chrome_text(lang, zh, en))
}

fn set_piece_label(pieces: i64, lang: &str) -> String {
    if pieces <= 0 {
        return String::new();
    }
    if chrome(lang) == "en" {
        format!("{pieces} pc")
    } else {
        format!("{pieces}件")
    }
}

fn set_title_lang(lang: &str, ko: &str, en: &str, zh: &str) -> String {
    let shown = lead(lang, en, zh);
    if shown.is_empty() { ko.trim().to_string() } else { shown }
}

fn set_stat_name(stat: &str, lang: &str) -> Option<String> {
    let (zh, en) = match stat {
        "S" => ("力量", "STR"),
        "D" => ("敏捷", "AGI"),
        "V" => ("體質", "VIT"),
        "I" => ("智力", "INT"),
        "P" => ("打擊抗性", "Hit resist"),
        "M" => ("魔法抗性", "Magic resist"),
        "W" => ("水屬性", "Water"),
        "A" => ("風屬性", "Wind"),
        "F" => ("火屬性", "Fire"),
        "T" => ("雷屬性", "Thunder"),
        "E" => ("土屬性", "Earth"),
        "R" => ("移動速度", "Move"),
        "C" => ("暴擊", "Crit"),
        _ => return None,
    };
    Some(chrome_text(lang, zh, en))
}

fn set_stat_line(stat: &str, value: i64, lang: &str) -> Option<String> {
    if value == 0 {
        return None;
    }
    Some(format!("{} +{value}", set_stat_name(stat, lang)?))
}

#[derive(Default)]
struct StatBag {
    order: Vec<String>,
    values: HashMap<String, i64>,
    dmin: i64,
    dmax: i64,
    notes: Vec<String>,
    damage_pct: i64,
    element: i64,
    earth: i64,
}

impl StatBag {
    fn add(&mut self, label: &str, value: i64) {
        if value == 0 || label.is_empty() {
            return;
        }
        if !self.values.contains_key(label) {
            self.order.push(label.to_string());
        }
        *self.values.entry(label.to_string()).or_default() += value;
    }

    fn add_gear(&mut self, values: &[i64; 10], lang: &str) {
        self.add(&chrome_text(lang, "力量", "STR"), values[0]);
        self.add(&chrome_text(lang, "敏捷", "AGI"), values[1]);
        self.add(&chrome_text(lang, "體質", "VIT"), values[2]);
        self.add(&chrome_text(lang, "智力", "INT"), values[3]);
        self.add(&chrome_text(lang, "生命上限", "Max HP"), values[4]);
        self.add(&chrome_text(lang, "魔力上限", "Max MP"), values[5]);
        self.add(&chrome_text(lang, "攻擊", "Attack"), values[6]);
        self.add(&chrome_text(lang, "防禦", "Defense"), values[7]);
        self.add(&chrome_text(lang, "打擊抗性", "Hit resist"), values[8]);
        self.add(&chrome_text(lang, "魔法抗性", "Magic resist"), values[9]);
    }

    fn add_affix(&mut self, values: &[i64; 10], lang: &str) {
        self.add(&chrome_text(lang, "力量", "STR"), values[0]);
        self.add(&chrome_text(lang, "敏捷", "AGI"), values[1]);
        self.add(&chrome_text(lang, "體質", "VIT"), values[2]);
        self.add(&chrome_text(lang, "智力", "INT"), values[3]);
        self.add_element(values[4], values[5]);
        self.add_damage_pct(values[6]);
        self.add(&phrase("dmg-cut", lang), values[7]);
        self.add(&phrase("crit", lang), values[8]);
        self.add(&phrase("crit-dmg", lang), values[9]);
    }

    fn absorb(&mut self, other: &StatBag, copies: i64) {
        if other.dmin != 0 || other.dmax != 0 {
            self.dmin += other.dmin * copies;
            self.dmax += other.dmax * copies;
        }
        self.add_damage_pct(other.damage_pct * copies);
        self.element += other.element * copies;
        self.earth += other.earth * copies;
        for label in &other.order {
            self.add(label, other.values[label] * copies);
        }
        for note in &other.notes {
            if !self.notes.iter().any(|have| have == note) {
                self.notes.push(note.clone());
            }
        }
    }

    fn add_damage_pct(&mut self, value: i64) {
        self.damage_pct += value;
    }

    fn add_element(&mut self, element: i64, land: i64) {
        if element == 0 && land == 0 {
            return;
        }
        self.element += element;
        self.earth += if land == 0 { element } else { element + land };
    }

    fn note(&mut self, text: String) {
        if !text.is_empty() && !self.notes.iter().any(|have| have == &text) {
            self.notes.push(text);
        }
    }

    fn is_empty(&self) -> bool {
        self.dmin == 0
            && self.dmax == 0
            && self.damage_pct == 0
            && self.element == 0
            && self.earth == 0
            && self.values.values().all(|value| *value == 0)
            && self.notes.is_empty()
    }

    fn rows(&self, lang: &str) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        if self.dmin != 0 || self.dmax != 0 {
            let damage = if self.dmin == self.dmax {
                commas(self.dmin)
            } else {
                format!("{}-{}", commas(self.dmin), commas(self.dmax))
            };
            rows.push((phrase("damage", lang), damage));
        }
        if self.damage_pct != 0 {
            rows.push((phrase("damage", lang), format!("+{}%", self.damage_pct)));
        }
        if self.element != 0 {
            rows.push((phrase("ela", lang), format!("+{}", self.element)));
        }
        if self.earth != 0 && self.earth != self.element {
            rows.push((chrome_text(lang, "土", "Earth"), format!("+{}", self.earth)));
        }
        for label in &self.order {
            let value = self.values[label];
            if value == 0 {
                continue;
            }
            let shown = if value > 0 { format!("+{value}") } else { value.to_string() };
            rows.push((label.clone(), shown));
        }
        for note in &self.notes {
            rows.push((note.clone(), String::new()));
        }
        rows
    }

    fn text(&self, lang: &str) -> String {
        let mut parts = Vec::new();
        if self.dmin != 0 || self.dmax != 0 {
            let damage = if self.dmin == self.dmax {
                commas(self.dmin)
            } else {
                format!("{}-{}", commas(self.dmin), commas(self.dmax))
            };
            parts.push(format!("{} {damage}", phrase("damage", lang)));
        }
        if self.damage_pct != 0 {
            parts.push(format!("{} +{}%", phrase("damage", lang), self.damage_pct));
        }
        if self.element != 0 || self.earth != 0 {
            let earth = chrome_text(lang, "土", "Earth");
            if self.earth != 0 && self.earth != self.element {
                parts.push(format!("{} +{}（{earth} +{}）", phrase("ela", lang), self.element, self.earth));
            } else {
                parts.push(format!("{} +{}", phrase("ela", lang), self.element));
            }
        }
        for label in &self.order {
            let value = self.values[label];
            if value == 0 {
                continue;
            }
            if value > 0 {
                parts.push(format!("{label} +{value}"));
            } else {
                parts.push(format!("{label} {value}"));
            }
        }
        parts.extend(self.notes.iter().cloned());
        parts.join("、")
    }
}

fn damage_percent_magic(code: i64) -> bool {
    matches!(code, 10 | 14 | 15 | 23 | 28)
}

fn set_magic_line(code: i64, value1: i64, value2: i64, percent: bool, lang: &str) -> String {
    let mark = if percent { "%" } else { "" };
    let (zh, en) = match code {
        1 => (format!("攻擊力 +{value1}{mark}"), format!("Attack +{value1}{mark}")),
        2 => (format!("傷害反射 {value1}"), format!("Reflect {value1}")),
        3 => (format!("體力吸收 {value1}"), format!("Absorb HP {value1}")),
        4 => (format!("魔力消耗 -{value1}%"), format!("Mana cost -{value1}%")),
        5 => (format!("魔力吸收 {value1}%"), format!("Absorb mana {value1}%")),
        6 => ("免疫".into(), "Immunity".into()),
        7 | 16 | 21 | 30 => (
            format!("屬性 +{value1}（土 +{value2}）"),
            format!("Element +{value1} (earth +{value2})"),
        ),
        8 => (format!("對空傷害 +{value1}%"), format!("Air damage +{value1}%")),
        9 => (format!("對地傷害 +{value1}%"), format!("Ground damage +{value1}%")),
        10 | 14 | 15 | 23 | 28 => (format!("傷害 +{value1}%"), format!("Damage +{value1}%")),
        11 => (format!("每 2 秒回復體力 {value1}"), format!("HP regen {value1} / 2s")),
        12 => (format!("每 2 秒回復魔力 {value1}"), format!("Mana regen {value1} / 2s")),
        13 => (format!("每 3 秒回復 {value1}"), format!("Regen {value1} / 3s")),
        17 => ("雷電彈洗禮".into(), "Lightning barrage".into()),
        18 => ("冰花爆".into(), "Ice burst".into()),
        19 => ("黃牛突進".into(), "Bull rush".into()),
        20 => ("赤狼劍舞".into(), "Sword dance".into()),
        22 => ("宰相的加護".into(), "Bajirao's blessing".into()),
        24 => ("天子銃改良".into(), "Improved cannon".into()),
        25 => ("雷鳥突擊".into(), "Thunderbird charge".into()),
        26 => ("激流陣".into(), "Torrent".into()),
        27 => ("一閃".into(), "Quick draw".into()),
        29 => ("大帝的加護".into(), "Akbar's blessing".into()),
        31 => (format!("疾風 {value1}"), format!("Gale {value1}")),
        _ => return String::new(),
    };
    chrome_text(lang, &zh, &en)
}

fn weapon_family_pair(key: &str) -> (&'static str, &'static str) {
    match key {
        "axe" => ("Axe", "斧"),
        "spear" => ("Spear", "槍"),
        "bow" => ("Bow", "弓"),
        "sling" => ("Sling", "彈弓"),
        "cannon" => ("Cannon", "銃砲"),
        "gun" => ("Gun", "鐵炮"),
        "dart" => ("Dart", "鏢"),
        "fan" => ("Fan", "扇"),
        "needle" => ("Needle", "針"),
        "claw" => ("Claw", "爪"),
        "bead" => ("Beads", "念珠"),
        "mirror" => ("Mirror", "鏡"),
        "charm" => ("Charm", "符"),
        "blade" => ("Blade", "刀"),
        "sword" => ("Sword", "劍"),
        "doll" => ("Doll", "娃娃"),
        "bell" => ("Bell", "鈴"),
        "lute" => ("Lute", "月琴"),
        "tablet" => ("Tablet", "笏"),
        _ => ("Other", "其他"),
    }
}

fn weapon_family_name(key: &str, lang: &str) -> String {
    let (en, zh) = weapon_family_pair(key);
    chrome_text(lang, zh, en)
}

fn load_items(conn: &Connection, texts: &HashMap<String, String>) -> Result<Vec<ItemRow>, String> {
    let has_column = |name: &str| {
        conn.query_row(
            "SELECT 1 FROM pragma_table_info('items') WHERE name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .is_ok()
    };
    let sql = if has_column("subtype") {
        "SELECT code, name_ko, name_gts, IFNULL(category, ''), IFNULL(subtype, '') FROM items ORDER BY code"
    } else if has_column("category") {
        "SELECT code, name_ko, name_gts, IFNULL(category, ''), '' FROM items ORDER BY code"
    } else {
        "SELECT code, name_ko, name_gts, '', '' FROM items ORDER BY code"
    };
    let mut stmt = conn.prepare(sql).map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut items = Vec::new();
    for row in rows {
        let (code, ko, name_gts, category, subtype) = row.map_err(|err| err.to_string())?;
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        items.push(ItemRow {
            names: haystack(&[&ko, &en, &zh]),
            code_str: code.to_string(),
            en,
            zh,
            category,
            subtype,
        });
    }
    Ok(items)
}

fn load_sets(
    conn: &Connection,
    texts: &HashMap<String, String>,
    icons: &HashMap<i64, (PathBuf, i32)>,
) -> Result<(Vec<SetRow>, HashMap<i64, SetMagic>), String> {
    let ready = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'item_sets'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .is_ok();
    if !ready {
        return Ok((Vec::new(), HashMap::new()));
    }
    let mut icons_for: HashMap<i64, (PathBuf, i32)> = HashMap::new();
    let mut member_stmt = conn
        .prepare("SELECT set_code, item_code FROM item_set_members ORDER BY enhance, item_code")
        .map_err(|err| err.to_string())?;
    let member_rows = member_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|err| err.to_string())?;
    let mut counted: HashMap<i64, i64> = HashMap::new();
    for row in member_rows {
        let (set_code, item_code) = row.map_err(|err| err.to_string())?;
        *counted.entry(set_code).or_default() += 1;
        if icons_for.contains_key(&set_code) {
            continue;
        }
        if let Some(icon) = icons.get(&item_code) {
            icons_for.insert(set_code, icon.clone());
        }
    }
    let mut stmt = conn
        .prepare("SELECT code, kind, name_ko, name_gts, pieces FROM item_sets")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut sets = Vec::new();
    for row in rows {
        let (code, kind, ko, name_gts, pieces) = row.map_err(|err| err.to_string())?;
        if counted.get(&code).copied().unwrap_or(0) == 0 {
            continue;
        }
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        sets.push(SetRow {
            code,
            kind,
            pieces,
            names: haystack(&[&ko, &en, &zh, &code.to_string()]),
            ko,
            en,
            zh,
            icon: icons_for.remove(&code),
        });
    }
    let mut magic_stmt = conn
        .prepare("SELECT code, value1, value2, percent, note FROM item_set_magic")
        .map_err(|err| err.to_string())?;
    let magic_rows = magic_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut magic = HashMap::new();
    for row in magic_rows {
        let (code, value1, value2, percent, note) = row.map_err(|err| err.to_string())?;
        magic.insert(
            code,
            SetMagic {
                value1,
                value2,
                percent: percent != 0,
                note,
            },
        );
    }
    Ok((sets, magic))
}

fn load_item_icons(conn: &Connection, client_root: &Path) -> Result<HashMap<i64, (PathBuf, i32)>, String> {
    let folder = client_root.join("Assets").join("Textures").join("Items");
    let mut files = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            files.insert(entry.file_name().to_string_lossy().to_lowercase(), entry.path());
        }
    }
    let mut stmt = conn
        .prepare("SELECT code, large_image, large_frame FROM items WHERE large_image != '' AND large_frame >= 0")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))
        .map_err(|err| err.to_string())?;
    let mut icons = HashMap::new();
    for row in rows {
        let (code, image, frame) = row.map_err(|err| err.to_string())?;
        let Some(file_name) = Path::new(&image).file_name() else { continue };
        let Some(path) = files.get(&file_name.to_string_lossy().to_lowercase()) else { continue };
        icons.insert(code, (path.clone(), frame as i32));
    }
    borrow_base_icons(conn, &mut icons);
    Ok(icons)
}

fn borrow_base_icons(conn: &Connection, icons: &mut HashMap<i64, (PathBuf, i32)>) {
    let ready = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'item_bases'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .is_ok();
    if !ready {
        return;
    }
    let Ok(mut stmt) = conn.prepare("SELECT item_code, base_code FROM item_bases") else {
        return;
    };
    let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))) else {
        return;
    };
    let pairs: Vec<(i64, i64)> = rows.filter_map(Result::ok).collect();
    for _ in 0..4 {
        let mut added = false;
        for (item, base) in &pairs {
            if icons.contains_key(item) {
                continue;
            }
            if let Some(icon) = icons.get(base).cloned() {
                icons.insert(*item, icon);
                added = true;
            }
        }
        if !added {
            break;
        }
    }
}

fn load_recipes(
    conn: &Connection,
    texts: &HashMap<String, String>,
    icons: &HashMap<i64, (PathBuf, i32)>,
) -> Result<Vec<RecipeRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT r.id, r.source, r.result_item, i.name_ko, i.name_gts, r.level_min
             FROM recipes r
             JOIN items i ON i.code = r.result_item
             ORDER BY r.id",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut recipes = Vec::new();
    for row in rows {
        let (id, source, result, ko, name_gts, level_min) = row.map_err(|err| err.to_string())?;
        let en = online(texts, name_gts, "en").to_string();
        let zh = online(texts, name_gts, "zh").to_string();
        recipes.push(RecipeRow {
            names: haystack(&[&ko, &en, &zh]),
            id,
            source,
            result_str: result.to_string(),
            en,
            zh,
            level_min,
            icon: icons.get(&result).cloned(),
        });
    }
    Ok(recipes)
}

fn quest_source_rank(source: &str) -> usize {
    ["main", "letter", "intro", "mercenary", "dispatch"]
        .iter()
        .position(|key| *key == source)
        .unwrap_or(9)
}

fn main_type_name(kind: &str, lang: &str) -> Option<String> {
    let key = match kind {
        "0" => "starter",
        "2" => "combat",
        "5" => "repeat-type",
        "7" => "timed",
        _ => return None,
    };
    Some(phrase(key, lang))
}

fn load_quests(conn: &Connection) -> Result<Vec<QuestRow>, String> {
    let lines = load_quest_lines(conn)?;
    let npc_column = column_exists(conn, "quests", "npc_code");
    let sql = if npc_column {
        "SELECT id, source, code, title, detail, kind, npc_code FROM quests ORDER BY id"
    } else {
        "SELECT id, source, code, title, detail, kind, 0 FROM quests ORDER BY id"
    };
    let mut stmt = conn.prepare(sql).map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut quests = Vec::new();
    for row in rows {
        let (id, source, code, title, detail, kind, npc) = row.map_err(|err| err.to_string())?;
        let empty = Vec::new();
        let quest_lines = lines.get(&id).unwrap_or(&empty);
        let mut parts = vec![title.clone(), detail.clone()];
        for line in quest_lines {
            for text in line.texts.values() {
                parts.push(text.clone());
            }
        }
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        let names = haystack(&borrowed);
        quests.push(QuestRow {
            title_en: quest_heading("en", &code, quest_lines),
            title_zh: quest_heading("zh", &code, quest_lines),
            id,
            source,
            code,
            kind,
            npc,
            names,
        });
    }
    Ok(quests)
}

fn load_quest_lines(conn: &Connection) -> Result<HashMap<i64, Vec<QuestLine>>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT ql.quest_id, ql.role, ql.slot, IFNULL(s.lang, ''), IFNULL(s.text, '')
             FROM quest_lines ql
             LEFT JOIN strings s ON s.object = ql.text_object AND s.id = printf('%08d', ql.gts_id)
             ORDER BY ql.quest_id, ql.slot",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut lines: HashMap<i64, Vec<QuestLine>> = HashMap::new();
    for row in rows {
        let (quest_id, role, slot, lang, text) = row.map_err(|err| err.to_string())?;
        if lang.is_empty() || text.is_empty() {
            continue;
        }
        let quest = lines.entry(quest_id).or_default();
        if let Some(line) = quest.iter_mut().find(|line| line.slot == slot && line.role == role) {
            line.texts.insert(lang, game_breaks(text));
        } else {
            let mut texts = HashMap::new();
            texts.insert(lang, game_breaks(text));
            quest.push(QuestLine { role, slot, texts });
        }
    }
    Ok(lines)
}

fn quest_heading(lang: &str, code: &str, lines: &[QuestLine]) -> String {
    if let Some(text) = quest_line(lines, QUEST_TITLE_ROLES, lang) {
        return text;
    }
    let other = if lang == "en" { "zh" } else { "en" };
    quest_line(lines, QUEST_TITLE_ROLES, other).unwrap_or_else(|| code.to_string())
}

fn quest_line(lines: &[QuestLine], roles: &[&str], lang: &str) -> Option<String> {
    for role in roles {
        let Some(line) = lines.iter().find(|line| line.role == *role) else {
            continue;
        };
        if let Some(text) = line.texts.get(lang).filter(|text| !text.is_empty()) {
            return Some(text.clone());
        }
    }
    None
}

fn locate_client(stored: PathBuf, data: &Path) -> PathBuf {
    let bundled = data.join("client");
    if pictures_ready(&bundled) {
        return bundled;
    }
    if client_ready(&stored) {
        return stored;
    }
    client_hints(data)
        .into_iter()
        .find(|path| client_ready(path))
        .unwrap_or(bundled)
}

fn client_hints(data: &Path) -> Vec<PathBuf> {
    let mut hints = Vec::new();
    if let Ok(path) = std::env::var("GERSANG_CLIENT") {
        let path = PathBuf::from(path.trim());
        if !path.as_os_str().is_empty() {
            hints.push(path);
        }
    }
    if let Ok(text) = std::fs::read_to_string(data.join("client.txt")) {
        let path = PathBuf::from(text.trim());
        if !path.as_os_str().is_empty() {
            hints.push(path);
        }
    }
    for letter in local_drive_letters() {
        hints.push(PathBuf::from(format!(r"{letter}:\AKInteractive\static")));
    }
    hints
}

fn client_ready(root: &Path) -> bool {
    root.join("gersang.gcs").is_file() || pictures_ready(root)
}

fn pictures_ready(root: &Path) -> bool {
    root.join("Assets")
        .join("Textures")
        .join("UI")
        .join("Portraits")
        .join("PORTRAIT_INVEN.AGF")
        .is_file()
}

fn local_drive_letters() -> Vec<char> {
    #[cfg(windows)]
    {
        extern "system" {
            fn GetLogicalDrives() -> u32;
            fn GetDriveTypeW(root: *const u16) -> u32;
        }
        const DRIVE_FIXED: u32 = 3;
        let mask = unsafe { GetLogicalDrives() };
        let mut letters = Vec::new();
        for index in 0..26 {
            if mask & (1 << index) == 0 {
                continue;
            }
            let letter = b'A' + index;
            let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
            if unsafe { GetDriveTypeW(root.as_ptr()) } == DRIVE_FIXED {
                letters.push(letter as char);
            }
        }
        if !letters.is_empty() {
            return letters;
        }
    }
    vec!['C', 'D', 'E']
}

fn find_db() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("GERSANG_DB") {
        let path = PathBuf::from(path.trim());
        if path.is_file() {
            return Ok(path);
        }
    }
    let mut starts = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            starts.push(dir.to_path_buf());
        }
    }
    for start in starts {
        let mut dir = start;
        for _ in 0..8 {
            let nested = dir.join("data").join("gersang.db");
            if nested.is_file() {
                return Ok(nested);
            }
            let direct = dir.join("gersang.db");
            if direct.is_file() {
                return Ok(direct);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    Err("找不到 data/gersang.db。下載的專案裡應該已經有這份圖鑑。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn display_modes_keep_chinese_and_english() {
        assert_eq!(lead("zh-en", "Fire", "火"), "火");
        assert_eq!(trail("zh-en", "Fire", "火"), "Fire");
        assert_eq!(show("zh-en", "Fire", "火"), "火 · Fire");
        assert_eq!(lead("en-zh", "Fire", "火"), "Fire");
        assert_eq!(trail("en-zh", "Fire", "火"), "火");
        assert_eq!(show("en-zh", "Fire", "火"), "Fire · 火");
        assert_eq!(show("zh-en", "Same", "Same"), "Same");
        assert_eq!(show("en-zh", "", "火"), "火");
        assert_eq!(stack("en-zh", "Fire", "火"), "Fire\n火");
        assert_eq!(phrase("hp", "zh-en"), "體力");
        assert_eq!(phrase("hp", "en-zh"), "HP");
        assert_eq!(phrase("ela", "zh-en"), "屬性值");
        assert_eq!(phrase("ela", "en-zh"), "ELA");
    }

    #[test]
    fn monster_related_lists_kill_and_gather_quests() {
        let catalog = Catalog::open().expect("db");
        let deer = catalog.monster_quests("M24", "en-zh");
        assert!(
            deer.iter().any(|hit| hit.subtitle.contains("30201")),
            "deer should list the antler quest"
        );
        assert!(deer.iter().all(|hit| hit.key.starts_with("quest:")));
        let wolf = catalog.monster_quests("M25", "en-zh");
        assert!(
            wolf.iter().any(|hit| hit.subtitle.contains("50002") && hit.aside.contains('×')),
            "wolf should list the kill quest"
        );
    }

    #[test]
    fn monster_sheet_shows_attribute_value() {
        let catalog = Catalog::open().expect("db");
        let zh = catalog.monster_sheet("K63", "zh-en").expect("K63");
        assert!(zh.3.iter().any(|(label, value)| label == "屬性值" && value == "90"));
        let en = catalog.monster_sheet("K63", "en-zh").expect("K63");
        assert!(en.3.iter().any(|(label, value)| label == "ELA" && value == "90"));
    }

    #[test]
    fn search_returns_before_a_frame() {
        let catalog = Catalog::open().expect("db");
        let started = Instant::now();
        for _ in 0..40 {
            let _ = catalog.search("item", "zzzz", "zh", "", "");
            let _ = catalog.search("quest", "龍", "zh", "", "");
            let _ = catalog.search("recipe", "a", "en", "", "");
            let _ = catalog.search("monster", "a", "zh", "", "");
            let _ = catalog.search("item", "", "ko", "", "");
        }
        let elapsed = started.elapsed();
        eprintln!("200 searches in {elapsed:?}");
        assert!(elapsed.as_millis() < 400, "searches too slow: {elapsed:?}");

        let items = catalog.search("item", "", "zh", "", "");
        assert_eq!(items.len(), 27);
        assert_eq!(items[0].key, "icat:set");
        assert_eq!(items[0].title, "套裝");
        assert!(catalog.search("item", "zzzz-not-a-real-item", "zh", "", "").is_empty());
        let stones = catalog.search("item", "SSA", "zh", "", "");
        for code in ["6213", "6214", "6215", "6216", "6943"] {
            assert!(stones.iter().any(|hit| hit.key == format!("item:{code}")), "{code}");
        }
        assert!(catalog.search("item", "古土", "zh", "", "").iter().any(|hit| hit.key == "item:8900"));
        assert!(catalog.search("item", "EOS", "en", "", "").iter().any(|hit| hit.key == "item:11656"));
        assert!(catalog.search("item", "星之精随", "zh", "", "").iter().any(|hit| hit.key == "item:11656"));
        assert!(catalog.search("item", "essense of stars", "en", "", "").iter().any(|hit| hit.key == "item:11656"));

        let nations = catalog.search("monster", "", "zh", "", "");
        assert!(nations.iter().any(|hit| hit.key.starts_with("nation:")));
        let nation = nations[0].key.trim_start_matches("nation:");
        let places = catalog.search("monster", "", "en", "", nation);
        assert!(places.iter().all(|hit| hit.key.starts_with("region:") || hit.key.starts_with("area:")));

        assert_eq!(
            keys(&catalog.search("item", "龍", "zh", "", "")),
            item_keys(&catalog, "龍")
        );
        assert_eq!(
            keys(&catalog.search("item", "zzzz", "en", "", "")),
            item_keys(&catalog, "zzzz")
        );
        assert_eq!(
            keys(&catalog.search("recipe", "a", "ko", "", "")),
            recipe_keys(&catalog, "a")
        );
        let crafts = catalog.search("recipe", "", "zh", "", "");
        assert!(crafts.iter().all(|hit| hit.key.starts_with("craft:")));
        assert!(crafts.iter().any(|hit| hit.key == "craft:alchemy"));
        let alchemy = catalog.search("recipe", "", "zh", "alchemy", "");
        assert!(!alchemy.is_empty());
        assert!(alchemy.iter().all(|hit| hit.key.starts_with("recipe:")));
        assert_eq!(
            keys(&catalog.search("monster", "a", "en", "", "")),
            monster_keys(&catalog, "a", "")
        );
        let quests = catalog.search("quest", "龍", "zh", "", "");
        let quest_ids = quest_ids(&catalog, "龍");
        assert_eq!(keys(&quests), quest_ids.iter().map(|id| format!("quest:{id}")).collect::<Vec<_>>());
        for (hit, id) in quests.iter().zip(quest_ids) {
            let (title, code): (String, String) = catalog
                .conn
                .query_row("SELECT title, code FROM quests WHERE id = ?1", [id], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .unwrap();
            let shown = catalog.quest_title(id, &title, &code, "zh");
            assert_eq!(hit.title, shown.lines().next().unwrap_or(""));
        }
    }

    fn keys(hits: &[Hit]) -> Vec<String> {
        hits.iter().map(|hit| hit.key.clone()).collect()
    }

fn item_keys(catalog: &Catalog, query: &str) -> Vec<String> {
    let like = format!("%{query}%");
    let mut stmt = catalog
        .conn
        .prepare(
            "SELECT i.code
             FROM items i
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE ?1 = ''
                OR CAST(i.code AS TEXT) = ?1
                OR i.name_ko LIKE ?2
                OR se.text LIKE ?2
                OR sz.text LIKE ?2
             ORDER BY i.code
             LIMIT 40",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![query, like], |row| row.get::<_, i64>(0))
        .unwrap()
        .filter_map(Result::ok)
        .map(|code| format!("item:{code}"))
        .collect()
}

fn recipe_keys(catalog: &Catalog, query: &str) -> Vec<String> {
    let like = format!("%{query}%");
    let mut stmt = catalog
        .conn
        .prepare(
            "SELECT r.id
             FROM recipes r
             JOIN items i ON i.code = r.result_item
             LEFT JOIN strings se ON se.object = 'OnlineText.dat' AND se.lang = 'en'
                AND i.name_gts > 0 AND se.id = printf('%08d', i.name_gts)
             LEFT JOIN strings sz ON sz.object = 'OnlineText.dat' AND sz.lang = 'zh'
                AND i.name_gts > 0 AND sz.id = printf('%08d', i.name_gts)
             WHERE ?1 = ''
                OR CAST(r.result_item AS TEXT) = ?1
                OR i.name_ko LIKE ?2
                OR se.text LIKE ?2
                OR sz.text LIKE ?2
             ORDER BY r.id
             LIMIT 40",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![query, like], |row| row.get::<_, i64>(0))
        .unwrap()
        .filter_map(Result::ok)
        .map(|id| format!("recipe:{id}"))
        .collect()
}

fn monster_keys(catalog: &Catalog, query: &str, place: &str) -> Vec<String> {
    let like = format!("%{query}%");
    let mut stmt = catalog
        .conn
        .prepare(
            "SELECT code
             FROM v_monster
             WHERE (?3 = '' OR EXISTS (
                    SELECT 1 FROM monster_locations ml
                    WHERE ml.monster_code = v_monster.code AND ml.location_gts = ?3
                ))
               AND (?1 = ''
                OR code = ?1 COLLATE NOCASE
                OR name_ko LIKE ?2
                OR IFNULL(name_en, '') LIKE ?2
                OR IFNULL(name_zh, '') LIKE ?2)
             ORDER BY code
             LIMIT 80",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![query, like, place], |row| row.get::<_, String>(0))
        .unwrap()
        .filter_map(Result::ok)
        .map(|code| format!("monster:{code}"))
        .collect()
}

fn quest_ids(catalog: &Catalog, query: &str) -> Vec<i64> {
    let like = format!("%{query}%");
    let mut stmt = catalog
        .conn
        .prepare(
            "SELECT id
             FROM quests
             WHERE ?1 = '' OR code = ?1 OR title LIKE ?2 OR detail LIKE ?2
                OR EXISTS (
                    SELECT 1 FROM quest_lines ql
                    JOIN strings s ON s.object = ql.text_object AND s.id = printf('%08d', ql.gts_id)
                    WHERE ql.quest_id = quests.id AND s.text LIKE ?2
                )
             ORDER BY id
             LIMIT 40",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![query, like], |row| row.get(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect()
    }

    #[test]
    fn king_nicknames_find_the_set_and_the_slot() {
        assert_eq!(set_query("龜天").map(|(_, _, slot)| slot), Some(None));
        assert_eq!(set_query("龜天手").map(|(_, ko, slot)| (ko, slot)), Some(("다문천왕", Some("G"))));
        assert!(set_query("龍").is_none());
        let catalog = Catalog::open().expect("db");
        let turtle = catalog.search("item", "龜天", "zh", "", "");
        for code in ["6097", "6098", "6099", "6100", "6101", "6525"] {
            assert!(turtle.iter().any(|hit| hit.key == format!("item:{code}")), "{code}");
        }
        assert!(turtle.len() > 40, "set nickname should list every matching item, got {}", turtle.len());
        let glove = catalog.search("item", "龜天手", "zh", "", "");
        assert_eq!(glove.first().map(|hit| hit.key.as_str()), Some("item:6099"));
        assert!(glove.iter().all(|hit| hit.key != "item:6097"));
        assert_eq!(catalog.search("item", "雀天手", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6240"));
        assert_eq!(catalog.search("item", "龍天甲", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6109"));
        assert_eq!(catalog.search("item", "虎天頭", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6243"));
        assert_eq!(catalog.search("item", "龜天戒", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6525"));
        assert_eq!(catalog.search("item", "雀天腰", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6241"));
        assert_eq!(catalog.search("item", "虎天鞋", "zh", "", "").first().map(|hit| hit.key.as_str()), Some("item:6247"));
    }

    #[test]
    fn nicknames_shorten_seals_and_english_names() {
        assert!(nicknames("古代地之印章(土)").iter().any(|alias| alias == "古土"));
        assert!(nicknames("Essence of Stars").iter().any(|alias| alias == "eos"));
        assert!(nicknames("Ancient Earth Seal").iter().any(|alias| alias == "aes"));
        let book = parse_alias_book("SSA 6213, 6214 6215 6216 6943\n8900 古土印");
        assert!(book.get("ssa").is_some_and(|codes| codes.contains("6213") && codes.contains("6943")));
        assert!(book.get("古土印").is_some_and(|codes| codes.contains("8900")));
        let rows = alias_rows_from("SSA 6213 6214 6215 6216 6943");
        assert_eq!(rows, vec![("SSA".into(), "6213 6214 6215 6216 6943".into())]);
        let seal = haystack(&["고대 땅의 인장(土)", "Ancient Earth Seal", "古代地之印章(土)"]);
        assert!(seal.split('\n').any(|line| line == "古土"));
        assert!(seal.split('\n').any(|line| line == "aes"));
        let star = haystack(&["별의정수", "Essence of Stars", "星之精髓"]);
        assert!(star.split('\n').any(|line| line == "eos"));
        assert!(star.split('\n').any(|line| line == "星精"));
        assert_eq!(fuzzy_budget("eo"), 0);
        assert!(fuzzy_dist("essense", "essence of stars", 2).is_some());
        assert!(fuzzy_dist("essense", "essence of stars", 1).is_some());
        assert!(fuzzy_dist("星之精随", "星之精髓", 1).is_some());
        assert!(fuzzy_dist("zzzz", "essence of stars", 1).is_none());
    }

    #[test]
    fn place_split_keeps_floor_suffix() {
        assert_eq!(place_split("冰城 - 1層"), ("冰城".into(), "1層".into()));
        assert_eq!(place_split("帝王的陵寢 -  3層"), ("帝王的陵寢".into(), "3層".into()));
        assert_eq!(place_split("激戰香巴拉2樓"), ("激戰香巴拉".into(), "2樓".into()));
        assert_eq!(place_split("鬼曲城，神仙谷"), ("鬼曲城，神仙谷".into(), String::new()));
        assert_eq!(place_split("Ice Castle - 2nd floor"), ("Ice Castle".into(), "2nd floor".into()));
    }

    #[test]
    fn similar_places_merge_and_sort_floors() {
        let catalog = Catalog::open().expect("db");
        let places = catalog.search("monster", "", "zh", "", "2");
        let ice = places.iter().find(|hit| hit.key == "area:冰城").expect("merged ice castle");
        assert_eq!(ice.title, "冰城");
        assert!(places.iter().all(|hit| !hit.title.starts_with("冰城 -")));
        let floors = catalog.search_scoped("monster", "", "zh", "", "2", "", "冰城");
        assert_eq!(floors[0].title, "冰城 - 所有");
        assert_eq!(floors[0].key, "span:冰城");
        let names: Vec<_> = floors.iter().skip(1).map(|hit| hit.title.as_str()).collect();
        assert_eq!(names, vec!["冰城 - 1層", "冰城 - 2層", "冰城 - 3層"]);
        let all = catalog.search_scoped("monster", "", "zh", "span:冰城", "2", "", "冰城");
        let one = catalog.search("monster", "", "zh", "32092", "2");
        assert!(!one.is_empty());
        assert!(one.iter().all(|hit| all.iter().any(|row| row.key == hit.key)));
        assert!(all.len() > one.len());
    }

    #[test]
    fn favourites_search_is_not_cut_off_at_eighty() {
        let catalog = Catalog::open().expect("db");
        let all = catalog.search("monster-all", "", "zh", "", "");
        assert!(all.len() > 80, "favourite scan stopped at {}", all.len());
        assert!(all.iter().any(|hit| hit.key == "monster:E190"));
        assert_eq!(catalog.search("monster", "a", "zh", "", "").len(), 80);
    }

    #[test]
    fn saved_monster_shows_on_outer_pages() {
        use std::collections::HashSet;
        let catalog = Catalog::open().expect("db");
        let monster = catalog
            .search("monster", "", "zh", "32092", "2")
            .into_iter()
            .find(|hit| hit.key.starts_with("monster:"))
            .expect("ice castle monster");
        let code = monster.key.trim_start_matches("monster:").to_string();
        let codes = HashSet::from([code]);
        assert!(catalog.scope_saved("region:32092", "2", &codes));
        assert!(catalog.scope_saved("area:冰城", "2", &codes));
        assert!(catalog.scope_saved("span:冰城", "2", &codes));
        assert!(catalog.scope_saved("nation:2", "2", &codes));
        assert!(!catalog.scope_saved("nation:1", "2", &codes));
        assert!(!catalog.scope_saved("region:32092", "2", &HashSet::new()));
    }

    #[test]
    fn quest_sheet_links_the_chain_and_targets() {
        let catalog = Catalog::open().expect("db");
        let id = quest_code_id(&catalog, "main", "50001");
        let prev = quest_code_id(&catalog, "main", "50000");
        let next = quest_code_id(&catalog, "main", "50002");
        let (text, links, icon) = catalog.open_key(&format!("quest:{id}"), "zh");
        assert!(icon.is_none());
        assert!(!text.is_empty());
        assert!(links.iter().all(|hit| !hit.key.starts_with("quest:")));
        let (prev_hit, next_hits) = catalog.quest_chain(id, "zh");
        assert_eq!(prev_hit.expect("prev").key, format!("quest:{prev}"));
        assert_eq!(next_hits.len(), 1);
        assert_eq!(next_hits[0].key, format!("quest:{next}"));
        assert!(links.iter().all(|hit| hit.key != "item:140"));
        assert!(links.iter().any(|hit| hit.key == "item:8762"));
        let (kill_text, kill_links, _) = catalog.open_key(&format!("quest:{next}"), "zh");
        assert!(!kill_text.is_empty());
        assert!(kill_links.iter().all(|hit| hit.key != "monster:M25"));
        let sheet = catalog.quest_sheet(id, "zh").expect("sheet");
        assert!(!sheet.0.is_empty());
        assert!(sheet.2.contains("50001"));
        assert!(sheet.3.iter().any(|(label, value, key)| label == "要物品" && key == "item:140" && value.contains('×')));
        assert!(catalog.linked_icon("item:140").is_some());
        assert!(sheet.3.iter().any(|(label, value, _)| label == "其他" && value == "等級 2"));
        let rewards = catalog.quest_reward_lines(id, "zh");
        assert!(rewards.iter().any(|(label, value)| label == "經驗" && value == "150,000"));
        assert!(rewards.iter().any(|(label, value)| label == "信用" && value == "100"));
        let kill_sheet = catalog.quest_sheet(next, "zh").expect("kill sheet");
        assert!(kill_sheet.3.iter().any(|(label, value, key)| label == "打怪物" && key == "monster:M25" && value.contains('×')));
        assert!(catalog.linked_icon("monster:M25").is_some());
        let sources = catalog.search("quest", "", "zh", "", "");
        assert_eq!(
            sources.iter().map(|hit| hit.key.as_str()).collect::<Vec<_>>(),
            vec!["qsrc:main", "qsrc:letter", "qsrc:intro", "qsrc:mercenary", "qsrc:dispatch"]
        );
        let starters = catalog.search_scoped("quest", "", "zh", "main", "", "", "0");
        assert!(!starters.is_empty());
        assert!(starters.iter().all(|hit| hit.key.starts_with("quest:")));
        assert!(starters.len() < 40);
        let (quest_id, npc_code): (i64, i64) = catalog
            .conn
            .query_row(
                "SELECT q.id, q.npc_code FROM quests q JOIN npcs n ON n.code = q.npc_code WHERE q.source = 'main' AND n.portrait > 0 LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let (_, links, icon) = catalog.open_key(&format!("quest:{quest_id}"), "zh");
        assert!(icon.is_some());
        assert_eq!(catalog.quest_giver(quest_id), format!("npc:{npc_code}"));
        assert!(links.iter().all(|hit| !hit.key.starts_with("npc:")));
        let (_, npc_links, _) = catalog.open_key(&format!("npc:{npc_code}"), "zh");
        assert!(npc_links.iter().any(|hit| hit.key == format!("quest:{quest_id}")));
        let story = quest_code_id(&catalog, "mercenary", "1");
        let note = catalog.quest_sheet(story, "zh").expect("story").4;
        assert!(note.contains('\n'));
        assert!(!note.contains("\\n"));
        let letter = quest_code_id(&catalog, "letter", "10101");
        let follow = quest_code_id(&catalog, "letter", "10102");
        let (_, letter_links, _) = catalog.open_key(&format!("quest:{letter}"), "zh");
        assert!(letter_links.iter().all(|hit| hit.key != format!("quest:{follow}")));
        let (_, letter_next) = catalog.quest_chain(letter, "zh");
        assert!(letter_next.iter().any(|hit| hit.key == format!("quest:{follow}")));
        let token = quest_code_id(&catalog, "main", "53026");
        let before = quest_code_id(&catalog, "main", "53025");
        let after = quest_code_id(&catalog, "main", "53027");
        let token_sheet = catalog.quest_sheet(token, "zh").expect("53026");
        assert!(token_sheet.3.iter().any(|(label, value, key)| label == "要物品" && key == "item:2263" && value.contains('×')));
        assert!(token_sheet.3.iter().any(|(label, value, key)| label == "其他" && value == "等級 120" && key.is_empty()));
        assert!(token_sheet.3.iter().all(|(label, _, _)| label != "經驗" && label != "信用"));
        let token_rewards = catalog.quest_reward_lines(token, "zh");
        assert!(token_rewards.iter().any(|(label, value)| label == "經驗" && value == "5,000,000"));
        assert!(token_rewards.iter().any(|(label, value)| label == "信用" && value == "5,000"));
        let (_, token_links, _) = catalog.open_key(&format!("quest:{token}"), "zh");
        assert!(token_links.iter().any(|hit| hit.key == "item:5713"));
        assert!(token_links.iter().any(|hit| hit.key == "item:11487"));
        assert!(token_links.iter().all(|hit| hit.aside.is_empty()));
        let (token_prev, token_next) = catalog.quest_chain(token, "zh");
        assert_eq!(token_prev.expect("53025").key, format!("quest:{before}"));
        assert_eq!(token_next.len(), 1);
        assert_eq!(token_next[0].key, format!("quest:{after}"));
    }

    #[test]
    fn npc_sheet_shows_name_nation_map_and_place() {
        let catalog = Catalog::open().expect("db");
        let hits = catalog.search("npc", "金春森", "zh-en", "", "");
        let hit = hits.iter().find(|row| row.title == "金春森").expect("kim choonsam");
        assert!(hit.icon.is_some());
        assert_eq!(hit.aside, "全區");
        let code: i64 = hit.key.trim_start_matches("npc:").parse().unwrap();
        let (name, alt, map, facts, note) = catalog.npc_sheet(code, "zh-en").unwrap();
        assert_eq!(name, "金春森");
        assert_eq!(alt, "Kim Choonsam");
        assert!(!map.is_empty());
        assert!(facts.iter().any(|(label, value)| label == "國籍" && value == "全區"));
        assert!(facts.iter().any(|(label, value)| label == "地圖" && value == &map));
        assert!(facts.iter().any(|(label, value)| label == "座標" && value.contains(',')));
        assert!(note.contains("平壤"));
    }

    #[test]
    fn hero_sheet_shows_rank_nation_and_hire() {
        let catalog = Catalog::open().expect("db");
        let ranks = catalog.search("hero", "", "zh-en", "", "");
        assert!(ranks.iter().any(|hit| hit.key == "rank:0" && hit.title == "傭兵"));
        let (name, alt, heading, facts, note) = catalog.hero_sheet("U00", "zh-en").expect("porter");
        assert!(!name.is_empty());
        assert!(!alt.is_empty() || name != "U00");
        assert!(heading.contains("傭兵"));
        assert!(heading.contains("U00"));
        assert!(facts.iter().any(|(label, value)| label == "國籍" && value == "朝鮮"));
        assert!(facts.iter().any(|(label, _)| label == "雇用"));
        assert!(note.contains("行李"));
        let kings = catalog.search_scoped("hero", "", "zh-en", "9", "", "", "");
        assert!(kings.iter().any(|hit| hit.title == "持國天王"));
        let lands = catalog.search_scoped("hero", "", "zh-en", "", "", "land", "");
        assert!(lands.iter().any(|hit| hit.key == "land:1" && hit.title == "朝鮮"));
        let tiers = catalog.search_scoped("hero", "", "zh-en", "1", "", "land", "");
        assert!(tiers.iter().any(|hit| hit.key == "band:0" && hit.title == "傭兵"));
        assert!(tiers.iter().any(|hit| hit.key == "band:1" && hit.title == "一轉"));
        assert!(tiers.iter().any(|hit| hit.key == "band:2" && hit.title == "二轉"));
        let swords = catalog.search_scoped("hero", "", "zh-en", "1", "", "land", "0");
        assert!(swords.iter().any(|hit| hit.key == "hero:U86"));
        assert!(catalog.hero_mark().is_some());
        let (_, chain, _) = catalog.open_key("hero:U86", "zh-en");
        assert_eq!(chain.len(), 4);
        assert_eq!(chain[0].key, "hero:U86");
        assert_eq!(chain[0].aside, "原本");
        assert_eq!(chain[1].key, "hero:U87");
        assert_eq!(chain[1].aside, "一轉");
        assert_eq!(chain[2].aside, "二轉");
        assert_eq!(chain[3].aside, "改造");
        assert!(chain[0].subtitle.contains("轉職"));
        assert!(chain.iter().all(|hit| hit.chip != "label"));
        let (_, kundali, _) = catalog.open_key("hero:T86", "zh");
        let source_at = kundali.iter().position(|hit| hit.key == "hero:T86" && hit.chip != "label").expect("kundali");
        let awa_at = kundali.iter().position(|hit| hit.key == "hero:V29").expect("awa kundali");
        assert!(kundali[source_at..awa_at].iter().all(|hit| !hit.key.starts_with("item:")));
        assert!(kundali.iter().any(|hit| hit.chip == "section" && hit.key == "need"));
        let (_, awa_kundali, _) = catalog.open_key("hero:V29", "zh");
        let heading = awa_kundali.iter().position(|hit| hit.chip == "label" && hit.title.contains("260")).expect("level");
        assert!(!awa_kundali[heading].title.contains(",,") && !awa_kundali[heading].title.contains('['));
        let mats: Vec<_> = awa_kundali[heading + 1..].iter().take_while(|hit| hit.chip != "label").filter(|hit| hit.key.starts_with("item:")).collect();
        assert_eq!(mats.len(), 4);
        let relic = mats.iter().find(|hit| hit.key == "item:12793").expect("relic");
        assert_eq!(relic.aside, "× 1");
        assert!(relic.icon.is_some());
        assert_eq!(mats.iter().find(|hit| hit.key == "item:7063").unwrap().aside, "× 200");
        assert_eq!(mats.iter().find(|hit| hit.key == "item:5860").unwrap().aside, "× 10");
        assert_eq!(mats.iter().find(|hit| hit.key == "item:10880").unwrap().aside, "× 200");
        let (_, kings, _) = catalog.open_key("hero:T49", "zh");
        assert!(kings.iter().any(|hit| hit.key == "item:4066" && hit.aside == "× 1"));
        let (_, qiongqi, _) = catalog.open_key("hero:T65", "zh");
        assert!(qiongqi.iter().any(|hit| hit.key == "item:3669" && hit.aside == "× 75"));
        assert!(qiongqi.iter().any(|hit| hit.key == "item:5798" && hit.aside == "× 2"));
        let (_, promoted, _) = catalog.open_key("hero:D78", "zh");
        let bead = promoted.iter().find(|hit| hit.key == "item:853").expect("seal bead");
        assert_eq!(bead.aside, "× 7");
        assert!(promoted.iter().any(|hit| hit.chip == "label" && hit.title.contains("轉職")));
        let (_, virudhaka, _) = catalog.open_key("hero:T51", "zh");
        let line_end = virudhaka.iter().position(|hit| hit.chip == "section" && hit.key == "need").unwrap_or(virudhaka.len());
        assert!(virudhaka[..line_end].iter().all(|hit| !hit.key.starts_with("item:")));
        assert!(virudhaka.iter().all(|hit| !hit.title.contains(",,") && !hit.title.contains('[')));
        assert!(virudhaka.iter().any(|hit| hit.key == "hero:E85"));
        assert!(virudhaka.iter().all(|hit| hit.key != "item:10745"));
        let (_, awa_viru, _) = catalog.open_key("hero:V09", "zh");
        assert!(awa_viru.iter().any(|hit| hit.chip == "label" && hit.title.contains("260") && !hit.title.contains('[') && !hit.title.contains(",,")));
        assert!(awa_viru.iter().any(|hit| hit.key == "item:10745"));
        let (_, king, _) = catalog.open_key("hero:T49", "zh");
        assert!(king.iter().any(|hit| hit.key == "item:5789"));
        assert!(king.iter().any(|hit| hit.chip == "label" && hit.title.contains("持國天王")));
        let (_, kirin, _) = catalog.open_key("hero:E85", "zh");
        assert!(kirin.iter().all(|hit| hit.key != "item:5789"));
        assert!(kirin.iter().any(|hit| hit.key == "hero:T49" && hit.subtitle == "組合" && hit.chip != "label"));
        let (_, awa, _) = catalog.open_key("hero:T62", "zh");
        let section = awa.iter().position(|hit| hit.chip == "section" && hit.key == "need").expect("requirements");
        assert!(awa[..section].iter().all(|hit| !hit.key.starts_with("item:")));
        let hire = awa.iter().position(|hit| hit.chip == "label" && hit.title.contains("僱")).expect("hire");
        let route = awa.iter().position(|hit| hit.key == "hero:T86" && hit.subtitle == "組合").expect("kundali route");
        assert!(section < hire && hire < route);
        assert!(awa.iter().all(|hit| hit.key != "hero:H47"));
        let (_, airavata, _) = catalog.open_key("hero:T79", "zh");
        let related = airavata.iter().position(|hit| hit.chip == "section" && hit.key == "related").expect("related");
        let route = airavata.iter().position(|hit| hit.key == "hero:T113" && hit.subtitle == "組合").expect("acala");
        assert!(related < route);
        assert!(airavata[..related].iter().all(|hit| hit.key != "hero:T113"));
        let (_, tortoise, _) = catalog.open_key("hero:E77", "zh");
        assert!(tortoise.iter().any(|hit| hit.title.contains("不同幻獸石像") && hit.aside == "× 3"));
        assert!(tortoise.iter().any(|hit| hit.chip == "label" && hit.subtitle.contains("95")));
        for statue in [3817, 3828, 3839, 3850] {
            let row = tortoise.iter().find(|hit| hit.key == format!("item:{statue}")).expect("statue");
            assert!(row.icon.is_some(), "{statue}");
            assert!(row.title.contains("200"), "{statue}");
            assert_eq!(row.aside, "任選");
        }
        assert!(tortoise.iter().any(|hit| hit.key == "item:3897" && hit.aside == "× 10"));
        assert!(tortoise.iter().any(|hit| hit.key == "item:3506" && hit.aside == "× 3"));
        let (_, kirin_make, _) = catalog.open_key("hero:E85", "zh");
        assert!(kirin_make.iter().any(|hit| hit.chip == "label" && hit.subtitle.contains("100")));
        assert!(kirin_make.iter().any(|hit| hit.key == "item:3894" && hit.aside == "× 1" && hit.title.contains("200")));
        assert!(kirin_make.iter().any(|hit| hit.key == "item:3861" && hit.aside == "× 1" && hit.title.contains("200")));
        assert!(kirin_make.iter().any(|hit| hit.key == "item:3872" && hit.aside == "× 1" && hit.title.contains("200")));
        assert!(kirin_make.iter().any(|hit| hit.key == "item:3883" && hit.aside == "× 1" && hit.title.contains("200")));
        assert!(kirin_make.iter().any(|hit| hit.key == "item:3899" && hit.aside == "× 10"));
        assert!(kirin_make.iter().any(|hit| hit.key == "item:3506" && hit.aside == "× 4"));
        let (_, turtle, _) = catalog.open_key("hero:T40", "zh");
        assert!(turtle.iter().any(|hit| hit.chip == "label" && hit.title.contains("改造")));
        let (_, awakened, _) = catalog.open_key("hero:V01", "zh");
        assert!(awakened.iter().any(|hit| hit.chip == "label" && hit.title.contains("覺醒")));
        let (_, jumong, _) = catalog.open_key("hero:T80", "zh");
        assert!(jumong.iter().any(|hit| hit.chip == "label" && hit.title.contains("轉職")));
        let (_, fox, _) = catalog.open_key("hero:M12", "zh");
        assert!(fox.iter().any(|hit| hit.title.contains("訓練所")));
        assert!(fox.iter().all(|hit| !hit.title.contains("封印珠")));
        let (_, bokuden, _) = catalog.open_key("hero:T118", "zh");
        assert!(bokuden.iter().any(|hit| hit.key == "item:6831" && hit.aside == "× 4"));
        assert!(bokuden.iter().any(|hit| hit.key == "hero:T63"));
        assert!(bokuden.iter().any(|hit| hit.key == "hero:T57"));
        assert!(bokuden.iter().any(|hit| hit.key == "item:5747" && hit.aside == "× 150"));
        let (_, relic, _) = catalog.open_key("item:12793", "zh");
        assert!(relic.iter().any(|hit| hit.chip == "section" && hit.title == "角色"));
        assert!(relic.iter().any(|hit| hit.key == "hero:V29" && hit.subtitle.contains("覺醒")));
        let (_, none, _) = catalog.open_key("hero:U00", "zh-en");
        assert!(none.is_empty());
        let japan = catalog.search_scoped("hero", "", "zh", "10", "", "land", "0");
        let ronin = japan.iter().find(|hit| hit.key == "hero:U09").expect("ronin");
        assert_eq!(ronin.title, "劍術浪人");
        assert_eq!(ronin.aside, "日本");
        assert!(ronin.icon.is_some());
        assert!(japan.iter().all(|hit| hit.aside == "日本"));
        let (name, _, _, facts, _) = catalog.hero_sheet("U09", "zh").unwrap();
        assert_eq!(name, "劍術浪人");
        assert!(facts.iter().any(|(label, value)| label == "國籍" && value == "日本"));
        for (land, label) in [("100", "中國"), ("1000", "台灣"), ("10000", "印度")] {
            let hits = catalog.search_scoped("hero", "", "zh", land, "", "land", "0");
            assert!(!hits.is_empty(), "{land}");
            assert!(hits.iter().all(|hit| hit.aside == label), "{land}");
        }
    }

    #[test]
    fn production_lists_buildings_then_goods() {
        let catalog = Catalog::open().expect("db");
        let buildings = catalog.search("works", "", "zh", "", "");
        assert!(buildings.iter().all(|hit| hit.key != "yard:LMB" && hit.key != "yard:MIN"));
        let factory = buildings.iter().find(|hit| hit.key == "yard:FAC").expect("factory");
        assert!(!factory.title.is_empty());
        let pasture = buildings.iter().find(|hit| hit.key == "yard:RAN").expect("pasture");
        assert!(!pasture.title.is_empty());
        let farm = catalog.search_scoped("works", "", "zh", "FAR", "", "", "");
        let orchard = farm.iter().find(|hit| hit.key == "bay:FRU").expect("orchard");
        assert!(!orchard.title.is_empty());
        assert!(orchard.icon.is_some());
        assert!(orchard.aside.is_empty());
        let goods = catalog.search_scoped("works", "", "zh", "FAR", "", "", "FRU");
        let apple = goods.iter().find(|hit| hit.key == "item:12").expect("orchard goods");
        assert!(apple.icon.is_some());
        assert!(apple.aside.contains("蒙古"));
        let everywhere = goods.iter().find(|hit| hit.key == "item:11").expect("all regions");
        assert_eq!(everywhere.aside, "所有地區");
        assert!(goods.iter().all(|hit| hit.key.starts_with("item:") && hit.icon.is_some()));
        let butcher = catalog.search_scoped("works", "", "zh", "RAN", "", "", "");
        assert!(butcher.iter().any(|hit| hit.key == "bay:BUT"));
        let stable = butcher.iter().find(|hit| hit.key == "bay:BAN").expect("stable");
        assert!(stable.icon.is_some());
        let woven = catalog.search_scoped("works", "", "zh", "FAC", "", "", "WEA");
        let silk = woven.iter().find(|hit| hit.key == "item:597").expect("china weave");
        assert_eq!(silk.aside, "中國·台灣");
        assert!(woven.iter().any(|hit| hit.aside == "日本"));
        assert!(woven.iter().any(|hit| hit.aside == "所有地區"));
        assert!(woven.iter().all(|hit| hit.aside != "朝鮮 · 日本 · 中國·台灣"));
        assert!(woven.iter().all(|hit| hit.aside != "Weaving Shop"));
        let (_, links, _) = catalog.open_key("item:597", "zh");
        assert!(links.iter().any(|hit| hit.key == "section:make"));
        assert!(!links.iter().any(|hit| hit.chip == "note"));
        assert!(links.iter().any(|hit| hit.key == "bay:WEA" && hit.aside == "中國·台灣"));
        assert!(links.iter().any(|hit| hit.key == "item:63" && hit.chip == "make" && hit.subtitle.contains("200")));
        let (_, fang, _) = catalog.open_key("item:4550", "zh");
        assert!(fang.iter().any(|hit| hit.key == "bay:SPC" && hit.title.contains("特殊武器")));
        let (_, club, _) = catalog.open_key("item:73", "zh");
        let sword = club.iter().find(|hit| hit.key == "bay:SWD").expect("sword factory");
        assert_eq!(sword.aside, "中國");
        assert!(sword.subtitle.contains("武器"));
        assert_eq!(club.iter().filter(|hit| hit.key.starts_with("bay:")).count(), 1);
        assert!(!catalog.work_parent("SWD").is_empty());
        assert!(club.iter().any(|hit| hit.chip == "make" && hit.key.is_empty() && hit.title.starts_with("勞動")));
        let (_, skin, _) = catalog.open_key("item:6886", "zh");
        let crafts: Vec<_> = skin.iter().filter(|hit| hit.chip == "craft-how" && hit.key.starts_with("recipe:")).collect();
        assert_eq!(crafts.len(), 1);
        assert_eq!(crafts[0].key, "recipe:659");
        let (_, quested, _) = catalog.open_key("item:110", "zh");
        assert!(quested.iter().any(|hit| hit.chip == "used-for" && hit.aside == "任務"));
    }

    #[test]
    fn town_shops_show_on_the_item_and_the_map() {
        let catalog = Catalog::open().expect("db");
        let (_, links, _) = catalog.open_key("item:12", "zh");
        assert!(links.iter().any(|hit| hit.chip == "section" && hit.title == "在賣"));
        assert!(links.iter().any(|hit| hit.key == "town:KY000001" && hit.title == "漢陽" && hit.aside == "特產"));
        let realms = catalog.search("atlas", "", "zh", "", "");
        assert!(realms.iter().any(|hit| hit.key == "realm:joseon" && hit.title == "朝鮮"));
        assert!(realms.iter().all(|hit| hit.key != "realm:field"));
        let towns = catalog.search_scoped("atlas", "", "zh", "joseon", "", "", "");
        assert!(towns.iter().any(|hit| hit.key == "town:KY000001" && hit.title == "漢陽"));
        let goods = catalog.search_scoped("atlas", "", "zh", "joseon", "", "", "KY000001");
        assert!(goods.iter().any(|hit| hit.key == "item:12" && hit.aside == "特產"));
    }

    #[test]
    fn item_sheet_lists_the_enhance_curve() {
        let catalog = Catalog::open().expect("db");
        let (_, _, _, lines, _) = catalog.item_sheet(9077, "zh-en").expect("star weapon");
        let curve: Vec<_> = lines.iter().filter(|(_, value)| value.starts_with('+')).map(|(_, value)| value.as_str()).collect();
        assert!(curve.len() >= 10);
        assert!(curve[0].contains("+1"));
        assert!(curve[0].contains("60"));
        assert!(curve.iter().any(|line| line.contains("+10") && line.contains("600")));
        let (_, _, _, bracelet, _) = catalog.item_sheet(6163, "zh-en").expect("bracelet");
        let fire: Vec<_> = bracelet.into_iter().filter(|(_, value)| value.starts_with('+')).map(|(_, value)| value).collect();
        assert!(fire.iter().any(|line| line.contains("+5") && line.contains('5')));
        assert!(fire.iter().any(|line| line.contains('火')));
    }

    #[test]
    fn item_page_groups_by_category() {
        let catalog = Catalog::open().expect("db");
        let kinds = catalog.search("item", "", "zh-en", "", "");
        assert!(kinds.iter().any(|hit| hit.key == "icat:W" && hit.title == "武器"));
        assert!(kinds.iter().any(|hit| hit.key == "icat:E" && hit.title == "材料"));
        assert!(kinds.iter().any(|hit| hit.key == "icat:other" && hit.title == "其他"));
        let weapons = catalog.search("item", "", "zh-en", "W", "");
        assert!(weapons.iter().any(|hit| hit.key == "isub:axe" && hit.title == "斧" && hit.icon.is_some()));
        assert!(weapons.iter().any(|hit| hit.key == "isub:bow" && hit.title == "弓"));
        assert!(weapons.iter().any(|hit| hit.key == "isub:sword" && hit.title == "劍"));
        let axes = catalog.search_scoped("item", "斧", "zh-en", "W", "", "", "axe");
        assert!(axes.iter().any(|hit| hit.title.contains('斧')));
        let bows = catalog.search_scoped("item", "斧", "zh-en", "W", "", "", "bow");
        assert!(bows.is_empty());
        let (_, _, beast) = catalog.open_key("item:3895", "zh-en");
        assert!(beast.is_some(), "神獸之根源 should have an inventory image");
        let (_, _, charm) = catalog.open_key("item:1012", "zh-en");
        assert!(charm.is_some(), "護身符 should have an inventory image");
        let (_, relations, _) = catalog.open_key("item:110", "zh-en");
        assert!(relations.iter().any(|hit| hit.chip == "section" && hit.title == "合成"));
        assert!(relations.iter().any(|hit| hit.key.starts_with("recipe:")));
        assert!(relations.iter().any(|hit| hit.chip == "section" && hit.title == "用途"));
        assert!(relations.iter().any(|hit| hit.key.starts_with("quest:")));
        let sources = catalog.search("recipe", "", "zh-en", "", "");
        assert!(sources.iter().any(|hit| hit.key.starts_with("craft:") && hit.icon.is_some()));
        assert!(kinds.iter().any(|hit| hit.key == "icat:set" && hit.title == "套裝"));
        let sets = catalog.search("item", "夜叉", "zh-en", "set", "");
        let yaksa = sets.iter().find(|hit| hit.key == "set:79").expect("vajrayaksa set");
        assert!(yaksa.title.contains("夜叉"));
        assert_eq!(yaksa.subtitle.contains("6"), true);
        let (_, _, _, sheet, _) = catalog.set_sheet(79, "zh-en").expect("vajrayaksa sheet");
        assert!(sheet.iter().any(|(label, value, _, _)| {
            label == "部位" && value.contains("頭") && value.contains("甲") && value.contains("手") && value.contains("腰") && value.contains("鞋") && value.contains("戒×2")
        }));
        let (_, pieces, icon) = catalog.open_key("set:79", "zh-en");
        assert!(icon.is_some());
        for code in [7126, 7127, 7128, 7129, 7130, 9465] {
            assert!(pieces.iter().any(|hit| hit.key == format!("item:{code}")), "{code}");
        }
        assert!(pieces.iter().any(|hit| hit.key == "section:H" && hit.chip == "section"));
        assert!(pieces.iter().any(|hit| hit.key == "section:R" && hit.title.contains("×2")));
        let belt = pieces.iter().position(|hit| hit.key == "section:B").expect("belt slot");
        let next = pieces.iter().skip(belt + 1).position(|hit| hit.chip == "section").unwrap_or(pieces.len() - belt - 1);
        let belt_items = &pieces[belt + 1..belt + 1 + next];
        assert!(belt_items.iter().any(|hit| hit.key == "item:7129"));
        assert!(belt_items.iter().all(|hit| hit.key != "item:8302"));
        let saved_extra = std::fs::read_to_string(data_dir().join("set-extra.txt")).ok();
        save_set_extra(false);
        let (_, amaterasu, _) = catalog.open_key("set:32", "zh-en");
        let shoes = amaterasu.iter().position(|hit| hit.key == "section:S").expect("shoes");
        let shoes_end = amaterasu.iter().skip(shoes + 1).position(|hit| hit.chip == "section").unwrap_or(amaterasu.len() - shoes - 1);
        let shoe_items = &amaterasu[shoes + 1..shoes + 1 + shoes_end];
        assert!(shoe_items.iter().any(|hit| hit.key == "item:5306"));
        assert!(shoe_items.iter().all(|hit| hit.key != "item:8280" && hit.key != "item:8375" && hit.key != "item:7793"));
        let (_, spear, _) = catalog.open_key("set:34", "zh-en");
        assert!(spear.iter().any(|hit| hit.key == "item:5488"), "untradeable set piece stays");
        let recipe_id: i64 = catalog
            .conn
            .query_row(
                "SELECT r.id FROM recipes r
                 JOIN recipe_materials m ON m.recipe_id = r.id
                 WHERE r.result_item = 11887 AND m.item_code = 6923",
                [],
                |row| row.get(0),
            )
            .expect("choi ring recipe");
        let (_, mats, _) = catalog.open_key(&format!("recipe:{recipe_id}"), "zh-en");
        let ring = mats.iter().find(|hit| hit.title.contains("廣開土")).expect("kwanggaeto ring");
        assert!(ring.title.contains("太皇"));
        assert!(ring.icon.is_some());
        let charm = catalog.search_scoped("item", "", "zh-en", "set", "", "", "1");
        assert!(charm.iter().any(|hit| hit.key == "set:98"));
        assert!(charm.iter().all(|hit| hit.key != "set:79"));
        let normal = catalog.search_scoped("item", "", "zh-en", "set", "", "", "0");
        assert!(normal.iter().any(|hit| hit.key == "set:79"));
        assert!(normal.iter().all(|hit| hit.key != "set:98"));
        let (_, dipper, _) = catalog.open_key("set:98", "zh-en");
        let charm_belt = dipper.iter().find(|hit| hit.key == "item:8421").expect("dipper belt");
        assert!(charm_belt.title.contains("北斗七星"));
        assert!(charm_belt.title.contains("腰帶"));
        let (_, _, _, dipper_sheet, _) = catalog.set_sheet(98, "zh-en").expect("dipper sheet");
        assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "力量" && value == "+25" && section == "item"));
        assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "體質" && value == "+480" && section == "item"));
        assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "智力" && value == "+100" && section == "item"));
        assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "防禦" && value == "+150" && section == "item"));
        assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "打擊抗性" && value == "+30" && section == "item"));
        assert!(dipper_sheet.iter().any(|(label, value, _, section)| label == "魔法抗性" && value == "+25" && section == "item"));
        let (_, arhat, arhat_icon) = catalog.open_key("set:140", "zh-en");
        assert!(arhat_icon.is_some(), "arhat set borrows the base icon");
        let necklace = arhat.iter().find(|hit| hit.key == "item:12036").expect("necklace");
        assert!(necklace.icon.is_some());
        assert!(!necklace.title.is_empty());
        let (_, _, _, arhat_sheet, _) = catalog.set_sheet(140, "zh-en").expect("arhat sheet");
        assert!(arhat_sheet.iter().any(|(label, _, key, _)| label == "道具" && key == "fold:item"));
        assert!(arhat_sheet.iter().any(|(_, value, key, section)| key.starts_with("item:") && value.contains("項鍊") && section == "item"));
        assert!(arhat_sheet.iter().any(|(label, value, _, section)| label == "暴擊傷害" && value == "+20" && section == "item"));
        assert!(arhat_sheet.iter().any(|(label, _, key, _)| label == "合計" && key == "fold:total"));
        assert!(arhat_sheet.iter().any(|(label, value, _, section)| label == "暴擊傷害" && value == "+40" && section == "total"));
        assert!(arhat_sheet.iter().any(|(label, value, _, section)| label == "體質" && value == "+200" && section == "set"));
        let (_, deity_pieces, deity_icon) = catalog.open_key("set:137", "zh-en");
        assert!(deity_icon.is_some(), "deity set borrows the base icon");
        let deity_necklace = deity_pieces.iter().find(|hit| hit.key == "item:11943").expect("deity necklace");
        assert!(deity_necklace.icon.is_some(), "deity necklace borrows the base icon");
        let (_, _, _, deity, _) = catalog.set_sheet(137, "zh-en").expect("deity sheet");
        assert!(deity.iter().any(|(_, _, key, _)| key == "item:11943"));
        assert!(deity.iter().any(|(label, value, _, section)| label == "屬性值" && value == "+5" && section == "item"));
        assert!(deity.iter().any(|(label, value, _, section)| label == "土" && value == "+3" && section == "item"));
        let (_, _, _, sage, _) = catalog.set_sheet(138, "zh-en").expect("sage set");
        assert!(sage.iter().any(|(label, value, _, section)| label == "傷害" && value == "+5%" && section == "item"));
        assert!(sage.iter().any(|(label, _, key, _)| label == "合計" && key == "fold:total"));
        assert!(sage.iter().any(|(label, value, _, section)| label == "傷害" && value == "+15%" && section == "total"));
        match saved_extra {
            Some(text) => {
                let _ = std::fs::write(data_dir().join("set-extra.txt"), text);
            }
            None => {
                let _ = std::fs::remove_file(data_dir().join("set-extra.txt"));
            }
        }
        let (_, _, _, shoe_lines, shoes) = catalog.item_sheet(6257, "zh-en").expect("zhuge shoes");
        assert!(shoes.contains("諸葛"));
        assert!(!shoes.contains("Shoes"));
        assert!(shoe_lines.iter().any(|(label, value)| label == "體質" && value == "+50"));
        assert!(shoe_lines.iter().any(|(label, value)| label == "力量" && value == "+50"));
        assert!(shoe_lines.iter().any(|(label, value)| label == "敏捷" && value == "+50"));
        assert!(shoe_lines.iter().any(|(label, value)| label == "智力" && value == "+50"));
        assert!(shoe_lines.iter().any(|(label, value)| label == "打擊抗性" && value == "+20"));
        assert!(shoe_lines.iter().any(|(label, value)| label == "魔法抗性" && value == "+20"));
        let (_, _, _, shoe_en, shoes_en) = catalog.item_sheet(6257, "en-zh").expect("zhuge shoes en");
        assert!(shoes_en.contains("Shoes"));
        assert!(!shoes_en.contains("諸葛"));
        assert!(shoe_en.iter().any(|(label, value)| label == "VIT" && value == "+50"));
        let (_, _, _, lines, _) = catalog.item_sheet(7126, "zh-en").expect("headband");
        assert!(lines.iter().any(|(label, value)| label == "套裝" && value.contains("夜叉")));
        assert!(lines.iter().any(|(label, value)| label == "2件" && value.contains("智力") && value.contains("75")));
        assert!(lines.iter().any(|(label, value)| label == "6件" && value.contains("425") && value.contains("屬性")));
        let (_, relations, _) = catalog.open_key("item:7126", "zh-en");
        assert!(relations.iter().any(|hit| hit.key == "set:79" && hit.chip == "set"));
        assert!(relations.iter().any(|hit| hit.key == "section:set" && hit.title == "套裝"));
        assert_eq!(relation_label("set", "zh-en"), "套裝");
        assert_eq!(relation_label("set", "en-zh"), "Set");
        let kinds_icon = kinds.iter().find(|hit| hit.key == "icat:W").unwrap();
        assert!(kinds_icon.icon.is_some());
        let (_, _, _, lines, _) = catalog.item_sheet(58, "zh-en").expect("belt");
        assert!(lines.iter().any(|(label, value)| label == "分類" && value == "腰帶"));
    }

    #[test]
    fn shipped_pictures_open_without_the_game_client() {
        let db = find_db().expect("db");
        let client = locate_client(PathBuf::from(r"Z:\no-such-client"), db.parent().unwrap());
        assert!(pictures_ready(&client), "{}", client.display());
        assert!(!client.join("gersang.gcs").is_file());
        assert!(client.join("Assets").join("Textures").join("Items").is_dir());
    }

    fn quest_code_id(catalog: &Catalog, source: &str, code: &str) -> i64 {
        catalog
            .conn
            .query_row(
                "SELECT id FROM quests WHERE source = ?1 AND code = ?2",
                (source, code),
                |row| row.get(0),
            )
            .unwrap()
    }
}
