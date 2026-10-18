//! Direct recipe costing and portable, self-contained offline calculation documents.
//! Prices are user input; no catalogue prices, labels, or recipes are changed.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::planner::{ItemKey, PlannerCandidate, PlannerCatalog};
use super::user_data::{preferences_dir, write_preference_checked};
use super::Catalog;

const VERSION: u32 = 1;
const PREFIX: &str = "GS-C1";
const MAX_PACKED: usize = 48 * 1024;
const MAX_JSON: usize = 256 * 1024;
const MAX_CODE: usize = MAX_PACKED * 4 / 3 + 80;
const MAX_MATERIALS: usize = 128;
const OVERFLOW: &str = "數量或成本過大 / Quantity or cost exceeds the supported range";

pub struct CalcCatalog {
    planner: PlannerCatalog,
    by_item: BTreeMap<i64, Vec<i64>>,
    catalog_version: String,
}

pub struct CalcItem {
    pub code: i64,
    pub title: String,
    pub recipe_count: usize,
}

/// A portable recipe snapshot. Quantities and prices remain editable without
/// access to the originating database. Computed totals are never trusted input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalcDocument {
    pub version: u32,
    pub catalog_version: String,
    pub language: String,
    pub recipe_id: i64,
    pub item_code: i64,
    pub title: String,
    pub source: String,
    pub output: i64,
    pub fee_per_batch: i64,
    pub learn_fee: Option<i64>,
    pub success: Option<i64>,
    pub quantity: i64,
    pub materials: Vec<CalcIngredient>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalcIngredient {
    pub code: i64,
    pub enhance: i64,
    pub title: String,
    pub per_batch: i64,
    /// None is unknown. Some(0) is an explicit zero-cost input.
    pub unit_price: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CalcLine {
    pub key: ItemKey,
    pub title: String,
    pub quantity: i64,
    pub unit_price: Option<i64>,
    pub line_total: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CalcBreakdown {
    pub batches: i64,
    pub output_units: i64,
    pub fee: i64,
    /// Sum of entered material costs, excluding all unpriced rows.
    pub material_total: i64,
    /// Entered material costs plus crafting fees. This can be incomplete.
    pub known_total: i64,
    /// Present only after every material has an explicit unit price.
    pub total: Option<i64>,
    pub priced: usize,
    pub materials: Vec<CalcLine>,
}

fn valid_text(value: &str, limit: usize, allow_empty: bool) -> bool {
    value.len() <= limit
        && (allow_empty || !value.trim().is_empty())
        && !value.chars().any(char::is_control)
}

impl CalcDocument {
    pub fn calculate(&self) -> Result<CalcBreakdown, String> {
        if self.version != VERSION {
            return Err(format!(
                "不支援的計算版本 {} / Unsupported calculation version",
                self.version
            ));
        }
        if !valid_text(&self.catalog_version, 64, true)
            || !valid_text(&self.language, 32, false)
            || !valid_text(&self.title, 1024, false)
            || !valid_text(&self.source, 256, false)
            || self.recipe_id <= 0
            || self.item_code <= 0
            || self.quantity <= 0
            || self.output <= 0
            || self.fee_per_batch < 0
            || self.learn_fee.is_some_and(|fee| fee < 0)
            || self.success.is_some_and(|rate| !(1..=100).contains(&rate))
            || self.materials.is_empty()
            || self.materials.len() > MAX_MATERIALS
        {
            return Err("配方資料或數量無效 / Invalid recipe data or quantity".into());
        }
        let batches = self.quantity / self.output + i64::from(self.quantity % self.output != 0);
        let output_units = self.output.checked_mul(batches).ok_or(OVERFLOW)?;
        let fee = self.fee_per_batch.checked_mul(batches).ok_or(OVERFLOW)?;
        let mut keys = BTreeSet::new();
        let mut material_total = 0_i64;
        let mut priced = 0;
        let mut materials = Vec::with_capacity(self.materials.len());
        for material in &self.materials {
            let key = ItemKey {
                code: material.code,
                enhance: material.enhance,
            };
            if key.code <= 0
                || key.enhance < 0
                || !keys.insert(key)
                || material.per_batch <= 0
                || !valid_text(&material.title, 1024, false)
                || material.unit_price.is_some_and(|price| price < 0)
            {
                return Err("材料資料重複或無效 / Duplicate or invalid material data".into());
            }
            let quantity = material.per_batch.checked_mul(batches).ok_or(OVERFLOW)?;
            let line_total = material
                .unit_price
                .map(|price| price.checked_mul(quantity).ok_or(OVERFLOW))
                .transpose()?;
            if let Some(total) = line_total {
                priced += 1;
                material_total = material_total.checked_add(total).ok_or(OVERFLOW)?;
            }
            materials.push(CalcLine {
                key,
                title: material.title.clone(),
                quantity,
                unit_price: material.unit_price,
                line_total,
            });
        }
        let known_total = material_total.checked_add(fee).ok_or(OVERFLOW)?;
        Ok(CalcBreakdown {
            batches,
            output_units,
            fee,
            material_total,
            known_total,
            total: (priced == materials.len()).then_some(known_total),
            priced,
            materials,
        })
    }
}

impl Catalog {
    pub fn calculator_catalog(&self) -> Result<CalcCatalog, String> {
        let planner = self.planner_catalog()?;
        let mut by_item: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
        for recipe in &self.index.recipes {
            // Reinforcement output levels are absent from the DB, so those
            // records cannot describe a precise craftable result.
            if recipe.source != "reinforce" {
                if let Some(candidate) = self.planner_recipe(&planner, recipe.id, "zh-en") {
                    by_item.entry(candidate.code).or_default().push(recipe.id);
                }
            }
        }
        let catalog_version = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'catalog_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| error.to_string())?;
        Ok(CalcCatalog {
            planner,
            by_item,
            catalog_version,
        })
    }

    pub fn calculator_items(&self, book: &CalcCatalog, query: &str, lang: &str) -> Vec<CalcItem> {
        let query = query.trim();
        let folded = query.to_lowercase();
        self.index.prepare_aliases(&folded);
        let mut rows: Vec<_> = self
            .index
            .recipes
            .iter()
            .filter(|recipe| {
                let Ok(code) = recipe.result_str.parse::<i64>() else {
                    return false;
                };
                book.by_item
                    .get(&code)
                    .is_some_and(|ids| ids.contains(&recipe.id))
                    && (query.is_empty()
                        || self
                            .index
                            .name_hit(query, &folded, &recipe.result_str, &recipe.names))
            })
            .collect();
        rows.sort_by_key(|recipe| (recipe.level_min.unwrap_or(i64::MAX), recipe.id));
        let mut seen = BTreeSet::new();
        rows.into_iter()
            .filter_map(|recipe| {
                let code = recipe.result_str.parse().ok()?;
                if !seen.insert(code) {
                    return None;
                }
                Some(CalcItem {
                    code,
                    title: self.item_name(code, lang),
                    recipe_count: book.by_item[&code].len(),
                })
            })
            .take(40)
            .collect()
    }

    pub fn calculator_recipes(
        &self,
        book: &CalcCatalog,
        code: i64,
        lang: &str,
    ) -> Vec<PlannerCandidate> {
        book.by_item
            .get(&code)
            .into_iter()
            .flatten()
            .filter_map(|id| self.planner_recipe(&book.planner, *id, lang))
            .collect()
    }

    pub fn calculator_icon(&self, code: i64) -> Option<(PathBuf, i32)> {
        self.item_icon(code)
    }

    pub fn calculator_document(
        &self,
        book: &CalcCatalog,
        recipe_id: i64,
        lang: &str,
    ) -> Result<CalcDocument, String> {
        let candidate = self
            .planner_recipe(&book.planner, recipe_id, lang)
            .filter(|candidate| {
                book.by_item
                    .get(&candidate.code)
                    .is_some_and(|ids| ids.contains(&recipe_id))
            })
            .ok_or("找不到完整的合成配方 / No complete crafting recipe found")?;
        let learn_fee = self
            .conn
            .query_row(
                "SELECT learn_fee FROM recipes WHERE id = ?1",
                [recipe_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let mut stmt = self.conn.prepare(
            "SELECT item_code, enhance, qty FROM recipe_materials WHERE recipe_id = ?1 ORDER BY slot"
        ).map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([recipe_id], |row| {
                Ok((
                    ItemKey {
                        code: row.get(0)?,
                        enhance: row.get(1)?,
                    },
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        let mut materials: Vec<CalcIngredient> = Vec::new();
        // Preserve recipe slot order while merging repeated material slots.
        for row in rows {
            let (key, qty) = row.map_err(|error| error.to_string())?;
            if let Some(existing) = materials
                .iter_mut()
                .find(|row| row.code == key.code && row.enhance == key.enhance)
            {
                existing.per_batch = existing.per_batch.checked_add(qty).ok_or(OVERFLOW)?;
            } else {
                let mut title = self.item_name(key.code, lang);
                if key.enhance > 0 {
                    title.push_str(&format!(" +{}", key.enhance));
                }
                materials.push(CalcIngredient {
                    code: key.code,
                    enhance: key.enhance,
                    title,
                    per_batch: qty,
                    unit_price: None,
                });
            }
        }
        let document = CalcDocument {
            version: VERSION,
            catalog_version: book.catalog_version.clone(),
            language: lang.into(),
            recipe_id,
            item_code: candidate.code,
            title: candidate.title,
            source: candidate.source,
            output: candidate.output,
            fee_per_batch: candidate.fee,
            learn_fee,
            success: candidate.success,
            quantity: 1,
            materials,
        };
        document.calculate()?;
        Ok(document)
    }
}

fn serialize(document: &CalcDocument) -> Result<Vec<u8>, String> {
    document.calculate()?;
    let body = serde_json::to_vec(document)
        .map_err(|error| format!("無法編碼計算 / Cannot encode calculation: {error}"))?;
    if body.len() > MAX_JSON {
        return Err("計算資料過大 / Calculation data is too large".into());
    }
    Ok(body)
}

fn deserialize(body: &[u8]) -> Result<CalcDocument, String> {
    if body.len() > MAX_JSON {
        return Err("計算資料過大 / Calculation data is too large".into());
    }
    #[derive(Deserialize)]
    struct Header {
        version: u32,
    }
    let header: Header =
        serde_json::from_slice(body).map_err(|_| "計算資料格式錯誤 / Invalid calculation data")?;
    if header.version != VERSION {
        return Err(format!(
            "不支援的計算版本 {} / Unsupported calculation version",
            header.version
        ));
    }
    let document: CalcDocument =
        serde_json::from_slice(body).map_err(|_| "計算資料格式錯誤 / Invalid calculation data")?;
    document.calculate()?;
    Ok(document)
}

pub(super) fn checksum(body: &[u8]) -> String {
    // Accidental corruption detection, not an authenticity signature.
    Sha256::digest(body)[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub(super) fn base64_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity((bytes.len() * 4).div_ceil(3));
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        encoded.push(ALPHABET[(a >> 2) as usize] as char);
        encoded.push(ALPHABET[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize] as char);
        }
        if chunk.len() > 2 {
            encoded.push(ALPHABET[(c & 63) as usize] as char);
        }
    }
    encoded
}

fn base64_decode(encoded: &str) -> Result<Vec<u8>, String> {
    base64_decode_limited(encoded, MAX_PACKED)
}

pub(super) fn base64_decode_limited(encoded: &str, max_packed: usize) -> Result<Vec<u8>, String> {
    let invalid = "分享碼格式錯誤 / Invalid share code";
    if encoded.is_empty() || encoded.len() % 4 == 1 || encoded.len() > max_packed * 4 / 3 + 2 {
        return Err(invalid.into());
    }
    let mut bytes = Vec::with_capacity(encoded.len() * 3 / 4);
    let mut accumulator = 0_u32;
    let mut bits = 0;
    for character in encoded.bytes() {
        let value = match character {
            b'A'..=b'Z' => character - b'A',
            b'a'..=b'z' => character - b'a' + 26,
            b'0'..=b'9' => character - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(invalid.into()),
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((accumulator >> bits) as u8);
            accumulator &= (1 << bits) - 1;
        }
    }
    // Require canonical unpadded base64url, including zero trailing bits.
    if accumulator != 0 || bytes.len() > max_packed {
        return Err(invalid.into());
    }
    Ok(bytes)
}

pub fn encode_calculator(document: &CalcDocument) -> Result<String, String> {
    let body = serialize(document)?;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&body)
        .map_err(|error| error.to_string())?;
    let packed = encoder.finish().map_err(|error| error.to_string())?;
    if packed.len() > MAX_PACKED {
        return Err("計算資料過大 / Calculation data is too large".into());
    }
    Ok(format!(
        "{PREFIX}.{}.{}",
        base64_encode(&packed),
        checksum(&packed)
    ))
}

pub fn decode_calculator(code: &str) -> Result<CalcDocument, String> {
    if code.len() > MAX_CODE {
        return Err("分享碼過長 / Share code is too long".into());
    }
    let mut pieces = code.trim().split('.');
    let prefix = pieces.next().unwrap_or_default();
    let body = pieces.next().unwrap_or_default();
    let check = pieces.next().unwrap_or_default();
    if prefix != PREFIX || pieces.next().is_some() || check.len() != 32 {
        return Err("分享碼格式或版本不支援 / Unsupported share code format or version".into());
    }
    let packed = base64_decode(body)?;
    if checksum(&packed) != check {
        return Err("分享碼不完整或已損壞 / Share code is incomplete or damaged".into());
    }
    let mut decoder = ZlibDecoder::new(packed.as_slice());
    let mut json = Vec::new();
    (&mut decoder)
        .take(MAX_JSON as u64 + 1)
        .read_to_end(&mut json)
        .map_err(|_| "分享碼無法解壓 / Cannot decompress share code")?;
    if json.len() > MAX_JSON || decoder.total_in() != packed.len() as u64 {
        return Err("分享碼資料過大或格式錯誤 / Oversized or invalid share code data".into());
    }
    deserialize(&json)
}

fn load_from(path: &Path) -> Result<Option<CalcDocument>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("無法讀取計算 / Cannot read calculation: {error}")),
    };
    let mut body = Vec::new();
    file.take(MAX_JSON as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    deserialize(&body).map(Some).map_err(|error| {
        format!(
            "{error}；原有檔案已保留 / Original file preserved: {}",
            path.display()
        )
    })
}

fn save_to(path: &Path, document: &CalcDocument) -> Result<(), String> {
    let body = serialize(document)?;
    // Never replace corrupt or future-version state with a fresh default.
    load_from(path)?;
    write_preference_checked(path, body)
        .map_err(|error| format!("無法儲存計算 / Cannot save calculation: {error}"))
}

pub fn load_calculator() -> Result<Option<CalcDocument>, String> {
    load_from(&preferences_dir().join("calculator.json"))
}

pub fn save_calculator(document: &CalcDocument) -> Result<(), String> {
    save_to(&preferences_dir().join("calculator.json"), document)
}

pub fn calculator_export_dir() -> PathBuf {
    preferences_dir().join("exports")
}

#[cfg(test)]
mod tests;
