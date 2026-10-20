//! Direct acquisition costs for characters. Prerequisite characters are priced
//! as complete inputs; their older forms are never silently charged again.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::hero::{fixed_beast, hero_names, hero_rank_order, hero_rank_pair, tidy};
use super::text::lead;
use super::user_data::{preferences_dir, write_preference_checked};
use super::{ui_text, Catalog};

const VERSION: u32 = 1;
const MAX_INPUTS: usize = 512;
const MAX_JSON: usize = 256 * 1024;
const INVALID: &str = "角色計算資料無效 / Invalid character calculation";
const OVERFLOW: &str = "數量或成本過大 / Quantity or cost exceeds the supported range";

pub struct CharacterCalcCandidate {
    pub code: String,
    pub title: String,
    pub subtitle: String,
    pub route_count: usize,
    pub icon: Option<(PathBuf, i32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CharacterCalcRoute {
    pub id: String,
    pub title: String,
    pub conditions: String,
    pub inputs: Vec<CharacterCalcInput>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterCalcInput {
    /// Typed keys keep character codes separate from numerical item codes.
    pub key: String,
    /// hero, item, fee, or choice (a recorded alternative-material group).
    pub kind: String,
    pub title: String,
    pub per_unit: i64,
    /// None means unknown; zero must be an explicit user estimate.
    pub unit_price: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterCalcDocument {
    pub version: u32,
    pub language: String,
    pub hero_code: String,
    pub title: String,
    pub route_id: String,
    pub route_title: String,
    pub conditions: String,
    pub quantity: i64,
    pub inputs: Vec<CharacterCalcInput>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CharacterCalcLine {
    pub key: String,
    pub quantity: i64,
    pub unit_price: Option<i64>,
    pub line_total: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CharacterCalcBreakdown {
    /// All entered character/material/choice costs, excluding fee rows.
    pub material_total: i64,
    /// Entered fees only. Unknown fees keep total absent.
    pub fee: i64,
    pub known_total: i64,
    pub total: Option<i64>,
    pub priced: usize,
    pub inputs: Vec<CharacterCalcLine>,
}

fn safe_text(value: &str, maximum: usize, empty: bool) -> bool {
    value.len() <= maximum
        && (empty || !value.trim().is_empty())
        && !value.chars().any(|ch| ch.is_control() && ch != '\n')
}

fn safe_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b':' | b'-' | b'_'))
}

impl CharacterCalcDocument {
    pub fn calculate(&self) -> Result<CharacterCalcBreakdown, String> {
        if self.version != VERSION
            || self.quantity <= 0
            || !safe_code(&self.hero_code)
            || !safe_text(&self.language, 32, false)
            || !safe_text(&self.title, 1024, false)
            || !safe_text(&self.route_id, 1024, false)
            || !safe_text(&self.route_title, 1024, false)
            || !safe_text(&self.conditions, 32 * 1024, true)
            || self.inputs.is_empty()
            || self.inputs.len() > MAX_INPUTS
        {
            return Err(INVALID.into());
        }
        let mut keys = BTreeSet::new();
        let mut result = CharacterCalcBreakdown {
            material_total: 0,
            fee: 0,
            known_total: 0,
            total: None,
            priced: 0,
            inputs: Vec::with_capacity(self.inputs.len()),
        };
        for input in &self.inputs {
            let valid_key = input.key.split_once(':').is_some_and(|(kind, code)| {
                kind == input.kind
                    && match kind {
                        "item" => code.parse::<i64>().is_ok_and(|code| code > 0),
                        "hero" | "fee" | "choice" => safe_code(code),
                        _ => false,
                    }
            });
            if !valid_key
                || !keys.insert(&input.key)
                || input.per_unit <= 0
                || input.unit_price.is_some_and(|price| price < 0)
                || !safe_text(&input.title, 1024, false)
            {
                return Err(INVALID.into());
            }
            let quantity = input.per_unit.checked_mul(self.quantity).ok_or(OVERFLOW)?;
            let line_total = input
                .unit_price
                .map(|price| quantity.checked_mul(price).ok_or(OVERFLOW))
                .transpose()?;
            if let Some(total) = line_total {
                result.priced += 1;
                result.known_total = result.known_total.checked_add(total).ok_or(OVERFLOW)?;
                if input.kind == "fee" {
                    result.fee = result.fee.checked_add(total).ok_or(OVERFLOW)?;
                } else {
                    result.material_total =
                        result.material_total.checked_add(total).ok_or(OVERFLOW)?;
                }
            }
            result.inputs.push(CharacterCalcLine {
                key: input.key.clone(),
                quantity,
                unit_price: input.unit_price,
                line_total,
            });
        }
        if result.priced == self.inputs.len() {
            result.total = Some(result.known_total);
        }
        Ok(result)
    }
}

fn input(key: String, kind: &str, title: String, per_unit: i64) -> CharacterCalcInput {
    CharacterCalcInput {
        key,
        kind: kind.into(),
        title,
        per_unit,
        unit_price: None,
    }
}

fn add_condition(conditions: &mut String, note: &str) {
    let note = tidy(note);
    if !note.is_empty() && !conditions.contains(&note) {
        if !conditions.is_empty() {
            conditions.push('\n');
        }
        conditions.push_str(&note);
    }
}

fn finish_route(route: &mut CharacterCalcRoute, fee: Option<i64>, lang: &str) {
    route.inputs.push(CharacterCalcInput {
        key: "fee:acquisition".into(),
        kind: "fee".into(),
        per_unit: 1,
        unit_price: fee,
        title: ui_text(lang, "合成／轉職費用", "Combination / promotion fee"),
    });
    if fee.is_none() {
        add_condition(&mut route.conditions, &ui_text(lang,
            "請確認合成／轉職費用並填入；確認沒有費用時才填 0。",
            "Confirm and enter the combination or promotion fee; use 0 only when you know there is no fee."));
    }
}

impl Catalog {
    fn character_calc_name(&self, code: &str, lang: &str) -> String {
        self.index
            .heroes
            .iter()
            .find(|hero| hero.code == code)
            .map(|hero| hero_names(lang, hero).0)
            .unwrap_or_else(|| code.into())
    }

    fn character_calc_parts(
        &self,
        parts: &[String],
        materials: &[(i64, i64)],
        lang: &str,
    ) -> Vec<CharacterCalcInput> {
        // Repeated inputs are charged by their actual quantity, not by the
        // number of distinct names. Invalid/overflowing quantities survive here
        // as an invalid document rather than producing a plausible low total.
        let mut quantities = BTreeMap::<String, (String, i64)>::new();
        for part in parts {
            let value = quantities
                .entry(format!("hero:{part}"))
                .or_insert_with(|| (self.character_calc_name(part, lang), 0));
            value.1 = value.1.checked_add(1).unwrap_or(-1);
        }
        for (code, qty) in materials {
            let value = quantities
                .entry(format!("item:{code}"))
                .or_insert_with(|| (self.item_name(*code, lang), 0));
            value.1 = if *qty <= 0 || value.1 < 0 {
                -1
            } else {
                value.1.checked_add(*qty).unwrap_or(-1)
            };
        }
        quantities
            .into_iter()
            .map(|(key, (title, qty))| {
                let kind = if key.starts_with("hero:") {
                    "hero"
                } else {
                    "item"
                };
                input(key, kind, title, qty)
            })
            .collect()
    }

    pub fn character_calculator_candidates(
        &self,
        query: &str,
        lang: &str,
    ) -> Vec<CharacterCalcCandidate> {
        let query = query.trim();
        let folded = query.to_lowercase();
        self.index.prepare_aliases(&folded);
        let mut heroes: Vec<_> = self
            .index
            .heroes
            .iter()
            .filter(|hero| {
                query.is_empty() || self.index.name_hit(query, &folded, &hero.code, &hero.names)
            })
            .collect();
        heroes.sort_by_key(|hero| (hero_rank_order(hero.rank), hero.code.as_str()));
        heroes
            .into_iter()
            .filter_map(|hero| {
                let route_count = self.character_calculator_routes(&hero.code, lang).len();
                if route_count == 0 {
                    return None;
                }
                let (en, zh) = hero_rank_pair(hero.rank);
                Some(CharacterCalcCandidate {
                    code: hero.code.clone(),
                    title: hero_names(lang, hero).0,
                    subtitle: format!("{} · {}", ui_text(lang, zh, en), hero.code),
                    route_count,
                    icon: hero.icon.clone(),
                })
            })
            .take(60)
            .collect()
    }

    pub fn character_calculator_routes(&self, code: &str, lang: &str) -> Vec<CharacterCalcRoute> {
        let Some(hero) = self.index.heroes.iter().find(|hero| hero.code == code) else {
            return Vec::new();
        };
        let hire = tidy(&lead(lang, &hero.hire_en, &hero.hire_zh));
        let mut routes = Vec::new();
        if let Some(combos) = self.index.combos.get(code) {
            let mut combos: Vec<_> = combos
                .iter()
                .filter(|recipe| recipe.result == code)
                .collect();
            combos.sort_by(|a, b| a.parts.cmp(&b.parts).then(a.materials.cmp(&b.materials)));
            combos.dedup_by(|a, b| a.parts == b.parts && a.materials == b.materials);
            for (index, recipe) in combos.into_iter().enumerate() {
                let names = recipe
                    .parts
                    .iter()
                    .map(|part| self.character_calc_name(part, lang))
                    .collect::<Vec<_>>();
                let mut route = CharacterCalcRoute {
                    id: format!("combination:{index}"),
                    title: format!(
                        "{} · {}",
                        ui_text(lang, "合成", "Combination"),
                        names.join(" + ")
                    ),
                    conditions: tidy(&lead(lang, &recipe.en, &recipe.zh)),
                    inputs: self.character_calc_parts(&recipe.parts, &recipe.materials, lang),
                };
                add_condition(&mut route.conditions, &hire);
                finish_route(&mut route, None, lang);
                routes.push(route);
            }
        }
        if let Some(steps) = self.index.chains.get(code) {
            for pair in steps.windows(2).filter(|pair| pair[1].code == code) {
                let source = &pair[0];
                let mut route = CharacterCalcRoute {
                    id: format!("upgrade:{}", source.code),
                    title: format!(
                        "{} · {}",
                        ui_text(lang, "轉職／覺醒", "Promotion / awakening"),
                        self.character_calc_name(&source.code, lang)
                    ),
                    conditions: tidy(&lead(lang, &source.en, &source.zh)),
                    inputs: self.character_calc_parts(
                        &[source.code.clone()],
                        &source.materials,
                        lang,
                    ),
                };
                add_condition(&mut route.conditions, &hire);
                finish_route(&mut route, None, lang);
                routes.push(route);
            }
        }
        if let Some(parts) = fixed_beast(code) {
            let mut route = CharacterCalcRoute {
                id: "beast".into(),
                title: ui_text(lang, "神獸合成", "Divine beast combination"),
                conditions: hire.clone(),
                inputs: self.character_calc_parts(
                    &[],
                    &parts
                        .iter()
                        .filter(|part| part.item > 0 && part.qty > 0)
                        .map(|part| (part.item, part.qty))
                        .collect::<Vec<_>>(),
                    lang,
                ),
            };
            let alternatives = parts
                .iter()
                .filter(|part| part.item > 0 && part.qty == 0)
                .map(|part| self.item_name(part.item, lang))
                .collect::<Vec<_>>();
            if !alternatives.is_empty() {
                // The source's zero quantities identify alternatives. Charging
                // all four (or treating them as free) would both be incorrect.
                let count = parts
                    .iter()
                    .find(|part| part.item == 0)
                    .map(|part| part.qty)
                    .unwrap_or(3);
                route.inputs.push(input(
                    "choice:statues".into(),
                    "choice",
                    ui_text(
                        lang,
                        &format!("{count} 種不同幻獸石像的合計成本"),
                        &format!("Combined cost of {count} different fey statues"),
                    ),
                    1,
                ));
                add_condition(&mut route.conditions, &ui_text(lang,
                    &format!("石像選項（選 {count} 種不同石像，輸入合計）：{}", alternatives.join("、")),
                    &format!("Statue choices (choose {count} different statues and enter their combined cost): {}", alternatives.join(", "))));
            }
            let credit = self.index.beast_credit(code);
            // The original recruitment text is more specific than the older
            // curated fallback (some beasts say 100 there, but 95 in the
            // fallback). Do not display two contradictory credit conditions.
            let recorded_credit =
                hero.hire_en.to_lowercase().contains("credit") || hero.hire_zh.contains("信用");
            if credit > 0 && !recorded_credit {
                add_condition(
                    &mut route.conditions,
                    &ui_text(
                        lang,
                        &format!("所需信用等級：{credit}"),
                        &format!("Required credit level: {credit}"),
                    ),
                );
            }
            finish_route(&mut route, None, lang);
            routes.push(route);
        }
        if hero.rank == 14 {
            routes.extend(self.character_calc_spirit_routes(code, lang));
        } else if hero.rank == 13 {
            if let Some(route) = self.character_calc_mount_route(code, lang) {
                routes.push(route);
            }
        }
        if routes.is_empty() && !hire.is_empty() && hero.rank != 14 {
            // A recruitment description is useful even when no structured
            // recipe exists. One explicit all-in estimate avoids claiming an
            // empty materials list is a free recruitment recipe.
            routes.push(CharacterCalcRoute {
                id: "recruitment".into(), title: ui_text(lang, "招募／取得估算", "Recruitment / acquisition estimate"),
                conditions: format!("{}\n{}", hire, ui_text(lang,
                    "此角色尚無完整配方，請填入整體取得成本（包括相關費用）。",
                    "A complete recipe is not available. Enter this character's full acquisition cost, including any fees.")),
                inputs: vec![input(format!("hero:{code}"), "hero", ui_text(lang,
                    "角色整體取得成本", "Complete character acquisition cost"), 1)],
            });
        }
        routes
    }

    fn character_calc_materials(&self, code: &str, kind: &str) -> Vec<(i64, i64)> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT item_code, qty FROM character_materials WHERE code=?1 AND kind=?2 ORDER BY slot",
        ) else { return Vec::new() };
        stmt.query_map([code, kind], |row| Ok((row.get(0)?, row.get(1)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    fn character_calc_facts(&self, code: &str, label: &str, lang: &str) -> Vec<String> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT value_en,value_zh FROM character_facts WHERE code=?1 AND label_en=?2 ORDER BY slot",
        ) else { return Vec::new() };
        stmt.query_map([code, label], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map(|rows| {
            rows.flatten()
                .map(|(en, zh)| lead(lang, &en, &zh))
                .collect()
        })
        .unwrap_or_default()
    }

    fn character_calc_spirit_routes(&self, code: &str, lang: &str) -> Vec<CharacterCalcRoute> {
        // PetInfo owns evolution materials on the SOURCE form. Joining both
        // ranks prevents ordinary upgrade/mount records being reversed here.
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT u.code FROM character_upgrades u
             JOIN characters s ON s.code=u.code AND s.rank=14
             JOIN characters t ON t.code=u.target_code AND t.rank=14
             WHERE u.target_code=?1 ORDER BY u.code",
        ) else {
            return Vec::new();
        };
        let sources: Vec<String> = stmt
            .query_map([code], |row| row.get(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default();
        sources
            .into_iter()
            .map(|source| {
                let mut route = CharacterCalcRoute {
                    id: format!("evolution:{source}"),
                    title: format!(
                        "{} · {}",
                        ui_text(lang, "精靈進化", "Spirit evolution"),
                        self.character_calc_name(&source, lang)
                    ),
                    conditions: String::new(),
                    inputs: self.character_calc_parts(
                        &[source.clone()],
                        &self.character_calc_materials(&source, "evolution"),
                        lang,
                    ),
                };
                for intimacy in self.character_calc_facts(&source, "Evolution intimacy", lang) {
                    add_condition(
                        &mut route.conditions,
                        &format!(
                            "{}：{intimacy}",
                            ui_text(lang, "所需親密度", "Required intimacy")
                        ),
                    );
                }
                finish_route(&mut route, None, lang);
                route
            })
            .collect()
    }

    fn character_calc_mount_route(&self, code: &str, lang: &str) -> Option<CharacterCalcRoute> {
        let materials = self.character_calc_materials(code, "acquisition");
        let required: Vec<String> = self.conn.prepare(
            "SELECT target_code FROM character_requirements WHERE code=?1 AND kind='acquisition' ORDER BY target_code",
        ).ok().and_then(|mut stmt| stmt.query_map([code], |row| row.get(0))
            .ok().map(|rows| rows.flatten().collect())).unwrap_or_default();
        let fees = self.character_calc_facts(code, "Acquisition fee", "en");
        if materials.is_empty() && required.is_empty() && fees.is_empty() {
            return None;
        }
        let fee = fees
            .first()
            .and_then(|value| value.replace(',', "").parse::<i64>().ok())
            .filter(|value| *value >= 0);
        let mut route = CharacterCalcRoute {
            id: "acquisition".into(),
            title: ui_text(lang, "坐騎購買／改良", "Mount purchase / improvement"),
            conditions: String::new(),
            inputs: self.character_calc_parts(&required, &materials, lang),
        };
        for fact in self.character_calc_facts(code, "Acquisition", lang) {
            add_condition(&mut route.conditions, &fact);
        }
        for level in self.character_calc_facts(code, "Required riding level", lang) {
            add_condition(
                &mut route.conditions,
                &format!(
                    "{}：{level}",
                    ui_text(lang, "所需馬術等級", "Required riding level")
                ),
            );
        }
        add_condition(
            &mut route.conditions,
            &ui_text(
                lang,
                "馬術訓練費用及材料另計。",
                "Riding training fees and materials are separate.",
            ),
        );
        finish_route(&mut route, fee, lang);
        Some(route)
    }

    pub fn character_calculator_document(
        &self,
        code: &str,
        route_id: &str,
        lang: &str,
    ) -> Result<CharacterCalcDocument, String> {
        let route = self
            .character_calculator_routes(code, lang)
            .into_iter()
            .find(|route| route.id == route_id)
            .ok_or_else(|| {
                ui_text(
                    lang,
                    "找不到角色取得路線。",
                    "Character acquisition route not found.",
                )
            })?;
        let doc = CharacterCalcDocument {
            version: VERSION,
            language: super::normalize_lang(lang),
            hero_code: code.into(),
            title: self.character_calc_name(code, lang),
            route_id: route.id,
            route_title: route.title,
            conditions: route.conditions,
            quantity: 1,
            inputs: route.inputs,
        };
        doc.calculate()?;
        Ok(doc)
    }

    pub fn character_calculator_icon(&self, key: &str) -> Option<(PathBuf, i32)> {
        match key.split_once(':')? {
            ("hero", code) => self.hero_portrait(code),
            ("item", code) => self.item_icon(code.parse().ok()?),
            _ => None,
        }
    }

    pub fn supplement_character_calculator_materials(
        &self,
        document: &CharacterCalcDocument,
    ) -> Result<CharacterCalcDocument, String> {
        document.calculate()?;
        let mut updated = document.clone();
        let Some(route) = self
            .character_calculator_routes(&document.hero_code, &document.language)
            .into_iter()
            .find(|route| route.id == document.route_id)
        else {
            return Ok(updated);
        };
        // Repair additive material omissions only. A changed prerequisite or
        // quantity could identify a different historical route; retain that draft.
        let same_input = |a: &CharacterCalcInput, b: &CharacterCalcInput| {
            a.key == b.key && a.kind == b.kind && a.per_unit == b.per_unit
        };
        if !document
            .inputs
            .iter()
            .all(|old| route.inputs.iter().any(|current| same_input(old, current)))
            || !route
                .inputs
                .iter()
                .filter(|current| current.kind != "item")
                .all(|current| document.inputs.iter().any(|old| same_input(old, current)))
        {
            return Ok(updated);
        }
        let additions = route.inputs.into_iter().filter(|current| {
            current.kind == "item"
                && !document.inputs.iter().any(|old| old.key == current.key)
        });
        let before_fee = updated
            .inputs
            .iter()
            .position(|input| input.kind == "fee")
            .unwrap_or(updated.inputs.len());
        updated.inputs.splice(before_fee..before_fee, additions);
        updated.calculate()?;
        Ok(updated)
    }

    pub fn localized_character_calculator_document(
        &self,
        document: &CharacterCalcDocument,
        lang: &str,
    ) -> CharacterCalcDocument {
        let mut translated = document.clone();
        translated.language = super::normalize_lang(lang);
        if self
            .index
            .heroes
            .iter()
            .any(|hero| hero.code == document.hero_code)
        {
            translated.title = self.character_calc_name(&document.hero_code, lang);
        }
        if let Some(route) = self
            .character_calculator_routes(&document.hero_code, lang)
            .into_iter()
            .find(|route| route.id == document.route_id)
        {
            translated.route_title = route.title;
            translated.conditions = route.conditions;
            for input in &mut translated.inputs {
                if let Some(current) = route.inputs.iter().find(|row| row.key == input.key) {
                    input.title = current.title.clone();
                }
            }
        }
        translated
    }
}

fn load_from(path: &Path) -> Result<Option<CharacterCalcDocument>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "無法讀取角色計算 / Cannot read character calculation: {error}"
            ))
        }
    };
    let mut body = Vec::new();
    file.take(MAX_JSON as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    let parse = || -> Result<CharacterCalcDocument, String> {
        if body.len() > MAX_JSON {
            return Err(INVALID.into());
        }
        let document: CharacterCalcDocument =
            serde_json::from_slice(&body).map_err(|error| error.to_string())?;
        document.calculate()?;
        Ok(document)
    };
    parse().map(Some).map_err(|error| {
        format!(
            "{error}；原有檔案已保留 / Original file preserved: {}",
            path.display()
        )
    })
}

fn save_to(path: &Path, document: &CharacterCalcDocument) -> Result<(), String> {
    document.calculate()?;
    let body = serde_json::to_vec_pretty(document).map_err(|error| error.to_string())?;
    if body.len() > MAX_JSON {
        return Err(INVALID.into());
    }
    // Match the item calculator: do not overwrite corrupt or future-version state.
    load_from(path)?;
    write_preference_checked(path, body)
        .map_err(|error| format!("無法儲存角色計算 / Cannot save character calculation: {error}"))
}

pub fn load_character_calculator() -> Result<Option<CharacterCalcDocument>, String> {
    load_from(&preferences_dir().join("character-calculator.json"))
}

pub fn save_character_calculator(document: &CharacterCalcDocument) -> Result<(), String> {
    save_to(
        &preferences_dir().join("character-calculator.json"),
        document,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> CharacterCalcDocument {
        CharacterCalcDocument {
            version: VERSION,
            language: "zh".into(),
            hero_code: "T49".into(),
            title: "持國天王".into(),
            route_id: "combination:0".into(),
            route_title: "合成".into(),
            conditions: String::new(),
            quantity: 2,
            inputs: vec![
                input("hero:E85".into(), "hero", "麒麟".into(), 1),
                input("item:140".into(), "item", "材料".into(), 3),
                input("fee:acquisition".into(), "fee", "費用".into(), 1),
            ],
        }
    }

    #[test]
    fn unknown_character_material_and_fee_costs_never_become_zero() {
        let mut doc = sample();
        assert_eq!(doc.calculate().unwrap().total, None);
        doc.inputs[0].unit_price = Some(100);
        doc.inputs[1].unit_price = Some(20);
        let partial = doc.calculate().unwrap();
        assert_eq!(
            (
                partial.known_total,
                partial.material_total,
                partial.fee,
                partial.priced
            ),
            (320, 320, 0, 2)
        );
        assert_eq!(partial.total, None);
        doc.inputs[2].unit_price = Some(10);
        let full = doc.calculate().unwrap();
        assert_eq!((full.total, full.fee), (Some(340), 20));
        doc.inputs[2].unit_price = Some(0);
        assert_eq!(doc.calculate().unwrap().total, Some(320));
    }

    #[test]
    fn rejects_negative_values_duplicates_and_invalid_typed_keys() {
        let doc = sample();
        let mut invalid = doc.clone();
        invalid.quantity = 0;
        assert!(invalid.calculate().is_err());
        let mut invalid = doc.clone();
        invalid.inputs[0].per_unit = -1;
        assert!(invalid.calculate().is_err());
        let mut invalid = doc.clone();
        invalid.inputs[0].unit_price = Some(-1);
        assert!(invalid.calculate().is_err());
        let mut invalid = doc.clone();
        invalid.inputs.push(invalid.inputs[0].clone());
        assert!(invalid.calculate().is_err());
        let mut invalid = doc.clone();
        invalid.inputs[0].key = "item:E85".into();
        assert!(invalid.calculate().is_err());
        let mut invalid = doc.clone();
        invalid.version = 2;
        assert!(invalid.calculate().is_err());
        let mut invalid = doc;
        invalid.inputs.clear();
        assert!(invalid.calculate().is_err());
    }

    #[test]
    fn checks_quantity_price_and_total_overflow() {
        let mut doc = sample();
        doc.quantity = i64::MAX;
        assert!(doc.calculate().is_err());
        doc.quantity = 2;
        doc.inputs[0].unit_price = Some(i64::MAX);
        assert!(doc.calculate().is_err());
        doc.quantity = 1;
        doc.inputs[1].unit_price = Some(1);
        assert!(doc.calculate().is_err());
    }

    #[test]
    fn bundled_routes_charge_only_incoming_prerequisites_and_preserve_levels() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let routes = catalog.character_calculator_routes("T49", "zh");
        let route = routes
            .iter()
            .find(|route| route.id.starts_with("combination:"))
            .unwrap();
        let heroes: Vec<_> = route
            .inputs
            .iter()
            .filter(|row| row.kind == "hero")
            .map(|row| row.key.as_str())
            .collect();
        assert_eq!(heroes, ["hero:E75", "hero:E85"]);
        assert!(route.inputs.iter().any(|row| row.kind == "item"));
        assert!(!route.inputs.iter().any(|row| row.key == "hero:V06"));
        assert_eq!(
            route
                .inputs
                .iter()
                .find(|row| row.kind == "fee")
                .unwrap()
                .unit_price,
            None
        );
        let awakened = catalog.character_calculator_routes("V06", "zh");
        assert!(awakened
            .iter()
            .any(|route| route.conditions.contains("260")));
        for hero in &catalog.index.heroes {
            for route in catalog.character_calculator_routes(&hero.code, "zh") {
                catalog
                    .character_calculator_document(&hero.code, &route.id, "zh")
                    .unwrap_or_else(|error| panic!("{} / {}: {error}", hero.code, route.id));
            }
        }
    }

    #[test]
    fn legendary_materials_and_beast_alternatives_are_not_lost() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let route = catalog.character_calculator_routes("T80", "en").remove(0);
        assert_eq!(
            route.inputs.iter().filter(|row| row.kind == "hero").count(),
            4
        );
        assert!(route
            .inputs
            .iter()
            .any(|row| row.key == "item:6831" && row.per_unit == 4));
        let beast = catalog
            .character_calculator_routes("E74", "en")
            .into_iter()
            .find(|route| route.id == "beast")
            .unwrap();
        assert_eq!(
            beast
                .inputs
                .iter()
                .filter(|row| row.kind == "choice")
                .count(),
            1
        );
        assert!(!beast.inputs.iter().any(|row| row.key == "item:3817"));
        assert!(beast.conditions.contains("100"));
        assert!(!beast.conditions.contains("Required credit level: 95"));
    }

    #[test]
    fn spirit_costs_use_previous_form_materials_and_mount_fee_is_recorded() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let pairs: Vec<(String, String)> = catalog.conn.prepare(
            "SELECT u.code,u.target_code FROM character_upgrades u JOIN characters c ON c.code=u.code AND c.rank=14 ORDER BY u.code",
        ).unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().flatten().collect();
        assert!(!pairs.is_empty());
        for (source, target) in pairs {
            let route = catalog
                .character_calculator_routes(&target, "en")
                .into_iter()
                .find(|route| route.id == format!("evolution:{source}"))
                .unwrap();
            assert!(route
                .inputs
                .iter()
                .any(|row| row.key == format!("hero:{source}")));
            for (item, qty) in catalog.character_calc_materials(&source, "evolution") {
                assert!(route
                    .inputs
                    .iter()
                    .any(|row| row.key == format!("item:{item}") && row.per_unit == qty));
            }
        }
        let mount = catalog
            .character_calculator_routes("mount:2", "en")
            .into_iter()
            .find(|route| route.id == "acquisition")
            .unwrap();
        assert_eq!(
            mount
                .inputs
                .iter()
                .find(|row| row.kind == "fee")
                .unwrap()
                .unit_price,
            Some(10_000_000)
        );
    }

    #[test]
    fn saved_king_estimates_gain_missing_tablets_without_losing_prices() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for (code, missing_key) in [("T50", "item:5750"), ("T51", "item:5748")] {
            let mut document = catalog
                .character_calculator_document(code, "combination:0", "zh")
                .unwrap();
            document.quantity = 3;
            document.inputs.retain(|input| input.key != missing_key);
            for (index, input) in document.inputs.iter_mut().enumerate() {
                input.unit_price = Some(index as i64 * 1_000);
            }
            let previous_cost = document.calculate().unwrap().total.unwrap();
            let updated = catalog
                .supplement_character_calculator_materials(&document)
                .unwrap();
            assert_eq!(updated.quantity, 3);
            for previous in &document.inputs {
                assert!(updated.inputs.contains(previous));
            }
            let added = updated.inputs.iter().find(|row| row.key == missing_key).unwrap();
            assert_eq!((added.per_unit, added.unit_price), (20, None));
            assert_eq!(updated.inputs.len(), document.inputs.len() + 1);
            let cost = updated.calculate().unwrap();
            assert_eq!(cost.known_total, previous_cost);
            assert_eq!(cost.total, None);
            assert_eq!(
                catalog.supplement_character_calculator_materials(&updated).unwrap(),
                updated
            );

            let mut historical = document.clone();
            historical.route_id = "historical-route".into();
            assert_eq!(
                catalog.supplement_character_calculator_materials(&historical).unwrap(),
                historical
            );
            let mut changed_quantity = document.clone();
            changed_quantity.inputs[0].per_unit += 1;
            assert_eq!(
                catalog.supplement_character_calculator_materials(&changed_quantity).unwrap(),
                changed_quantity
            );
            let mut changed_prerequisites = document;
            changed_prerequisites.inputs.retain(|input| input.kind != "hero");
            assert_eq!(
                catalog.supplement_character_calculator_materials(&changed_prerequisites).unwrap(),
                changed_prerequisites
            );
        }
    }

    #[test]
    fn search_localization_and_save_preserve_independent_estimates() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let candidates = catalog.character_calculator_candidates("持國天王", "zh");
        assert!(candidates.iter().any(|row| row.code == "T49"));
        let mut document = catalog
            .character_calculator_document("T49", "combination:0", "zh")
            .unwrap();
        document.quantity = 3;
        document.inputs[0].unit_price = Some(123);
        let localized = catalog.localized_character_calculator_document(&document, "en");
        assert_eq!(localized.quantity, 3);
        assert_eq!(localized.inputs[0].unit_price, Some(123));
        assert_ne!(localized.title, document.title);
        save_character_calculator(&document).unwrap();
        assert_eq!(load_character_calculator().unwrap(), Some(document.clone()));
        let path = preferences_dir().join("character-calculator.json");
        std::fs::write(&path, br#"{"version":999}"#).unwrap();
        assert!(save_character_calculator(&document).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), r#"{"version":999}"#);
    }
}
