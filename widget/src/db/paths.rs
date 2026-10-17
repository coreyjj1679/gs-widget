use std::path::{Path, PathBuf};

use super::text::{dotted_version, patch_number};

pub fn client_label() -> String {
    let text = std::fs::read_to_string(client_version_file()).unwrap_or_else(|_| "V3.4220".into());
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "V3.4220".into()
    } else {
        dotted_version(trimmed)
    }
}

pub(in crate::db) fn client_build() -> i64 {
    patch_number(&client_label())
}

pub(in crate::db) fn client_version_file() -> PathBuf {
    find_db()
        .ok()
        .and_then(|db| db.parent().map(|dir| dir.join("client-version.txt")))
        .unwrap_or_else(|| PathBuf::from("data/client-version.txt"))
}

pub(in crate::db) fn data_dir() -> PathBuf {
    find_db()
        .ok()
        .and_then(|db| db.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub(in crate::db) fn locate_client(stored: PathBuf, data: &Path) -> PathBuf {
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

pub(in crate::db) fn client_hints(data: &Path) -> Vec<PathBuf> {
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

pub(in crate::db) fn client_ready(root: &Path) -> bool {
    root.join("gersang.gcs").is_file() || pictures_ready(root)
}

pub(in crate::db) fn pictures_ready(root: &Path) -> bool {
    root.join("Assets")
        .join("Textures")
        .join("UI")
        .join("Portraits")
        .join("PORTRAIT_INVEN.AGF")
        .is_file()
}

pub(in crate::db) fn local_drive_letters() -> Vec<char> {
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

pub(in crate::db) fn find_db() -> Result<PathBuf, String> {
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
