use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::{Catalog, Hit};
use super::names::haystack;
use super::recipe::formula_lands;
use super::rows::{FormulaPart, Index, ItemFormula, MakePlace, WorkBuilding, WorkFacility, WorkProduct, WorkUpgradeName};
use super::store::{column_exists, online, table_exists};
use super::text::{caption, commas, lead3, phrase, trail3};

pub(in crate::db) fn work_lands(nations: &str, lang: &str) -> String {
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

pub(in crate::db) fn chain_role(slot: i64, lang: &str) -> String {
    let key = match slot {
        1 => "job-base",
        2 => "job-first",
        3 => "job-second",
        4 => "job-remodel",
        _ => return String::new(),
    };
    phrase(key, lang)
}

pub(in crate::db) fn load_works(
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
        let mut parts = vec![ko.clone(), code.clone(), en.clone(), zh.clone()];
        for product in &mine {
            parts.push(product.item.to_string());
        }
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        yards.insert(
            code.clone(),
            WorkFacility {
                code: code.clone(),
                ko,
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
        let mut parts = vec![ko.clone(), code.clone(), en.clone(), zh.clone()];
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
            ko,
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

pub(in crate::db) fn work_art(root: &Path, relative: &str, frame: i32) -> Option<(PathBuf, i32)> {
    let path = root.join(relative);
    path.is_file().then_some((path, frame))
}

pub(in crate::db) fn building_art(root: &Path, code: &str) -> Option<(PathBuf, i32)> {
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

pub(in crate::db) fn facility_art(root: &Path, code: &str) -> Option<(PathBuf, i32)> {
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

pub(in crate::db) fn load_formulas(conn: &Connection) -> Result<HashMap<i64, ItemFormula>, String> {
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

pub(in crate::db) fn load_work_upgrades(conn: &Connection, texts: &HashMap<String, String>) -> Result<HashMap<String, WorkUpgradeName>, String> {
    if !table_exists(conn, "work_upgrades") {
        return Ok(HashMap::new());
    }
    let sql = if column_exists(conn, "work_upgrades", "effect_ko") {
        "SELECT code, name_ko, name_gts, effect_gts, effect_ko FROM work_upgrades"
    } else {
        "SELECT code, name_ko, name_gts, effect_gts, '' FROM work_upgrades"
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut upgrades = HashMap::new();
    for row in rows {
        let (code, ko, gts, effect, effect_ko) = row.map_err(|err| err.to_string())?;
        let en = online(texts, gts, "en").to_string();
        let zh = online(texts, gts, "zh").to_string();
        let effect_en = online(texts, effect, "en");
        let effect_zh = online(texts, effect, "zh");
        upgrades.insert(
            code,
            WorkUpgradeName {
                ko: if effect_ko.is_empty() { ko } else { format!("{ko} {effect_ko}") },
                en: if effect_en.is_empty() { en } else { format!("{en} {effect_en}") },
                zh: if effect_zh.is_empty() { zh } else { format!("{zh} {effect_zh}") },
            },
        );
    }
    Ok(upgrades)
}

impl Catalog {
    pub(in crate::db) fn production_for(&self, code: i64, lang: &str) -> Vec<Hit> {
        let formula = self.index.formulas.get(&code);
        let mut places = self.factory_places(code, lang);
        if places.is_empty() {
            if let Some(formula) = formula {
                if let Some(place) = self.inferred_place(formula, lang) {
                    places.push(place);
                }
            }
        }
        // Default yield-only rows do not establish a production method. Keep
        // recorded facilities, labor, ingredients, and tools, but hide empty shells.
        if places.is_empty() && formula.is_some_and(|formula| {
            formula.labor == 0 && formula.plant.is_empty() && formula.parts.is_empty()
        }) {
            return Vec::new();
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

    pub(in crate::db) fn formulas_using(&self, code: i64, lang: &str) -> Vec<Hit> {
        let mut products: Vec<(i64, i64)> = Vec::new();
        for (product, formula) in &self.index.formulas {
            if *product == code {
                continue;
            }
            let qty = formula
                .parts
                .iter()
                .filter(|part| part.kind == "mat" && part.item == code)
                .map(|part| part.qty)
                .sum::<i64>();
            if qty > 0 {
                products.push((*product, qty));
            }
        }
        products.sort_by_key(|(product, _)| *product);
        products
            .into_iter()
            .map(|(product, qty)| Hit {
                title: self.item_name(product, lang),
                subtitle: format!("× {}", commas(qty)),
                key: format!("item:{product}"),
                icon: self.item_icon(product),
                solo: false,
                aside: phrase("make", lang),
                badge: None,
                chip: String::new(),
            })
            .collect()
    }

    pub(in crate::db) fn factory_places(&self, code: i64, lang: &str) -> Vec<MakePlace> {
        let mut places = Vec::new();
        for building in &self.index.works {
            let building_name = lead3(lang, &building.en, &building.zh, &building.ko);
            for facility in &building.facilities {
                let Some(yard) = self.index.yards.get(facility) else { continue };
                let Some(product) = yard.products.iter().find(|product| product.item == code) else { continue };
                let facility_name = lead3(lang, &yard.en, &yard.zh, &yard.ko);
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

    pub(in crate::db) fn inferred_place(&self, formula: &ItemFormula, lang: &str) -> Option<MakePlace> {
        if formula.plant.is_empty() {
            return None;
        }
        let yard = self.index.yards.get(&formula.plant)?;
        let building = self.index.works.iter().find(|row| row.facilities.iter().any(|code| code == &yard.code));
        let facility = lead3(lang, &yard.en, &yard.zh, &yard.ko);
        Some(MakePlace {
            region: formula_lands(formula.country, lang),
            building: building.map(|row| lead3(lang, &row.en, &row.zh, &row.ko)).unwrap_or_default(),
            facility: if facility.is_empty() { yard.code.clone() } else { facility },
            key: format!("bay:{}", yard.code),
            icon: yard.icon.clone(),
        })
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
}

impl Index {
    pub(in crate::db) fn listed_building(&self, code: &str) -> bool {
        !matches!(code, "LMB" | "MIN")
    }

    pub(in crate::db) fn listed_facility(&self, code: &str) -> bool {
        self.works
            .iter()
            .any(|row| self.listed_building(&row.code) && row.facilities.iter().any(|slot| slot == code))
    }

    pub(in crate::db) fn work_hits(&self, query: &str, folded: &str, lang: &str, building: &str, facility: &str) -> Vec<Hit> {
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
            }
        }
        hits
    }

    pub(in crate::db) fn work_buildings(&self, lang: &str) -> Vec<Hit> {
        self.works
            .iter()
            .filter(|row| self.listed_building(&row.code))
            .map(|row| self.work_building_hit(row, lang))
            .collect()
    }

    pub(in crate::db) fn work_building_hit(&self, row: &WorkBuilding, lang: &str) -> Hit {
        let credit = format!("{} {}", phrase("credit", lang), commas(row.credit));
        Hit {
            title: lead3(lang, &row.en, &row.zh, &row.ko),
            subtitle: caption(&trail3(lang, &row.en, &row.zh, &row.ko), format!("{} · {credit}", commas(row.cost))),
            key: format!("yard:{}", row.code),
            icon: row.icon.clone(),
            solo: false,
            aside: String::new(),
            badge: None,
            chip: String::new(),
        }
    }

    pub(in crate::db) fn work_facilities(&self, building: &str, query: &str, folded: &str, lang: &str) -> Vec<Hit> {
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

    pub(in crate::db) fn work_facility_hit(&self, yard: &WorkFacility, lang: &str) -> Hit {
        let count = format!("{} {}", commas(yard.products.len() as i64), phrase("goods", lang));
        Hit {
            title: lead3(lang, &yard.en, &yard.zh, &yard.ko),
            subtitle: caption(&trail3(lang, &yard.en, &yard.zh, &yard.ko), count),
            key: format!("bay:{}", yard.code),
            icon: yard.icon.clone(),
            solo: false,
            aside: String::new(),
            badge: None,
            chip: String::new(),
        }
    }

    pub(in crate::db) fn work_building_goods(&self, building: &str, lang: &str) -> Vec<Hit> {
        let Some(row) = self.works.iter().find(|row| row.code == building) else {
            return Vec::new();
        };
        let mut hits = Vec::new();
        for facility in &row.facilities {
            hits.extend(self.work_products(facility, "", "", lang));
        }
        hits
    }

    pub(in crate::db) fn work_products(&self, facility: &str, query: &str, folded: &str, lang: &str) -> Vec<Hit> {
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

    pub(in crate::db) fn work_product_hit(&self, product: &WorkProduct, lang: &str) -> Option<Hit> {
        let item = self.find_item(product.item)?;
        let (title, alt) = (lead3(lang, &item.en, &item.zh, &item.ko), trail3(lang, &item.en, &item.zh, &item.ko));
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

    pub fn work_building_name(&self, code: &str, lang: &str) -> String {
        self.works
            .iter()
            .find(|row| row.code == code)
            .map(|row| lead3(lang, &row.en, &row.zh, &row.ko))
            .unwrap_or_else(|| code.to_string())
    }

    pub fn work_facility_name(&self, code: &str, lang: &str) -> String {
        self.yards
            .get(code)
            .map(|row| lead3(lang, &row.en, &row.zh, &row.ko))
            .unwrap_or_else(|| code.to_string())
    }

    pub fn work_parent(&self, facility: &str) -> String {
        self.works
            .iter()
            .find(|row| row.facilities.iter().any(|code| code == facility))
            .map(|row| row.code.clone())
            .unwrap_or_default()
    }
}
