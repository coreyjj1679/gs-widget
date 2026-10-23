use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use super::text::{dotted_version, patch_version};

pub fn client_label() -> String {
    let text = std::fs::read_to_string(client_version_file()).unwrap_or_else(|_| "V3.4220".into());
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "V3.4220".into()
    } else {
        dotted_version(trimmed)
    }
}

pub(in crate::db) fn client_build() -> (u64, String) {
    patch_version(&client_label())
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
    client_asset_roots(client_root, data).into_iter().find_map(|root| {
        resolve_client_path(&root, "Assets/Textures/Characters").filter(|path| path.is_dir())
    })
}

pub(in crate::db) fn character_file_index(client_root: &Path, data: &Path, selected: Option<&Path>) -> HashMap<String, PathBuf> {
    let folders = selected.map(Path::to_path_buf).into_iter().chain(
        client_asset_roots(client_root, data).into_iter().flat_map(|root| {
            ["Assets/Textures/Characters", "Assets/Textures/HeroCharacter"].into_iter().filter_map(move |relative| {
                resolve_client_path(&root, relative).filter(|path| path.is_dir())
            })
        }),
    );
    let mut files = HashMap::new();
    let mut visited = Vec::new();
    for folder in folders {
        if visited.contains(&folder) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        let group = folder.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        visited.push(folder);
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_file() {
                // A partial bundled directory must not hide other characters
                // supplied by the configured full client. Earlier roots win.
                let name = entry.file_name().to_string_lossy().to_lowercase();
                files.entry(format!("assets/textures/{group}/{name}")).or_insert_with(|| path.clone());
                files.entry(name).or_insert(path);
            }
        }
    }
    files
}

/// Prefer shipped artwork, then the selected client and configured client paths.
pub(in crate::db) fn client_asset_roots(client_root: &Path, data: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for root in std::iter::once(data.join("client"))
        .chain(std::iter::once(client_root.to_path_buf()))
        .chain(client_hints(data))
    {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

pub(in crate::db) fn resolve_client_file(root: &Path, relative: &str) -> Option<PathBuf> {
    resolve_client_path(root, relative).filter(|path| path.is_file())
}

fn resolve_client_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let relative = relative.replace('\\', "/");
    let mut current = root.to_path_buf();
    // Client manifests use inconsistent filename casing. Walk only the supplied
    // relative components inside a known client root; never search the machine.
    for component in Path::new(&relative).components() {
        let name = match component {
            Component::Normal(name) => name,
            Component::CurDir => continue,
            _ => return None,
        };
        let exact = current.join(name);
        if exact.exists() {
            current = exact;
            continue;
        }
        let matched = std::fs::read_dir(&current).ok()?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().eq_ignore_ascii_case(&name.to_string_lossy()))
            .min_by_key(|entry| entry.file_name())?;
        current = matched.path();
    }
    Some(current)
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
    resolve_client_file(root, "gersang.gcs").is_some() || pictures_ready(root)
}

pub(in crate::db) fn pictures_ready(root: &Path) -> bool {
    resolve_client_file(root, "Assets/Textures/UI/Portraits/PORTRAIT_INVEN.AGF").is_some()
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
    fn client_assets_resolve_mixed_case_and_windows_separators_within_the_root() {
        let root = super::super::user_data::preferences_dir().join("client-asset-case");
        let portraits = root.join("assets/textures/ui/portraits");
        let characters = root.join("assets/textures/characters");
        std::fs::create_dir_all(&portraits).unwrap();
        std::fs::create_dir_all(&characters).unwrap();
        std::fs::write(portraits.join("horseportrait.agf"), b"horse").unwrap();
        std::fs::write(portraits.join("portrait_inven.agf"), b"portrait").unwrap();
        std::fs::write(characters.join("Water1.AGF"), b"spirit").unwrap();

        let horse = resolve_client_file(&root, r"Assets\Textures\UI\Portraits\HorsePortrait.AGF").unwrap();
        assert_eq!(std::fs::read(horse).unwrap(), b"horse");
        let spirit = resolve_client_file(&root, "Assets/Textures/Characters/WATER1.agf").unwrap();
        assert_eq!(std::fs::read(spirit).unwrap(), b"spirit");
        assert!(pictures_ready(&root));
        assert_eq!(characters_dir(&root, &root).unwrap().canonicalize().unwrap(), characters.canonicalize().unwrap());
        assert!(resolve_client_file(&root, "../outside.agf").is_none());
        assert!(resolve_client_file(&root, "/outside.agf").is_none());
        assert!(resolve_client_file(&root, "Assets/Textures/Characters/missing.agf").is_none());
    }

    #[test]
    fn missing_bundled_portrait_falls_back_to_selected_client() {
        let data = super::super::user_data::preferences_dir().join("client-asset-fallback");
        let external = data.join("full-client");
        let relative = "Assets/Textures/UI/Portraits/HorsePortrait.agf";
        let bundled_file = data.join("client").join(relative);
        let external_file = external.join(relative);
        for file in [&bundled_file, &external_file] {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        }
        std::fs::write(&bundled_file, b"bundled").unwrap();
        std::fs::write(&external_file, b"external").unwrap();
        let roots = client_asset_roots(&external, &data);
        let find = || roots.iter().find_map(|root| resolve_client_file(root, relative)).unwrap();
        assert_eq!(std::fs::read(find()).unwrap(), b"bundled");
        std::fs::remove_file(bundled_file).unwrap();
        assert_eq!(std::fs::read(find()).unwrap(), b"external");
    }

    #[test]
    fn partial_bundled_characters_keep_priority_and_include_external_spirits() {
        let data = super::super::user_data::preferences_dir().join("split-character-assets");
        let external = data.join("full-client");
        let bundled_folder = data.join("client/Assets/Textures/Characters");
        let external_folder = external.join("assets/textures/characters");
        let mounts_folder = external.join("assets/textures/herocharacter");
        std::fs::create_dir_all(&bundled_folder).unwrap();
        std::fs::create_dir_all(external_folder.join("nested")).unwrap();
        std::fs::create_dir_all(&mounts_folder).unwrap();
        std::fs::write(bundled_folder.join("FixtureMonster.AGF"), b"bundled monster").unwrap();
        std::fs::write(bundled_folder.join("FixtureShared.AGF"), b"bundled shared").unwrap();
        std::fs::write(external_folder.join("fixtureshared.agf"), b"external shared").unwrap();
        std::fs::write(external_folder.join("FixtureSpirit.AGF"), b"external spirit").unwrap();
        std::fs::write(mounts_folder.join("FixtureMount.AGF"), b"external mount").unwrap();
        std::fs::write(mounts_folder.join("fixtureshared.agf"), b"mount with same filename").unwrap();
        std::fs::write(external_folder.join("nested/fixture-nested.agf"), b"not a direct asset").unwrap();

        let files = character_file_index(&external, &data, Some(&bundled_folder));
        assert_eq!(std::fs::read(&files["fixturemonster.agf"]).unwrap(), b"bundled monster");
        assert_eq!(std::fs::read(&files["fixtureshared.agf"]).unwrap(), b"bundled shared");
        assert_eq!(std::fs::read(&files["fixturespirit.agf"]).unwrap(), b"external spirit");
        assert_eq!(std::fs::read(&files["fixturemount.agf"]).unwrap(), b"external mount");
        assert_eq!(std::fs::read(&files["assets/textures/herocharacter/fixtureshared.agf"]).unwrap(), b"mount with same filename");
        assert!(!files.contains_key("fixture-nested.agf"));
    }

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
