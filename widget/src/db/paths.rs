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

#[cfg(all(not(windows), not(test)))]
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

pub(in crate::db) fn characters_dir(client_root: &Path, data: &Path) -> Option<PathBuf> {
    let bundled = client_root.join("Assets").join("Textures").join("Characters");
    if bundled.is_dir() {
        return Some(bundled);
    }
    client_hints(data).into_iter().find_map(|root| {
        let folder = root.join("Assets").join("Textures").join("Characters");
        folder.is_dir().then_some(folder)
    })
}

pub(in crate::db) fn client_hints(data: &Path) -> Vec<PathBuf> {
    let mut hints = Vec::new();
    if let Ok(path) = std::env::var("GERSANG_CLIENT") {
        let path = PathBuf::from(path.trim());
        if !path.as_os_str().is_empty() {
            hints.push(path);
        }
    }
    if let Ok(text) = std::fs::read_to_string(super::user_data::client_hint_file(data)) {
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
    let explicit = std::env::var("GERSANG_DB").ok().map(|path| PathBuf::from(path.trim()));
    let mut starts = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            starts.push(dir.to_path_buf());
        }
    }
    // Portable releases must load the data shipped beside their executable,
    // even when launched from a shortcut with an unrelated working directory.
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    find_db_in(explicit, starts)
}

fn find_db_in(explicit: Option<PathBuf>, starts: Vec<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = explicit.filter(|path| path.is_file()) {
        return Ok(path);
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

    #[test]
    fn portable_database_wins_over_working_directory_and_respects_override() {
        let root = super::super::user_data::preferences_dir().join("database-lookup");
        let executable = root.join("installed-app");
        let working = root.join("other-checkout");
        for directory in [&executable, &working] {
            std::fs::create_dir_all(directory.join("data")).unwrap();
            std::fs::write(directory.join("data/gersang.db"), b"fixture").unwrap();
        }
        let bundled = executable.join("data/gersang.db");
        let other = working.join("data/gersang.db");
        assert_eq!(find_db_in(None, vec![executable.clone(), working.clone()]).unwrap(), bundled);
        assert_eq!(find_db_in(Some(other.clone()), vec![executable.clone(), working.clone()]).unwrap(), other);
        // A normal development checkout still works when the executable is in
        // widget/target/debug rather than next to the data directory.
        assert_eq!(find_db_in(None, vec![working.join("widget/target/debug")]).unwrap(), other);
    }
}
