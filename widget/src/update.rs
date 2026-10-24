//! Portable Windows releases and an out-of-process, rollback-capable updater.
//!
//! Only application-owned files are replaced. User preferences, aliases and
//! favourites are deliberately outside the archive allow-list and transaction.
#![cfg_attr(not(windows), allow(dead_code))]

use reqwest::blocking::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REPOSITORY: &str = "coreyjj1679/gs-widget";
const API: &str = "https://api.github.com/repos/coreyjj1679/gs-widget/releases/latest";
const EXE: &str = "gersang-widget.exe";
const STAGING_PREFIX: &str = ".gs-widget-update-";
const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
const MAX_EXPANDED: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MANAGED: [&str; 4] = [
    EXE,
    "data/gersang.db",
    "data/client-version.txt",
    "data/client",
];
static UNIQUE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    url: String,
    size: u64,
    sha256: String,
}

#[cfg(test)]
pub(crate) fn release_for_test(version: &str) -> Release {
    Release {
        version: version.into(),
        url: "https://example.invalid/update.zip".into(),
        size: 1,
        sha256: "a".repeat(64),
    }
}

#[derive(Debug)]
pub struct PreparedUpdate {
    directory: PathBuf,
    launched: std::sync::atomic::AtomicBool,
    #[cfg(windows)]
    _lock: InstallationGuard,
}

/// An acquired Windows file-range lock. Explicit unlocking makes handoff to
/// another app/helper immediate instead of relying on deferred handle cleanup.
#[derive(Debug)]
pub struct InstallationGuard {
    #[cfg(windows)]
    file: File,
    #[cfg(windows)]
    extended: bool,
}

#[cfg(windows)]
impl Drop for InstallationGuard {
    fn drop(&mut self) {
        windows::unlock(&self.file, self.extended);
    }
}

impl Drop for PreparedUpdate {
    fn drop(&mut self) {
        if !self.launched.load(Ordering::Acquire)
            && !self.directory.join("journal.json").exists()
            && !self.directory.join("backup").exists()
        {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: u32,
    version: String,
    parent_pid: u32,
    files: Vec<VerifiedFile>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifiedFile {
    path: String,
    size: u64,
    sha256: String,
}

pub fn supported() -> bool {
    cfg!(all(target_os = "windows", target_arch = "x86_64"))
}

/// Hold for the GUI lifetime. Multiple app copies may read the installation;
/// the helper needs an exclusive lock after every copy has closed.
pub fn running_guard() -> Result<Option<InstallationGuard>, String> {
    #[cfg(windows)]
    {
        if !supported() {
            return Ok(None);
        }
        windows::running_lock(&install_directory()?, false).map(Some)
    }
    #[cfg(not(windows))]
    {
        Ok(None)
    }
}

fn http_client() -> Result<Client, String> {
    Client::builder()
        .user_agent(concat!("gs-widget/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let host = attempt.url().host_str().unwrap_or_default();
            if attempt.previous().len() >= 5 {
                attempt.error("Too many download redirects")
            } else if attempt.url().scheme() == "https"
                && (host == "github.com" || host.ends_with(".githubusercontent.com"))
            {
                attempt.follow()
            } else {
                attempt.error("Update redirected outside GitHub HTTPS")
            }
        }))
        .build()
        .map_err(|error| format!("Cannot initialize HTTPS: {error}"))
}

/// Run on a worker thread. A missing release (404) is not an update error.
pub fn check() -> Result<Option<Release>, String> {
    if !supported() {
        return Err("In-app updates currently support Windows x64 only".into());
    }
    let response = http_client()?
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(Duration::from_secs(30))
        .send()
        .map_err(|error| format!("Cannot contact GitHub: {error}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if response.status() == reqwest::StatusCode::FORBIDDEN
        || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        return Err(
            "GitHub temporarily refused the update check (rate limit); try again later".into(),
        );
    }
    let response = response
        .error_for_status()
        .map_err(|error| format!("GitHub update check failed: {error}"))?;
    let mut bytes = Vec::new();
    response
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read release metadata: {error}"))?;
    if bytes.len() > 1024 * 1024 {
        return Err("GitHub release metadata exceeded its size limit".into());
    }
    select_release(&bytes, env!("CARGO_PKG_VERSION"))
}

fn select_release(bytes: &[u8], current: &str) -> Result<Option<Release>, String> {
    let release: GithubRelease = serde_json::from_slice(bytes)
        .map_err(|error| format!("Invalid GitHub release metadata: {error}"))?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let tag = release
        .tag_name
        .strip_prefix('v')
        .ok_or("Release tag must start with v")?;
    let version = Version::parse(tag).map_err(|_| "Release tag is not a semantic version")?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err("Only stable release versions without build metadata are supported".into());
    }
    let current = Version::parse(current).map_err(|_| "Invalid installed application version")?;
    if version <= current {
        return Ok(None);
    }
    let name = format!("gs-widget-v{version}-windows-x64.zip");
    let mut assets = release
        .assets
        .into_iter()
        .filter(|asset| asset.name == name);
    let asset = assets
        .next()
        .ok_or_else(|| format!("Release is missing {name}"))?;
    if assets.next().is_some() {
        return Err("Release has duplicate Windows update assets".into());
    }
    let expected_url =
        format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{name}");
    if asset.browser_download_url != expected_url {
        return Err("Update asset URL does not match this repository and version".into());
    }
    if asset.size == 0 || asset.size > MAX_ARCHIVE {
        return Err("Update archive has an invalid size".into());
    }
    let sha256 = parse_digest(asset.digest.as_deref())?;
    Ok(Some(Release {
        version: version.to_string(),
        url: expected_url,
        size: asset.size,
        sha256,
    }))
}

fn parse_digest(digest: Option<&str>) -> Result<String, String> {
    let hash = digest
        .and_then(|value| value.strip_prefix("sha256:"))
        .ok_or("GitHub has not supplied a SHA-256 digest for this asset; try again later")?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("GitHub supplied an invalid SHA-256 digest".into());
    }
    Ok(hash.to_ascii_lowercase())
}

fn install_directory() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    if executable.file_name().and_then(|name| name.to_str()) != Some(EXE) {
        return Err(format!(
            "Run the unpacked Windows release as {EXE} to update it"
        ));
    }
    executable
        .parent()
        .ok_or("Cannot locate the application directory")?
        .canonicalize()
        .map_err(|error| format!("Cannot access the application directory: {error}"))
}

fn unique_directory(parent: &Path, prefix: &str) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    for _ in 0..32 {
        let directory = parent.join(format!(
            "{prefix}{}-{timestamp}-{}",
            std::process::id(),
            UNIQUE.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                "Cannot create update files in {}. Extract the app to a writable folder: {error}",
                parent.display()
            ))
            }
        }
    }
    Err("Cannot allocate a unique update directory".into())
}

fn cleanup_abandoned(install: &Path) {
    // Called while holding the installation update lock. Preserve every backup
    // and journal, including incomplete rollback evidence. A day-long grace
    // period also excludes a helper still waiting for its parent to close.
    let Ok(entries) = fs::read_dir(install) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(STAGING_PREFIX))
        {
            continue;
        }
        let directory = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&directory) else {
            continue;
        };
        if !metadata.is_dir()
            || is_link(&metadata)
            || directory.join("backup").exists()
            || directory.join("journal.json").exists()
        {
            continue;
        }
        if metadata
            .modified()
            .ok()
            .and_then(|time| time.elapsed().ok())
            .is_some_and(|elapsed| elapsed > Duration::from_secs(24 * 60 * 60))
        {
            let _ = fs::remove_dir_all(directory);
        }
    }
}

/// Download, verify and unpack without modifying any installed application data.
pub fn download(release: &Release, progress: impl Fn(u64, u64)) -> Result<PreparedUpdate, String> {
    if !supported() {
        return Err("In-app updates currently support Windows x64 only".into());
    }
    let install = install_directory()?;
    #[cfg(windows)]
    let update_lock = windows::installation_lock(&install)?;
    validate_installation(&install)?;
    cleanup_abandoned(&install);
    let directory = unique_directory(&install, STAGING_PREFIX)?;
    let result = (|| {
        let response = http_client()?
            .get(&release.url)
            .send()
            .map_err(|error| format!("Update download failed: {error}"))?
            .error_for_status()
            .map_err(|error| format!("Update download failed: {error}"))?;
        if response
            .content_length()
            .is_some_and(|length| length != release.size)
        {
            return Err("Update download size differs from GitHub metadata".into());
        }
        let archive_path = directory.join("download.zip");
        let mut output = File::create(&archive_path).map_err(|error| error.to_string())?;
        let mut response = response;
        let mut hasher = Sha256::new();
        let mut downloaded = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        progress(0, release.size);
        loop {
            let count = response
                .read(&mut buffer)
                .map_err(|error| format!("Update download was interrupted: {error}"))?;
            if count == 0 {
                break;
            }
            downloaded += count as u64;
            if downloaded > release.size || downloaded > MAX_ARCHIVE {
                return Err("Update download exceeded its declared size".into());
            }
            hasher.update(&buffer[..count]);
            output
                .write_all(&buffer[..count])
                .map_err(|error| format!("Cannot save update; check free disk space: {error}"))?;
            progress(downloaded, release.size);
        }
        output.sync_all().map_err(|error| error.to_string())?;
        drop(output);
        if downloaded != release.size || format!("{:x}", hasher.finalize()) != release.sha256 {
            return Err(
                "Update failed SHA-256 or size verification; installed files were not changed"
                    .into(),
            );
        }
        let payload = directory.join("payload");
        fs::create_dir(&payload).map_err(|error| error.to_string())?;
        let files = extract_archive(&archive_path, &payload)?;
        let plan = Plan {
            schema: 1,
            version: release.version.clone(),
            parent_pid: std::process::id(),
            files,
        };
        write_json(&directory.join("plan.json"), &plan)?;
        fs::remove_file(archive_path).map_err(|error| error.to_string())?;
        Ok(PreparedUpdate {
            directory: directory.clone(),
            launched: std::sync::atomic::AtomicBool::new(false),
            #[cfg(windows)]
            _lock: update_lock,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(directory);
    }
    result
}

/// Windows names are validated on every platform, including in macOS tests.
fn archive_path(name: &str, directory: bool) -> Result<String, String> {
    if name.is_empty() || name.len() > 220 || !name.is_ascii() || name.contains('\\') {
        return Err(format!("Invalid archive path: {name:?}"));
    }
    let path = if directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with('.')
            || component.ends_with(' ')
            || component
                .bytes()
                .any(|byte| byte < 32 || b"<>:\"|?*".contains(&byte))
        {
            return Err(format!("Unsafe Windows archive path: {name:?}"));
        }
        let base = component
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        if matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(format!("Reserved Windows archive path: {name:?}"));
        }
    }
    let allowed = if directory {
        path == "data" || path == "data/client" || path.starts_with("data/client/")
    } else {
        path == EXE
            || path == "data/gersang.db"
            || path == "data/client-version.txt"
            || path.starts_with("data/client/")
    };
    if !allowed {
        return Err(format!(
            "Archive includes a file outside the application data allow-list: {name:?}"
        ));
    }
    Ok(path.to_owned())
}

fn extract_archive(zip_path: &Path, destination: &Path) -> Result<Vec<VerifiedFile>, String> {
    let file = File::open(zip_path).map_err(|error| error.to_string())?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("Invalid update ZIP: {error}"))?;
    if archive.len() == 0 || archive.len() > MAX_ENTRIES {
        return Err("Update ZIP contains an invalid number of files".into());
    }
    let mut paths = HashSet::new();
    let mut total = 0u64;
    // Complete preflight before writing anything, including case-insensitive
    // collisions and regular-file ancestors of other entries.
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|error| error.to_string())?;
        if file.encrypted() || file.is_symlink() || (!file.is_file() && !file.is_dir()) {
            return Err("Encrypted archives, links and special files are not supported".into());
        }
        if let Some(mode) = file.unix_mode() {
            let kind = mode & 0o170000;
            if kind != 0 && kind != 0o100000 && kind != 0o040000 {
                return Err("ZIP contains a symlink or special file".into());
            }
        }
        let name = archive_path(file.name(), file.is_dir())?;
        let key = name.to_ascii_lowercase();
        if !paths.insert(key) {
            return Err("ZIP contains duplicate or case-colliding paths".into());
        }
        total = total
            .checked_add(file.size())
            .ok_or("ZIP expanded size overflow")?;
        if total > MAX_EXPANDED {
            return Err("ZIP expanded size exceeded its limit".into());
        }
        entries.push((name, file.is_dir(), file.size()));
    }
    let regular: HashSet<String> = entries
        .iter()
        .filter(|entry| !entry.1)
        .map(|entry| entry.0.to_ascii_lowercase())
        .collect();
    for (name, _, _) in &entries {
        let mut ancestor = name.as_str();
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            if regular.contains(&parent.to_ascii_lowercase()) {
                return Err("ZIP file is also used as a directory".into());
            }
            ancestor = parent;
        }
    }
    for required in [EXE, "data/gersang.db", "data/client-version.txt"] {
        if !entries
            .iter()
            .any(|entry| entry.0 == required && !entry.1 && entry.2 > 0)
        {
            return Err(format!("ZIP is missing required file {required}"));
        }
    }
    if !entries
        .iter()
        .any(|entry| entry.0.starts_with("data/client/") && !entry.1 && entry.2 > 0)
    {
        return Err("ZIP is missing client assets".into());
    }
    let mut verified = Vec::new();
    for (index, (name, directory, expected_size)) in entries.into_iter().enumerate() {
        let output_path = destination.join(&name);
        if directory {
            fs::create_dir_all(&output_path).map_err(|error| error.to_string())?;
            continue;
        }
        fs::create_dir_all(output_path.parent().ok_or("Invalid archive destination")?)
            .map_err(|error| error.to_string())?;
        let file = archive.by_index(index).map_err(|error| error.to_string())?;
        let mut file = file.take(expected_size + 1);
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .map_err(|error| error.to_string())?;
        let mut hasher = Sha256::new();
        let mut actual_size = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|error| format!("Corrupt ZIP entry {name}: {error}"))?;
            if count == 0 {
                break;
            }
            actual_size += count as u64;
            if actual_size > expected_size {
                return Err("ZIP entry exceeded its declared size".into());
            }
            hasher.update(&buffer[..count]);
            output
                .write_all(&buffer[..count])
                .map_err(|error| error.to_string())?;
        }
        if actual_size != expected_size {
            return Err(format!("ZIP entry {name} has an invalid size"));
        }
        output.sync_all().map_err(|error| error.to_string())?;
        verified.push(VerifiedFile {
            path: name,
            size: actual_size,
            sha256: format!("{:x}", hasher.finalize()),
        });
    }
    Ok(verified)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    // Keep the previous journal intact if writing the next state is interrupted.
    let temporary = path.with_extension("json.new");
    let mut file = File::create(&temporary).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn reject_links(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect {}: {error}", path.display()))?;
    if is_link(&metadata) {
        return Err(format!(
            "Update cannot replace linked paths: {}",
            path.display()
        ));
    }
    if metadata.is_dir() {
        for child in fs::read_dir(path).map_err(|error| error.to_string())? {
            reject_links(&child.map_err(|error| error.to_string())?.path())?;
        }
    } else if !metadata.is_file() {
        return Err(format!("Update found a special file: {}", path.display()));
    }
    Ok(())
}

fn validate_installation(install: &Path) -> Result<(), String> {
    let data = install.join("data");
    for path in [install, data.as_path()] {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("Cannot inspect {}: {error}", path.display()))?;
        if is_link(&metadata) || !metadata.is_dir() {
            return Err(format!(
                "Application directory is not a regular directory: {}",
                path.display()
            ));
        }
    }
    for name in MANAGED {
        let path = install.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => reject_links(&path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn verify_payload(payload: &Path, files: &[VerifiedFile]) -> Result<(), String> {
    reject_links(payload)?;
    let mut names = HashSet::new();
    for entry in files {
        archive_path(&entry.path, false)?;
        if !names.insert(entry.path.to_ascii_lowercase()) {
            return Err("Duplicate staged file".into());
        }
        let path = payload.join(&entry.path);
        let mut file = File::open(&path).map_err(|error| error.to_string())?;
        if file.metadata().map_err(|error| error.to_string())?.len() != entry.size {
            return Err(format!("Staged file changed size: {}", entry.path));
        }
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher).map_err(|error| error.to_string())?;
        if format!("{:x}", hasher.finalize()) != entry.sha256 {
            return Err(format!(
                "Staged file failed SHA-256 verification: {}",
                entry.path
            ));
        }
    }
    // Ensure the manifest covers every extracted file, including the client
    // directory which will be renamed as a unit.
    fn inspect(root: &Path, dir: &Path, names: &HashSet<String>) -> Result<(), String> {
        for child in fs::read_dir(dir).map_err(|error| error.to_string())? {
            let path = child.map_err(|error| error.to_string())?.path();
            if path.is_dir() {
                inspect(root, &path, names)?;
            } else {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_str()
                    .ok_or("Invalid staged filename")?
                    .replace('\\', "/");
                if !names.contains(&relative.to_ascii_lowercase()) {
                    return Err(format!("Unexpected staged file: {relative}"));
                }
            }
        }
        Ok(())
    }
    inspect(payload, payload, &names)?;
    for required in [EXE, "data/gersang.db", "data/client-version.txt"] {
        if !files
            .iter()
            .any(|file| file.path == required && file.size > 0)
        {
            return Err(format!("Staged update is missing {required}"));
        }
    }
    if !files
        .iter()
        .any(|file| file.path.starts_with("data/client/") && file.size > 0)
    {
        return Err("Staged update is missing client assets".into());
    }
    Ok(())
}

fn verify_windows_x64_executable(path: &Path) -> Result<(), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut dos_header = [0u8; 64];
    file.read_exact(&mut dos_header)
        .map_err(|_| "Update executable has no valid DOS header")?;
    if &dos_header[..2] != b"MZ" {
        return Err("Update executable is not a Windows executable".into());
    }
    let offset = u32::from_le_bytes(dos_header[60..64].try_into().unwrap());
    if !(64..=1024 * 1024).contains(&offset) {
        return Err("Update executable has an invalid PE header offset".into());
    }
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|error| error.to_string())?;
    let mut header = [0u8; 26];
    file.read_exact(&mut header)
        .map_err(|_| "Update executable has a truncated PE header")?;
    if &header[..4] != b"PE\0\0"
        || u16::from_le_bytes([header[4], header[5]]) != 0x8664
        || u16::from_le_bytes([header[24], header[25]]) != 0x20b
    {
        return Err("Update executable must be a Windows x64 PE32+ application".into());
    }
    Ok(())
}

#[cfg(windows)]
fn check_staged_application(payload: &Path) -> Result<(), String> {
    verify_windows_x64_executable(&payload.join(EXE))?;
    let mut child = std::process::Command::new(payload.join(EXE))
        .arg("--check-installation")
        .current_dir(payload)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("New application cannot run its installation check: {error}"))?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "New application failed its installation check: {status}"
                ))
            };
        }
        if started.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "New application did not finish its installation check within 15 seconds".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The helper acknowledges startup before the caller should close its GUI.
pub fn launch_installer(update: &PreparedUpdate) -> Result<(), String> {
    #[cfg(windows)]
    {
        if update.launched.load(Ordering::Acquire) {
            return Err("This update helper has already started".into());
        }
        // Failed attempts are reaped below before returning, so these markers
        // cannot belong to a still-running helper when retrying this update.
        for name in ["ready", "cancel", "error.txt"] {
            match fs::remove_file(update.directory.join(name)) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(format!("Cannot reset updater state: {error}")),
            }
        }
        let source = std::env::current_exe().map_err(|error| error.to_string())?;
        let helper = update.directory.join("updater.exe");
        fs::copy(source, &helper)
            .map_err(|error| format!("Cannot prepare update helper: {error}"))?;
        let mut child = std::process::Command::new(helper)
            .arg("--apply-update")
            .arg(update.directory.join("plan.json"))
            .current_dir(&update.directory)
            .spawn()
            .map_err(|error| format!("Cannot start update helper: {error}"))?;
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(30) {
            if update.directory.join("error.txt").is_file() {
                let detail = fs::read_to_string(update.directory.join("error.txt"))
                    .unwrap_or_else(|_| "Unknown helper error".into());
                stop_unready_helper(&mut child, update)?;
                return Err(format!("Update helper could not start: {detail}"));
            }
            if update.directory.join("ready").is_file() {
                update.launched.store(true, Ordering::Release);
                return Ok(());
            }
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                let detail = fs::read_to_string(update.directory.join("error.txt"))
                    .unwrap_or_else(|_| status.to_string());
                return Err(format!("Update helper could not start: {detail}"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        stop_unready_helper(&mut child, update)?;
        Err("Update helper did not become ready. The application has not been changed".into())
    }
    #[cfg(not(windows))]
    {
        let _ = update;
        Err("In-app updates currently support Windows x64 only".into())
    }
}

#[cfg(windows)]
fn stop_unready_helper(
    child: &mut std::process::Child,
    update: &PreparedUpdate,
) -> Result<(), String> {
    // The parent is still alive, so the helper cannot yet be replacing files.
    let _ = fs::write(
        update.directory.join("cancel"),
        b"parent cancelled this helper",
    );
    if child
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(());
    }
    if let Err(error) = child.kill() {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none()
        {
            update.launched.store(true, Ordering::Release);
            return Err(format!("Cannot stop update helper; retry is disabled until the application restarts: {error}"));
        }
    }
    child
        .wait()
        .map_err(|error| format!("Cannot confirm helper shutdown: {error}"))?;
    Ok(())
}

#[derive(Default, Serialize)]
struct MoveState {
    path: String,
    backed_up: bool,
    installed: bool,
}

fn journal(directory: &Path, states: &[MoveState]) -> Result<(), String> {
    write_json(&directory.join("journal.json"), &states)
}

fn remove_managed(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if is_link(&metadata) {
        return Err(format!("Refusing to remove linked path {}", path.display()));
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|error| format!("Cannot remove {}: {error}", path.display()))
}

fn rollback(install: &Path, directory: &Path, states: &mut [MoveState]) -> Result<(), String> {
    let mut errors = Vec::new();
    for state in states.iter_mut().rev() {
        let installed = install.join(&state.path);
        if state.installed {
            match remove_managed(&installed) {
                Ok(()) => state.installed = false,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            }
        }
        if state.backed_up {
            match fs::rename(directory.join("backup").join(&state.path), &installed) {
                Ok(()) => state.backed_up = false,
                Err(error) => {
                    errors.push(format!("Cannot restore {}: {error}", installed.display()))
                }
            }
        }
    }
    if let Err(error) = journal(directory, states) {
        errors.push(error);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

// `before_move` enables real filesystem fault injection without mocking the
// operation whose rollback/preservation guarantees are under test.
fn replace_managed(
    install: &Path,
    directory: &Path,
    mut before_move: impl FnMut(usize) -> Result<(), String>,
) -> Result<Vec<MoveState>, String> {
    validate_installation(install)?;
    let backup = directory.join("backup");
    fs::create_dir(&backup).map_err(|error| error.to_string())?;
    fs::create_dir(backup.join("data")).map_err(|error| error.to_string())?;
    let recovery = "If an update was interrupted, close all gs-widget processes before recovery.\nKeep a copy of this entire recovery directory before making changes.\nThe actual contents of backup/ are the authoritative previous application files. A power interruption can occur between a move and its journal write, so journal.json can be incomplete or one move behind.\nFor each unit PRESENT in backup/, restore that unit to the same relative path in the installation directory: gersang-widget.exe, data/gersang.db, data/client-version.txt, data/client. Replace an existing data/client directory as a unit rather than merging it. Do not remove any installed unit solely because it is absent from backup/.\nNever replace the entire data directory: unrelated files may contain personal preferences or aliases. Personal files in the Windows user profile must also remain untouched.\n";
    fs::write(directory.join("RECOVERY.txt"), recovery).map_err(|error| error.to_string())?;
    let mut states: Vec<MoveState> = MANAGED
        .iter()
        .map(|name| MoveState {
            path: (*name).to_owned(),
            ..Default::default()
        })
        .collect();
    let result: Result<(), String> = (|| {
        journal(directory, &states)?;
        for index in 0..states.len() {
            let name = &states[index].path;
            let installed = install.join(name);
            let saved = backup.join(name);
            let payload = directory.join("payload").join(name);
            before_move(index * 2)?;
            if installed.exists() {
                fs::rename(&installed, saved).map_err(|error| {
                    format!(
                        "Cannot back up {} (close other copies of the app): {error}",
                        installed.display()
                    )
                })?;
                states[index].backed_up = true;
                journal(directory, &states)?;
            }
            before_move(index * 2 + 1)?;
            fs::rename(payload, &installed)
                .map_err(|error| format!("Cannot install {}: {error}", installed.display()))?;
            states[index].installed = true;
            journal(directory, &states)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(states),
        Err(error) => match rollback(install, directory, &mut states) {
            Ok(()) => Err(format!("{error}. The previous application was restored")),
            Err(restore) => Err(format!(
                "{error}. Automatic recovery is incomplete: {restore}. Keep the backup at {}",
                backup.display()
            )),
        },
    }
}

/// Call this before initializing Slint, the database or user settings.
pub fn run_helper_if_requested() -> bool {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new("--apply-update")) {
        return false;
    }
    #[cfg(windows)]
    {
        if let Err(error) = helper_main() {
            if let Ok(executable) = std::env::current_exe() {
                if let Some(directory) = executable.parent() {
                    let _ = fs::write(directory.join("error.txt"), &error);
                }
            }
            windows::show_error(&error);
        }
    }
    true
}

#[cfg(windows)]
fn helper_main() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let directory = executable
        .parent()
        .ok_or("Cannot find update helper directory")?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if executable.file_name().and_then(|name| name.to_str()) != Some("updater.exe")
        || !directory
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(STAGING_PREFIX))
    {
        return Err("Invalid update helper location".into());
    }
    let argument = std::env::args_os().nth(2).ok_or("Missing update plan")?;
    if PathBuf::from(argument)
        .canonicalize()
        .map_err(|error| error.to_string())?
        != directory.join("plan.json")
    {
        return Err("Update plan must belong to this helper".into());
    }
    let install = directory
        .parent()
        .ok_or("Cannot find installation directory")?;
    let bytes = fs::read(directory.join("plan.json")).map_err(|error| error.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Update plan is too large".into());
    }
    let plan: Plan =
        serde_json::from_slice(&bytes).map_err(|error| format!("Invalid update plan: {error}"))?;
    if plan.schema != 1
        || plan.parent_pid == 0
        || plan.parent_pid == std::process::id()
        || Version::parse(&plan.version).is_err()
        || plan.files.len() > MAX_ENTRIES
    {
        return Err("Unsupported update plan".into());
    }
    let parent = windows::ParentProcess::open(plan.parent_pid)?;
    verify_payload(&directory.join("payload"), &plan.files)?;
    check_staged_application(&directory.join("payload"))?;
    if directory.join("cancel").exists() {
        return Err("Update was cancelled before the app closed".into());
    }
    fs::write(directory.join("ready"), b"ready").map_err(|error| error.to_string())?;
    parent.wait()?;
    if directory.join("cancel").exists() {
        return Err("Update was cancelled; installed files were not changed".into());
    }
    let _lock = windows::installation_lock(install)?;
    let running_lock = windows::running_lock(install, true)?;
    // Recheck after shutdown, then move only the four application-owned units.
    verify_payload(&directory.join("payload"), &plan.files)?;
    let mut states = match replace_managed(install, &directory, |_| Ok(())) {
        Ok(states) => states,
        Err(error) => {
            drop(running_lock);
            if error.contains("The previous application was restored") {
                let _ = restart(install);
            }
            return Err(format!("{error}\nRecovery files: {}", directory.display()));
        }
    };
    drop(running_lock);
    if let Err(error) = restart(install) {
        // A user may open another copy after the exclusive guard is released.
        // Never roll back underneath it; the verified new installation and
        // previous backup remain intact for manual restart/recovery.
        let recovery_guard = windows::running_lock(install, true).map_err(|lock| {
            format!(
                "New version is installed but could not start: {error}. {lock}. Backup: {}",
                directory.display()
            )
        })?;
        match rollback(install, &directory, &mut states) {
            Ok(()) => {
                drop(recovery_guard);
                let _ = restart(install);
                return Err(format!(
                    "Cannot start the new app: {error}. The previous version was restored"
                ));
            }
            Err(restore) => {
                return Err(format!(
                    "Cannot start the new app: {error}. Recovery failed: {restore}. Keep {}",
                    directory.display()
                ))
            }
        }
    }
    let _ = fs::write(directory.join("completed"), plan.version);
    Ok(())
}

#[cfg(windows)]
fn restart(install: &Path) -> Result<(), String> {
    std::process::Command::new(install.join(EXE))
        .current_dir(install)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    type Handle = *mut c_void;
    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: Handle,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
        fn LockFile(
            file: Handle,
            offset_low: u32,
            offset_high: u32,
            length_low: u32,
            length_high: u32,
        ) -> i32;
        fn LockFileEx(
            file: Handle,
            flags: u32,
            reserved: u32,
            length_low: u32,
            length_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn UnlockFile(
            file: Handle,
            offset_low: u32,
            offset_high: u32,
            length_low: u32,
            length_high: u32,
        ) -> i32;
        fn UnlockFileEx(
            file: Handle,
            reserved: u32,
            length_low: u32,
            length_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(window: Handle, text: *const u16, title: *const u16, flags: u32) -> i32;
    }
    pub struct ParentProcess(Handle);
    impl ParentProcess {
        pub fn open(pid: u32) -> Result<Self, String> {
            // Hold a kernel handle before acknowledging readiness: a PID alone
            // could be reused after the GUI exits.
            let handle = unsafe { OpenProcess(0x00100000, 0, pid) }; // SYNCHRONIZE
            if handle.is_null() {
                Err(format!(
                    "Cannot observe application shutdown: {}",
                    std::io::Error::last_os_error()
                ))
            } else {
                Ok(Self(handle))
            }
        }
        pub fn wait(&self) -> Result<(), String> {
            match unsafe { WaitForSingleObject(self.0, 90_000) } {
                0 => Ok(()),
                258 => Err(
                    "The application did not close within 90 seconds; no files were replaced"
                        .into(),
                ),
                _ => Err(format!(
                    "Cannot wait for application shutdown: {}",
                    std::io::Error::last_os_error()
                )),
            }
        }
    }
    impl Drop for ParentProcess {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    pub fn installation_lock(
        install: &std::path::Path,
    ) -> Result<super::InstallationGuard, String> {
        use std::os::windows::io::AsRawHandle;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(install.join(".gs-widget-update.lock"))
            .map_err(|error| format!("Cannot lock this installation for updating: {error}"))?;
        if unsafe { LockFile(file.as_raw_handle(), 0, 0, 1, 0) } == 0 {
            return Err("Another update is already in progress for this application folder".into());
        }
        // Construct only after successful acquisition, so Drop never unlocks
        // another handle's lock after a failed attempt. Keep the file itself
        // to avoid inode races; the OS remains the fallback after a crash.
        Ok(super::InstallationGuard {
            file,
            extended: false,
        })
    }
    pub fn running_lock(
        install: &std::path::Path,
        exclusive: bool,
    ) -> Result<super::InstallationGuard, String> {
        use std::os::windows::io::AsRawHandle;
        let path = install.join(".gs-widget-running.lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| format!("Cannot open application lock {}: {error}", path.display()))?;
        let mut overlapped = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: std::ptr::null_mut(),
        };
        // FAIL_IMMEDIATELY, plus EXCLUSIVE_LOCK for the updater. Shared GUI
        // locks allow simultaneous copies while protecting their database.
        let flags = if exclusive { 3 } else { 1 };
        if unsafe { LockFileEx(file.as_raw_handle(), flags, 0, 1, 0, &mut overlapped) } == 0 {
            return Err(if exclusive {
                "Close every other gs-widget window and disconnect Gersang MCP in your AI apps before updating. No installed files were changed".into()
            } else {
                "This application folder is being updated. Wait for the update to finish, then open it again".into()
            });
        }
        Ok(super::InstallationGuard {
            file,
            extended: true,
        })
    }
    pub fn unlock(file: &std::fs::File, extended: bool) {
        use std::os::windows::io::AsRawHandle;
        if extended {
            let mut overlapped = Overlapped {
                internal: 0,
                internal_high: 0,
                offset: 0,
                offset_high: 0,
                event: std::ptr::null_mut(),
            };
            // Drop must not panic/report errors. Closing the handle after this
            // call still provides the OS fallback if explicit unlock fails.
            unsafe {
                UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &mut overlapped);
            }
        } else {
            unsafe {
                UnlockFile(file.as_raw_handle(), 0, 0, 1, 0);
            }
        }
    }
    pub fn show_error(error: &str) {
        let text: Vec<u16> = format!("更新未完成 / Update could not finish\n\n{error}\0")
            .encode_utf16()
            .collect();
        let title: Vec<u16> = "gs-widget 更新\0".encode_utf16().collect();
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            Self(unique_directory(&std::env::temp_dir(), "gs-update-test-").unwrap())
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_application_locks_handoff_between_shared_and_exclusive_handles() {
        let root = TestDirectory::new();
        let first = windows::running_lock(&root.0, false).unwrap();
        let second = windows::running_lock(&root.0, false).unwrap();
        assert!(windows::running_lock(&root.0, true).is_err());
        drop(first);
        assert!(windows::running_lock(&root.0, true).is_err());
        drop(second);

        let updater = windows::running_lock(&root.0, true).unwrap();
        assert!(windows::running_lock(&root.0, false).is_err());
        assert!(windows::running_lock(&root.0, true).is_err());
        drop(updater);
        let restarted_app = windows::running_lock(&root.0, false).unwrap();
        drop(restarted_app);
        let next_update = windows::running_lock(&root.0, true).unwrap();
        drop(next_update);
    }

    #[cfg(windows)]
    #[test]
    fn windows_update_lock_excludes_another_handle_then_releases_immediately() {
        let root = TestDirectory::new();
        let downloading = windows::installation_lock(&root.0).unwrap();
        assert!(windows::installation_lock(&root.0).is_err());
        // A failed acquisition must not unlock the still-active first guard.
        assert!(windows::installation_lock(&root.0).is_err());
        drop(downloading);
        let helper = windows::installation_lock(&root.0).unwrap();
        assert!(windows::installation_lock(&root.0).is_err());
        drop(helper);
        assert!(windows::installation_lock(&root.0).is_ok());
    }

    fn metadata(version: &str, digest: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "tag_name":format!("v{version}"), "draft":false, "prerelease":false,
            "assets":[{"name":format!("gs-widget-v{version}-windows-x64.zip"), "size":42,
                "browser_download_url":format!("https://github.com/{REPOSITORY}/releases/download/v{version}/gs-widget-v{version}-windows-x64.zip"), "digest":digest}]
        })).unwrap()
    }

    #[test]
    fn selects_only_newer_stable_matching_assets_with_sha256() {
        let digest = serde_json::json!(format!("sha256:{}", "a".repeat(64)));
        assert_eq!(
            select_release(&metadata("1.10.0", digest.clone()), "1.9.0")
                .unwrap()
                .unwrap()
                .version,
            "1.10.0"
        );
        assert!(select_release(&metadata("1.0.0", digest.clone()), "1.0.0")
            .unwrap()
            .is_none());
        assert!(select_release(&metadata("1.0.0", digest.clone()), "1.1.0")
            .unwrap()
            .is_none());
        assert!(select_release(&metadata("2.0.0-beta.1", digest.clone()), "1.0.0").is_err());
        assert!(select_release(&metadata("2.0.0", serde_json::Value::Null), "1.0.0").is_err());
        assert!(
            select_release(&metadata("2.0.0", serde_json::json!("sha256:ab")), "1.0.0").is_err()
        );
        let mut wrong: serde_json::Value =
            serde_json::from_slice(&metadata("2.0.0", digest)).unwrap();
        wrong["assets"][0]["browser_download_url"] =
            serde_json::json!("https://example.com/file.zip");
        assert!(select_release(&serde_json::to_vec(&wrong).unwrap(), "1.0.0").is_err());
    }

    #[test]
    fn rejects_windows_traversal_devices_streams_and_personal_data() {
        for path in [
            "../outside",
            "/data/client/a",
            "C:/a",
            "data\\client\\a",
            "data/client/../a",
            "data/client/a:stream",
            "data/client/NUL.png",
            "data/client/com1.txt",
            "data/client/a.",
            "data/client/a ",
            "data/client//a",
            "data/client/a\0b",
            "data/preferences.json",
            "data/aliases.json",
            "data/client/./a",
            "data/client/é.png",
        ] {
            assert!(archive_path(path, false).is_err(), "accepted {path:?}");
        }
        assert!(archive_path("data/client/images/item-1.png", false).is_ok());
        assert!(archive_path("data/client/", true).is_ok());
    }

    #[test]
    fn executable_preflight_rejects_non_windows_and_wrong_architecture() {
        let root = TestDirectory::new();
        let path = root.0.join(EXE);
        fs::write(&path, b"not an executable").unwrap();
        assert!(verify_windows_x64_executable(&path).is_err());
        let mut executable = vec![0u8; 128];
        executable[..2].copy_from_slice(b"MZ");
        executable[60..64].copy_from_slice(&64u32.to_le_bytes());
        executable[64..68].copy_from_slice(b"PE\0\0");
        executable[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        executable[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        fs::write(&path, &executable).unwrap();
        verify_windows_x64_executable(&path).unwrap();
        executable[68..70].copy_from_slice(&0x14cu16.to_le_bytes());
        fs::write(&path, executable).unwrap();
        assert!(verify_windows_x64_executable(&path).is_err());
    }

    fn make_zip(path: &Path, extra: Option<&str>, symlink: bool) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for name in [
            EXE,
            "data/gersang.db",
            "data/client-version.txt",
            "data/client/image.png",
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(b"verified-content").unwrap();
        }
        if let Some(name) = extra {
            if symlink {
                zip.add_symlink(name, "../../outside", options).unwrap();
            } else {
                zip.start_file(name, options).unwrap();
                zip.write_all(b"attack").unwrap();
            }
        }
        zip.finish().unwrap();
    }

    #[test]
    fn archive_checks_links_collisions_and_manifest_integrity() {
        for (name, symlink) in [
            ("data/client/IMAGE.png", false),
            ("data/client/link", true),
            ("../outside", false),
            ("data/aliases.json", false),
        ] {
            let root = TestDirectory::new();
            let zip = root.0.join("archive.zip");
            make_zip(&zip, Some(name), symlink);
            let payload = root.0.join("payload");
            fs::create_dir(&payload).unwrap();
            assert!(extract_archive(&zip, &payload).is_err(), "accepted {name}");
            assert_eq!(fs::read_dir(payload).unwrap().count(), 0);
        }
        let root = TestDirectory::new();
        let zip = root.0.join("archive.zip");
        make_zip(&zip, None, false);
        let payload = root.0.join("payload");
        fs::create_dir(&payload).unwrap();
        let files = extract_archive(&zip, &payload).unwrap();
        verify_payload(&payload, &files).unwrap();
        fs::write(payload.join("data/client/image.png"), b"corrupted-content").unwrap();
        assert!(verify_payload(&payload, &files).is_err());
    }

    fn installation(root: &Path) -> PathBuf {
        fs::create_dir_all(root.join("data/client")).unwrap();
        fs::write(root.join(EXE), b"old-exe").unwrap();
        fs::write(root.join("data/gersang.db"), b"old-db").unwrap();
        fs::write(root.join("data/client-version.txt"), b"old-version").unwrap();
        fs::write(root.join("data/client/old.png"), b"old-image").unwrap();
        fs::write(
            root.join("data/preferences.json"),
            b"favourites-and-settings",
        )
        .unwrap();
        fs::write(root.join("data/aliases.json"), b"personal-aliases").unwrap();
        let staging = root.join("staging");
        fs::create_dir_all(staging.join("payload/data/client")).unwrap();
        fs::write(staging.join("payload").join(EXE), b"new-exe").unwrap();
        fs::write(staging.join("payload/data/gersang.db"), b"new-db").unwrap();
        fs::write(
            staging.join("payload/data/client-version.txt"),
            b"new-version",
        )
        .unwrap();
        fs::write(staging.join("payload/data/client/new.png"), b"new-image").unwrap();
        staging
    }

    fn assert_personal(root: &Path) {
        assert_eq!(
            fs::read(root.join("data/preferences.json")).unwrap(),
            b"favourites-and-settings"
        );
        assert_eq!(
            fs::read(root.join("data/aliases.json")).unwrap(),
            b"personal-aliases"
        );
    }

    #[test]
    fn every_move_failure_restores_old_files_and_preserves_personal_data() {
        for failure in 0..8 {
            let root = TestDirectory::new();
            let staging = installation(&root.0);
            assert!(
                replace_managed(&root.0, &staging, |step| if step == failure {
                    Err("injected failure".into())
                } else {
                    Ok(())
                })
                .is_err()
            );
            assert_eq!(fs::read(root.0.join(EXE)).unwrap(), b"old-exe");
            assert_eq!(fs::read(root.0.join("data/gersang.db")).unwrap(), b"old-db");
            assert_eq!(
                fs::read(root.0.join("data/client-version.txt")).unwrap(),
                b"old-version"
            );
            assert_eq!(
                fs::read(root.0.join("data/client/old.png")).unwrap(),
                b"old-image"
            );
            assert!(!root.0.join("data/client/new.png").exists());
            assert_personal(&root.0);
        }
    }

    #[test]
    fn successful_transaction_keeps_backup_and_can_rollback_restart_failure() {
        let root = TestDirectory::new();
        let staging = installation(&root.0);
        let mut states = replace_managed(&root.0, &staging, |_| Ok(())).unwrap();
        assert_eq!(fs::read(root.0.join(EXE)).unwrap(), b"new-exe");
        assert_eq!(
            fs::read(staging.join("backup").join(EXE)).unwrap(),
            b"old-exe"
        );
        assert!(!root.0.join("data/client/old.png").exists());
        assert!(root.0.join("data/client/new.png").exists());
        assert_personal(&root.0);
        rollback(&root.0, &staging, &mut states).unwrap();
        assert_eq!(fs::read(root.0.join(EXE)).unwrap(), b"old-exe");
        assert!(root.0.join("data/client/old.png").exists());
        assert_personal(&root.0);
    }

    #[cfg(unix)]
    #[test]
    fn linked_installation_is_rejected_without_mutating_target() {
        let root = TestDirectory::new();
        let staging = installation(&root.0);
        let outside = TestDirectory::new();
        fs::write(outside.0.join("precious"), b"keep").unwrap();
        fs::remove_dir_all(root.0.join("data/client")).unwrap();
        std::os::unix::fs::symlink(&outside.0, root.0.join("data/client")).unwrap();
        assert!(replace_managed(&root.0, &staging, |_| Ok(())).is_err());
        assert_eq!(fs::read(outside.0.join("precious")).unwrap(), b"keep");
        assert_eq!(fs::read(root.0.join(EXE)).unwrap(), b"old-exe");
        assert_personal(&root.0);
    }
}
