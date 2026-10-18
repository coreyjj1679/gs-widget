//! Saved recipe snapshots and data-only, offline folder sharing.
//!
//! Imported names are labels, never paths, commands, URLs, HTML, or templates.
//! IDs are allocated locally; an imported bundle cannot select an existing entry
//! to overwrite. Checksums detect corruption, not authenticity or honest prices.
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;

use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use serde::{Deserialize, Serialize};

use super::calculator::{base64_decode_limited, base64_encode, checksum, CalcDocument};
use super::user_data::{preferences_dir, write_preference_checked};

const VERSION: u32 = 1;
const PREFIX: &str = "GS-F1";
const FILE_NAME: &str = "calculator-library.json";
const MAX_FOLDERS: usize = 64;
const MAX_FOLDER_ENTRIES: usize = 64;
const MAX_ENTRIES: usize = 256;
const MAX_NAME: usize = 160;
const MAX_FILE: usize = 4 * 1024 * 1024;
const MAX_JSON: usize = 2 * 1024 * 1024;
const MAX_PACKED: usize = 512 * 1024;
const MAX_CODE: usize = MAX_PACKED * 4 / 3 + 80;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalcLibrary {
    pub version: u32,
    pub next_id: u64,
    pub folders: Vec<CalcFolder>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalcFolder {
    pub id: u64,
    pub name: String,
    pub entries: Vec<SavedCalculation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedCalculation {
    pub id: u64,
    pub name: String,
    pub document: CalcDocument,
}

/// Portable bundle deliberately contains no local IDs or filesystem locations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalcFolderShare {
    pub version: u32,
    pub name: String,
    pub entries: Vec<SharedCalculation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedCalculation {
    pub name: String,
    pub document: CalcDocument,
}

impl Default for CalcLibrary {
    fn default() -> Self {
        Self {
            version: VERSION,
            next_id: 2,
            folders: vec![CalcFolder {
                id: 1,
                name: "我的收藏".into(),
                entries: Vec::new(),
            }],
        }
    }
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= MAX_NAME
        && !name.chars().any(|ch| {
            ch.is_control()
                || matches!(ch, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn name_input(name: &str) -> Result<String, String> {
    if valid_name(name) {
        Ok(name.trim().into())
    } else {
        Err("名稱不可留空、包含控制字元或超過 160 bytes / Invalid or overly long name".into())
    }
}

fn missing_folder() -> String {
    "找不到資料夾 / Folder not found".into()
}

fn missing_entry() -> String {
    "找不到已儲存的計算 / Saved calculation not found".into()
}

impl CalcLibrary {
    fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err("不支援的收藏版本 / Unsupported library version".into());
        }
        if self.next_id == 0 || self.folders.is_empty() || self.folders.len() > MAX_FOLDERS {
            return Err("收藏資料無效或資料夾過多 / Invalid library or too many folders".into());
        }
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut count = 0;
        for folder in &self.folders {
            if folder.id == 0
                || folder.id >= self.next_id
                || !ids.insert(folder.id)
                || !valid_name(&folder.name)
                || !names.insert(folder.name.trim())
                || folder.entries.len() > MAX_FOLDER_ENTRIES
            {
                return Err(
                    "資料夾資料無效、重複或內容過多 / Invalid, duplicate, or oversized folder"
                        .into(),
                );
            }
            count += folder.entries.len();
            if count > MAX_ENTRIES {
                return Err(
                    "收藏最多可儲存 256 張計算表 / Library supports up to 256 calculations".into(),
                );
            }
            for entry in &folder.entries {
                if entry.id == 0
                    || entry.id >= self.next_id
                    || !ids.insert(entry.id)
                    || !valid_name(&entry.name)
                {
                    return Err(
                        "計算表資料無效或 ID 重複 / Invalid saved calculation or duplicate ID"
                            .into(),
                    );
                }
                entry.document.calculate()?;
            }
        }
        Ok(())
    }

    fn allocate_id(&mut self) -> Result<u64, String> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or("收藏 ID 已用盡 / Library ID range exhausted")?;
        Ok(id)
    }

    // Validate the complete replacement before changing memory or disk. The UI
    // can clone a library, mutate it, persist it, then publish the saved version.
    fn change<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        self.validate()?;
        let mut next = self.clone();
        let result = operation(&mut next)?;
        serialize_library(&next)?;
        *self = next;
        Ok(result)
    }

    pub fn new_folder(&mut self, name: &str) -> Result<u64, String> {
        let name = name_input(name)?;
        self.change(|next| {
            if next.folders.iter().any(|folder| folder.name.trim() == name) {
                return Err("資料夾名稱已存在 / Folder name already exists".into());
            }
            let id = next.allocate_id()?;
            next.folders.push(CalcFolder {
                id,
                name,
                entries: Vec::new(),
            });
            Ok(id)
        })
    }

    pub fn rename_folder(&mut self, id: u64, name: &str) -> Result<(), String> {
        let name = name_input(name)?;
        self.change(|next| {
            if next
                .folders
                .iter()
                .any(|folder| folder.id != id && folder.name.trim() == name)
            {
                return Err("資料夾名稱已存在 / Folder name already exists".into());
            }
            next.folders
                .iter_mut()
                .find(|folder| folder.id == id)
                .ok_or_else(missing_folder)?
                .name = name;
            Ok(())
        })
    }

    /// With an ID, update the existing sheet and move it if its folder changed.
    /// Without an ID, always create a new saved sheet, even for the same recipe.
    pub fn save_entry(
        &mut self,
        folder_id: u64,
        entry_id: Option<u64>,
        name: &str,
        document: CalcDocument,
    ) -> Result<u64, String> {
        let name = name_input(name)?;
        document.calculate()?;
        self.change(|next| {
            let destination = next
                .folders
                .iter()
                .position(|folder| folder.id == folder_id)
                .ok_or_else(missing_folder)?;
            let id = if let Some(id) = entry_id {
                let (source, position) = next.entry_position(id).ok_or_else(missing_entry)?;
                if source == destination {
                    next.folders[source].entries[position] =
                        SavedCalculation { id, name, document };
                    return Ok(id);
                }
                next.folders[source].entries.remove(position);
                id
            } else {
                next.allocate_id()?
            };
            next.folders[destination]
                .entries
                .push(SavedCalculation { id, name, document });
            Ok(id)
        })
    }

    fn entry_position(&self, id: u64) -> Option<(usize, usize)> {
        self.folders.iter().enumerate().find_map(|(folder, data)| {
            data.entries
                .iter()
                .position(|entry| entry.id == id)
                .map(|entry| (folder, entry))
        })
    }

    pub fn move_entry(&mut self, entry_id: u64, folder_id: u64) -> Result<(), String> {
        self.change(|next| {
            let destination = next
                .folders
                .iter()
                .position(|folder| folder.id == folder_id)
                .ok_or_else(missing_folder)?;
            let (source, position) = next.entry_position(entry_id).ok_or_else(missing_entry)?;
            if source != destination {
                let entry = next.folders[source].entries.remove(position);
                next.folders[destination].entries.push(entry);
            }
            Ok(())
        })
    }

    pub fn remove_entry(&mut self, entry_id: u64) -> Result<(), String> {
        self.change(|next| {
            let (folder, position) = next.entry_position(entry_id).ok_or_else(missing_entry)?;
            next.folders[folder].entries.remove(position);
            Ok(())
        })
    }

    /// Nonempty folders cannot be removed accidentally with all their sheets.
    pub fn remove_folder(&mut self, folder_id: u64) -> Result<(), String> {
        self.change(|next| {
            if next.folders.len() <= 1 {
                return Err("至少保留一個資料夾 / Keep at least one folder".into());
            }
            let position = next
                .folders
                .iter()
                .position(|folder| folder.id == folder_id)
                .ok_or_else(missing_folder)?;
            if !next.folders[position].entries.is_empty() {
                return Err(
                    "請先移動或刪除資料夾內的計算表 / Move or remove its calculations first".into(),
                );
            }
            next.folders.remove(position);
            Ok(())
        })
    }

    /// Imports always append a new folder. The source cannot supply local IDs,
    /// overwrite existing sheets, or choose any file location.
    pub fn import_folder(&mut self, share: CalcFolderShare) -> Result<u64, String> {
        serialize_share(&share)?;
        self.change(|next| {
            let name = unique_folder_name(&share.name, &next.folders);
            let id = next.allocate_id()?;
            let mut entries = Vec::with_capacity(share.entries.len());
            for entry in share.entries {
                entries.push(SavedCalculation {
                    id: next.allocate_id()?,
                    name: entry.name,
                    document: entry.document,
                });
            }
            next.folders.push(CalcFolder { id, name, entries });
            Ok(id)
        })
    }
}

fn unique_folder_name(name: &str, folders: &[CalcFolder]) -> String {
    let name = name.trim();
    if !folders.iter().any(|folder| folder.name.trim() == name) {
        return name.into();
    }
    for number in 2..=MAX_FOLDERS + 2 {
        let suffix = format!(" ({number})");
        let mut end = name.len().min(MAX_NAME - suffix.len());
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        let candidate = format!("{}{suffix}", name[..end].trim_end());
        if !folders.iter().any(|folder| folder.name.trim() == candidate) {
            return candidate;
        }
    }
    unreachable!("There are more candidate suffixes than existing folders")
}

impl CalcFolderShare {
    fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err("不支援的資料夾分享版本 / Unsupported folder share version".into());
        }
        if !valid_name(&self.name) || self.entries.len() > MAX_FOLDER_ENTRIES {
            return Err("資料夾名稱無效或超過 64 張計算表 / Invalid folder name or more than 64 calculations".into());
        }
        for entry in &self.entries {
            if !valid_name(&entry.name) {
                return Err("計算表名稱無效 / Invalid calculation name".into());
            }
            entry.document.calculate()?;
        }
        Ok(())
    }
}

fn serialize_library(library: &CalcLibrary) -> Result<Vec<u8>, String> {
    library.validate()?;
    let body = serde_json::to_vec(library).map_err(|_| "無法編碼收藏 / Cannot encode library")?;
    if body.len() > MAX_FILE {
        return Err("收藏資料超過 4 MiB / Library exceeds 4 MiB".into());
    }
    Ok(body)
}

fn deserialize_library(body: &[u8]) -> Result<CalcLibrary, String> {
    if body.len() > MAX_FILE {
        return Err("收藏資料超過 4 MiB / Library exceeds 4 MiB".into());
    }
    let library: CalcLibrary = serde_json::from_slice(body)
        .map_err(|_| "收藏資料格式或版本錯誤 / Invalid library format or version")?;
    library.validate()?;
    Ok(library)
}

fn serialize_share(share: &CalcFolderShare) -> Result<Vec<u8>, String> {
    share.validate()?;
    let body = serde_json::to_vec(share).map_err(|_| "無法編碼資料夾 / Cannot encode folder")?;
    if body.len() > MAX_JSON {
        return Err("資料夾分享資料超過 2 MiB / Folder share exceeds 2 MiB".into());
    }
    Ok(body)
}

fn deserialize_share(body: &[u8]) -> Result<CalcFolderShare, String> {
    if body.len() > MAX_JSON {
        return Err("資料夾分享資料超過 2 MiB / Folder share exceeds 2 MiB".into());
    }
    let share: CalcFolderShare = serde_json::from_slice(body)
        .map_err(|_| "資料夾分享格式或版本錯誤 / Invalid folder share format or version")?;
    share.validate()?;
    Ok(share)
}

pub fn encode_calculator_folder(folder: &CalcFolder) -> Result<String, String> {
    let share = CalcFolderShare {
        version: VERSION,
        name: folder.name.clone(),
        entries: folder
            .entries
            .iter()
            .map(|entry| SharedCalculation {
                name: entry.name.clone(),
                document: entry.document.clone(),
            })
            .collect(),
    };
    let body = serialize_share(&share)?;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&body)
        .map_err(|_| "無法壓縮資料夾 / Cannot compress folder")?;
    let packed = encoder
        .finish()
        .map_err(|_| "無法壓縮資料夾 / Cannot compress folder")?;
    if packed.len() > MAX_PACKED {
        return Err("資料夾分享碼過大 / Folder share code is too large".into());
    }
    Ok(format!(
        "{PREFIX}.{}.{}",
        base64_encode(&packed),
        checksum(&packed)
    ))
}

pub fn decode_calculator_folder(code: &str) -> Result<CalcFolderShare, String> {
    if code.len() > MAX_CODE {
        return Err("資料夾分享碼過長 / Folder share code is too long".into());
    }
    let mut pieces = code.trim().split('.');
    let prefix = pieces.next().unwrap_or_default();
    let body = pieces.next().unwrap_or_default();
    let check = pieces.next().unwrap_or_default();
    if prefix != PREFIX || pieces.next().is_some() || check.len() != 32 {
        return Err(
            "資料夾分享碼格式或版本不支援 / Unsupported folder share code format or version".into(),
        );
    }
    let packed = base64_decode_limited(body, MAX_PACKED)?;
    if checksum(&packed) != check {
        return Err(
            "資料夾分享碼不完整或已損壞 / Folder share code is incomplete or damaged".into(),
        );
    }
    let mut decoder = ZlibDecoder::new(packed.as_slice());
    let mut json = Vec::new();
    (&mut decoder)
        .take(MAX_JSON as u64 + 1)
        .read_to_end(&mut json)
        .map_err(|_| "資料夾分享碼無法解壓 / Cannot decompress folder share code")?;
    if json.len() > MAX_JSON || decoder.total_in() != packed.len() as u64 {
        return Err("資料夾分享資料過大或格式錯誤 / Oversized or invalid folder share data".into());
    }
    deserialize_share(&json)
}

fn load_from(path: &Path) -> Result<CalcLibrary, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CalcLibrary::default())
        }
        Err(error) => return Err(format!("無法讀取收藏 / Cannot read library: {error}")),
    };
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("收藏檔案不是一般檔案 / Library is not a regular file".into());
    }
    let mut body = Vec::new();
    file.take(MAX_FILE as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    deserialize_library(&body).map_err(|error| {
        format!(
            "{error}；原有檔案已保留 / Original file preserved: {}",
            path.display()
        )
    })
}

fn save_to(path: &Path, library: &CalcLibrary) -> Result<(), String> {
    let body = serialize_library(library)?;
    // Preserve corrupt/future state instead of replacing it with a default.
    load_from(path)?;
    write_preference_checked(path, body)
        .map_err(|error| format!("無法儲存收藏 / Cannot save library: {error}"))
}

pub fn load_calculator_library() -> Result<CalcLibrary, String> {
    load_from(&preferences_dir().join(FILE_NAME))
}

pub fn save_calculator_library(library: &CalcLibrary) -> Result<(), String> {
    save_to(&preferences_dir().join(FILE_NAME), library)
}

#[cfg(test)]
mod tests;
