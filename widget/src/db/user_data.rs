//! Keep mutable Windows preferences outside the replaceable application bundle.
use std::path::{Path, PathBuf};

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(all(windows, not(test)))]
use std::sync::OnceLock;

#[cfg(all(windows, not(test)))]
static USER_DATA: OnceLock<PathBuf> = OnceLock::new();

#[cfg(any(windows, test))]
const USER_FILES: &[&str] = &[
    "favourites.txt",
    "aliases.txt",
    "ui-lang.txt",
    "languages.txt",
    "view.txt",
    "dock.txt",
    "home.txt",
    "opacity.txt",
    "set-extra.txt",
    "window.txt",
    "window-macos.txt",
    "client.txt",
    "planner.json",
    "calculator.json",
    "calculator-library.json",
];

/// Call before loading the catalogue or any preferences. Never fall back to the
/// application directory if migration fails: an updater may replace that tree.
pub fn initialize_user_data() -> Result<(), String> {
    #[cfg(all(windows, not(test)))]
    {
        if USER_DATA.get().is_some() {
            return Ok(());
        }
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                "找不到 Windows LOCALAPPDATA 個人資料目錄，為保障收藏和設定，程式不會繼續啟動。"
                    .to_string()
            })?;
        let destination = root.join("gs-widget");
        let db = super::paths::find_db()?;
        let source = db
            .parent()
            .ok_or_else(|| "找不到舊版設定目錄。".to_string())?;
        migrate_user_data(source, &destination).map_err(|error| {
            format!(
                "無法保留個人資料至 {}：{error}。原有資料未被刪除，請修正目錄權限後重試。",
                destination.display()
            )
        })?;
        // Concurrent startup paths use the same directory; publishing only after
        // migration succeeds prevents readers seeing an incomplete migration.
        let _ = USER_DATA.set(destination);
    }
    #[cfg(test)]
    {
        let _ = preferences_dir();
    }
    Ok(())
}

pub(in crate::db) fn preferences_dir() -> PathBuf {
    #[cfg(test)]
    {
        return TEST_USER_DATA.with(|directory| directory.0.clone());
    }
    #[cfg(all(windows, not(test)))]
    {
        USER_DATA
            .get()
            .expect("initialize_user_data must succeed before accessing preferences")
            .clone()
    }
    #[cfg(all(not(windows), not(test)))]
    {
        super::paths::data_dir()
    }
}

pub(in crate::db) fn client_hint_file(data: &Path) -> PathBuf {
    #[cfg(any(windows, test))]
    {
        let _ = data;
        preferences_dir().join("client.txt")
    }
    #[cfg(all(not(windows), not(test)))]
    {
        data.join("client.txt")
    }
}

/// On Windows, write complete, flushed temporary files before replacing a
/// preference so interruption cannot truncate the previous copy.
pub(in crate::db) fn write_preference(path: impl AsRef<Path>, body: impl AsRef<[u8]>) {
    let path = path.as_ref();
    #[cfg(any(windows, test))]
    let result = write_preference_checked(path, body);
    #[cfg(all(not(windows), not(test)))]
    let result = std::fs::write(path, body);
    if let Err(error) = result {
        eprintln!("無法儲存設定 {}：{error}", path.display());
    }
}

/// Atomically replace state on every platform and return failures to the UI.
pub(in crate::db) fn write_preference_checked(
    path: impl AsRef<Path>,
    body: impl AsRef<[u8]>,
) -> std::io::Result<()> {
    atomic_write(path.as_ref(), body.as_ref())
}

// A concurrent migration or another instance's first save must win over a
// default alias file. Do not use the replacing save operation for defaults.
pub(in crate::db) fn write_default_preference(path: &Path, body: &[u8]) {
    #[cfg(any(windows, test))]
    let result = (|| {
        let directory = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "設定路徑沒有父目錄"))?;
        let (temporary, mut file) = new_temporary(directory)?;
        file.write_all(body)?;
        file.sync_all()?;
        drop(file);
        publish(&temporary.0, path, false)
    })();
    #[cfg(all(not(windows), not(test)))]
    let result = {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .and_then(|mut file| file.write_all(body))
    };
    if let Err(error) = result {
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            eprintln!("無法建立預設設定 {}：{error}", path.display());
        }
    }
}

#[cfg(any(windows, test))]
fn migrate_user_data(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    // Check that the destination is writable even on a clean installation with
    // no legacy preferences. A failure must stop startup, not appear as defaults.
    let (probe, file) = new_temporary(destination)?;
    file.sync_all()?;
    drop(file);
    drop(probe);
    for name in USER_FILES {
        let target = destination.join(name);
        match fs::metadata(&target) {
            Ok(metadata) if metadata.is_file() => {
                File::open(&target)?;
                continue;
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} 不是一般檔案", target.display()),
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let original = source.join(name);
        let mut input = match File::open(&original) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !input.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} 不是一般檔案", original.display()),
            ));
        }
        let (temporary, mut output) = new_temporary(destination)?;
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        drop(output);
        match publish(&temporary.0, &target, false) {
            Ok(()) => {}
            // Another instance may have saved/migrated this preference in the
            // meantime. Its complete file always wins; never overwrite it.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !target.is_file() {
                    return Err(error);
                }
                File::open(&target)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn new_temporary(directory: &Path) -> io::Result<(Temporary, File)> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    loop {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(".gs-widget-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((Temporary(path), file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

fn atomic_write(path: &Path, body: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "設定路徑沒有父目錄"))?;
    let (temporary, mut file) = new_temporary(directory)?;
    file.write_all(body)?;
    file.sync_all()?;
    drop(file);
    publish(&temporary.0, path, true)
}

#[cfg(windows)]
fn publish(source: &Path, destination: &Path, replace: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let wide = |path: &Path| -> io::Result<Vec<u16>> {
        let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "設定路徑包含 NUL",
            ));
        }
        value.push(0);
        Ok(value)
    };
    let source = wide(source)?;
    let destination = wide(destination)?;
    const MOVEFILE_REPLACE_EXISTING: u32 = 1;
    const MOVEFILE_WRITE_THROUGH: u32 = 8;
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn publish(source: &Path, destination: &Path, replace: bool) -> io::Result<()> {
    if replace {
        fs::rename(source, destination)
    } else {
        // A same-filesystem hard link provides atomic, no-clobber publication.
        // Temporary's Drop removes the second link after success.
        fs::hard_link(source, destination)
    }
}

#[cfg(test)]
struct TestDirectory(PathBuf);

#[cfg(test)]
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        loop {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "gs-widget-preferences-test-{}-{id}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create isolated test preferences: {error}"),
            }
        }
    }
}

#[cfg(test)]
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
thread_local! {
    // Tests never touch LOCALAPPDATA or preferences in the checked-out data/.
    static TEST_USER_DATA: TestDirectory = TestDirectory::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_every_preference_without_touching_bundle_or_originals() {
        let legacy = TestDirectory::new();
        let profile = TestDirectory::new();
        for name in USER_FILES {
            fs::write(legacy.0.join(name), format!("saved {name}\n收藏\n")).unwrap();
        }
        fs::write(legacy.0.join("gersang.db"), b"bundled database").unwrap();
        fs::write(legacy.0.join("client-version.txt"), b"V3.4220").unwrap();
        migrate_user_data(&legacy.0, &profile.0).unwrap();
        for name in USER_FILES {
            assert_eq!(
                fs::read(profile.0.join(name)).unwrap(),
                fs::read(legacy.0.join(name)).unwrap(),
                "{name}"
            );
        }
        assert!(!profile.0.join("gersang.db").exists());
        assert!(!profile.0.join("client-version.txt").exists());
        // Replacing the entire application data directory cannot erase the
        // migrated favourites, aliases or settings.
        fs::remove_dir_all(&legacy.0).unwrap();
        for name in USER_FILES {
            assert_eq!(
                fs::read_to_string(profile.0.join(name)).unwrap(),
                format!("saved {name}\n收藏\n")
            );
        }
    }

    #[test]
    fn existing_preferences_win_including_intentionally_empty_favourites() {
        let legacy = TestDirectory::new();
        let profile = TestDirectory::new();
        fs::write(legacy.0.join("favourites.txt"), b"item:123").unwrap();
        fs::write(legacy.0.join("aliases.txt"), b"OLD 123").unwrap();
        fs::write(profile.0.join("favourites.txt"), b"").unwrap();
        fs::write(profile.0.join("aliases.txt"), b"NEW 456").unwrap();
        for _ in 0..2 {
            migrate_user_data(&legacy.0, &profile.0).unwrap();
            assert!(fs::read(profile.0.join("favourites.txt"))
                .unwrap()
                .is_empty());
            assert_eq!(fs::read(profile.0.join("aliases.txt")).unwrap(), b"NEW 456");
        }
        assert_eq!(
            fs::read(legacy.0.join("favourites.txt")).unwrap(),
            b"item:123"
        );
        assert_eq!(fs::read(legacy.0.join("aliases.txt")).unwrap(), b"OLD 123");
    }

    #[test]
    fn failed_migration_retries_missing_files_without_reverting_completed_files() {
        let legacy = TestDirectory::new();
        let profile = TestDirectory::new();
        fs::write(legacy.0.join("favourites.txt"), b"item:123").unwrap();
        // A directory where the second preference should be forces an error
        // after favourites have been published successfully.
        fs::create_dir(legacy.0.join("aliases.txt")).unwrap();
        assert!(migrate_user_data(&legacy.0, &profile.0).is_err());
        assert_eq!(
            fs::read(profile.0.join("favourites.txt")).unwrap(),
            b"item:123"
        );
        assert!(!profile.0.join("aliases.txt").exists());
        fs::write(profile.0.join("favourites.txt"), b"item:456").unwrap();
        fs::remove_dir(legacy.0.join("aliases.txt")).unwrap();
        fs::write(legacy.0.join("aliases.txt"), b"MY ALIAS 789").unwrap();
        // An orphan left by an interrupted copy is never mistaken for a
        // completed preference and does not prevent a retry.
        fs::write(profile.0.join(".gs-widget-interrupted.tmp"), b"partial").unwrap();
        migrate_user_data(&legacy.0, &profile.0).unwrap();
        assert_eq!(
            fs::read(profile.0.join("favourites.txt")).unwrap(),
            b"item:456"
        );
        assert_eq!(
            fs::read(profile.0.join("aliases.txt")).unwrap(),
            b"MY ALIAS 789"
        );
        assert_eq!(
            fs::read(legacy.0.join("favourites.txt")).unwrap(),
            b"item:123"
        );
    }

    #[test]
    fn invalid_destination_fails_without_changing_legacy_data() {
        let legacy = TestDirectory::new();
        let profile = TestDirectory::new();
        fs::write(legacy.0.join("favourites.txt"), b"item:123").unwrap();
        fs::write(profile.0.join("blocked"), b"keep me").unwrap();
        assert!(migrate_user_data(&legacy.0, &profile.0.join("blocked")).is_err());
        assert_eq!(
            fs::read(legacy.0.join("favourites.txt")).unwrap(),
            b"item:123"
        );
        assert_eq!(fs::read(profile.0.join("blocked")).unwrap(), b"keep me");
    }

    #[test]
    fn publication_never_clobbers_a_file_created_during_migration() {
        let profile = TestDirectory::new();
        let (temporary, mut file) = new_temporary(&profile.0).unwrap();
        file.write_all(b"legacy").unwrap();
        file.sync_all().unwrap();
        drop(file);
        let target = profile.0.join("aliases.txt");
        fs::write(&target, b"new preference").unwrap();
        assert_eq!(
            publish(&temporary.0, &target, false).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&target).unwrap(), b"new preference");
    }

    #[test]
    fn incomplete_save_does_not_modify_existing_preference() {
        let profile = TestDirectory::new();
        let target = profile.0.join("favourites.txt");
        fs::write(&target, b"item:123").unwrap();
        {
            let (_temporary, mut file) = new_temporary(&profile.0).unwrap();
            file.write_all(b"partial").unwrap();
            // Simulate stopping before atomic publication.
        }
        assert_eq!(fs::read(&target).unwrap(), b"item:123");
        atomic_write(&target, b"item:456\nitem:789").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"item:456\nitem:789");
        assert_eq!(fs::read_dir(&profile.0).unwrap().count(), 1);
    }
}
