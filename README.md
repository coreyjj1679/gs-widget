# Gersang

A bilingual item and monster reference for Windows and macOS. You do not need the game installed.

The catalog is `data/gersang.db`. Pictures are in `data/client`.

## Download for Windows

1. Open [Releases](https://github.com/coreyjj1679/gs-widget/releases/latest) and download **`gs-widget-v1.0.0-windows-x64.zip`** (or the newer version's ZIP).
2. Extract the entire ZIP to a writable folder, such as `%LOCALAPPDATA%\Programs\gs-widget`. Keep `gersang-widget.exe` and `data` together.
3. Open `gersang-widget.exe`. Windows users do not need Git, Rust, Visual Studio, or the game installed. Use the compiled `gs-widget-…-windows-x64.zip` asset; GitHub's **Source code** downloads are for developers.

Each launch checks for updates in the background. When a newer version is available, a download icon and version appear at the bottom left of the navigation bar. The currently installed app version is shown below the app name at the top left. Offline or failed background checks remain quiet; the local catalog, pictures, favourites and settings stay available. You can also check manually under **Settings → App / 設定 → 程式**, where failures are shown with a retry option. Opening an external website, such as the source repository, also requires an internet connection.

Choose **Download update / 下載更新** when you want the new version. Once the download is verified, choose **Restart and update / 重新啟動並更新**. The app restarts into the new version; downloads do not interrupt browsing or restart the app automatically. Updates currently support the Windows x64 release.

Favourites, aliases, language, shortcuts, and window settings are stored in `%LOCALAPPDATA%\gs-widget`, separately from the app's replaceable catalog and pictures. The first launch migrates settings found beside the old catalog without deleting the original files or replacing existing settings. Keep the old `data` folder in place for that first launch. To back up your personal data, copy `%LOCALAPPDATA%\gs-widget` while the app is closed.

See [WINDOWS.md](WINDOWS.md) for Windows build, release, update, and runtime verification instructions.

## Build from source

Windows developers need [Git](https://git-scm.com/download/win), [Rust](https://rustup.rs) **1.92 or newer**, and [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with **Desktop development with C++** and the Windows SDK (`rc.exe`). From PowerShell:

```powershell
git clone https://github.com/coreyjj1679/gs-widget.git
cd gs-widget
cargo build --locked --release --manifest-path widget/Cargo.toml --target x86_64-pc-windows-msvc
.\widget\target\x86_64-pc-windows-msvc\release\gersang-widget.exe
```

For maintainers, write the next version's user-facing changes in [`release-notes.json`](release-notes.json), with paired English (`en`) and Traditional Chinese (`zh-TW`) entries. Put the newest stable version first, use a `YYYY-MM-DD` release date, and group changes under `added`, `improved`, or `fixed`. Each English bullet must have its Chinese counterpart at the same position. Keep earlier entries so the app can show its bundled release history offline. Do not describe unshipped work as part of an older release.

Bump the package version in `widget/Cargo.toml` and its `gersang-widget` entry in `widget/Cargo.lock`, then validate the notes with `python scripts/release_notes.py --version 1.0.2` (substitute the version being released). Add `--render` to preview the exact English GitHub Release description. Merge and push that release commit to `main`, then create and push the matching stable tag, such as `v1.0.2`.

Only version-tag pushes trigger the Windows GitHub Actions workflow. It verifies the tagged commit is already in `origin/main` history, checks that the tag matches the package version and has complete bilingual notes, then runs the tests and ZIP packaging. It creates a draft Release titled with the tag, verifies the uploaded ZIP's digest, and publishes the English notes from the same file embedded in the app. The app's release history follows the player's interface language setting. Commit messages are not used to generate notes, and existing public releases are never overwritten. The original `v1.0.0` and `v1.0.1` GitHub descriptions remain empty. Ordinary pushes to `main`, feature or development branches and pull requests do not run this workflow; manual dispatch is disabled.

## macOS

Install the Xcode Command Line Tools with `xcode-select --install`, then install [Rust](https://rustup.rs) **1.92 or newer**. Open a new terminal and check `rustc --version`; run `rustup update stable` if needed.

```sh
git clone https://github.com/coreyjj1679/gs-widget.git
cd gs-widget/widget
cargo run --locked
```

From the repository root, you can instead run `cargo run --locked --manifest-path widget/Cargo.toml`. For an optimized build, run `cargo run --locked --release` from `widget`.

The F1–F9 shortcuts may require holding Fn/Globe, depending on your keyboard settings. macOS or another app may already use some function keys; use the widget's category buttons if a shortcut is unavailable.

## Interface

### AI assistants / MCP

The current source includes a local, read-only MCP server. In **Full / 完整** mode, choose **MCP** at the bottom of the sidebar for Codex, Cursor and Claude setup instructions. Copy buttons use this installation's actual executable and catalog paths. The compact widget has no MCP page.

The same executable runs without a window when an AI client launches it with `--mcp`. It provides catalog search, details, monster drops, quest dependencies, recipes and cost calculation from user-supplied prices. You do not need Node.js, Python, an API key or a separate server. See [MCP.md](MCP.md) for desktop setup, limitations and verification. This feature requires a build containing these changes; older released ZIPs do not include it.

Use **Full / 完整** for the desktop workspace: persistent category navigation, category search, monster comparisons, and a list beside the selected record's details. The overview provides quick entries and saved monsters. Full mode supports windows down to 960 × 640 logical pixels.

Use **Widget / 小窗** for the compact interface, or **–** for the floating category bar. The compact mode keeps its existing warm palette. Both systems share the Slint interface; macOS uses PingFang TC and Windows uses Microsoft JhengHei UI. See [DESIGN.md](DESIGN.md) for the implemented design tokens and layout rules.

Left-click a name in a record's detail heading to copy that name. Chinese and English headings copy separately, including item, quest, character, NPC and monster names; recipe headings copy the result item's name. Only the canonical name is copied, without IDs, quantities or descriptions. Hover to see the full name and copy hint, or focus the name and press Enter, Space or Ctrl+C. Existing list and related-record links continue to open their records. This is included in the current source build.

Copy feedback appears at the bottom centre of the content area (beside the sidebar in Full mode). Names stay borderless.

### Characters, mounts and spirits

The character directory includes **Mounts** and **Spirit**, with a fixed representative portrait for each category where an image is available. Select a character for a preview, then choose **Open full details** to view its portrait, base stats, skills and progression materials. Chinese and English names copy separately. The compact widget opens the same scrollable detail page. Mount lists show the required riding level and movement bonus, ordered by riding level. Spirits are grouped by their spirit family element and ordered by grade; this family can differ from a low-grade spirit's combat element. Spirit skill effects appear first, with base stats available under **Show base stats**.

The bundled catalog contains base stats for 263 characters and 33 spirit forms, plus 15 mounts with their recorded bonuses, riding requirements and materials. Unknown fields are omitted; mount bonuses are not presented as character base stats. These records come from the local parsed game database, rather than the older reference websites. Maintainers can refresh them transactionally with `python3 scripts/import_character_data.py /path/to/gersang-full.db` (see `--help` for options).

Character, spirit and mount portraits use the bundled portrait atlases. The bundle also includes animation sheets for all 33 spirit forms and base mount artwork for all 15 mounts, with a play/pause control. Mount previews show the base artwork; the game's additional layered glow effects are not composited. Other character animations can be supplied through `GERSANG_CLIENT` or `data/client.txt`. Configured client paths are checked case-insensitively when an asset is absent from the bundle. Missing assets show an unavailable message, and fully transparent animation frames are skipped.

## Data

Monster drops show the client catalog's recorded percentage beside each item. Unmatched rates appear as **未記錄 / Not recorded** (or **—** in the icon strip); they are not treated as zero. Single-player drops retain their existing label. These are the client's base values, before any in-game bonuses or events.

In full-mode monster filters, search the drop-item dropdown by name, alias or item code (for example `#11608`). It lists only items with monster drop sources. Selecting an item filters its monsters and sorts their recorded drop rates from high to low; the sort direction can be reversed, and unknown rates always remain last. The selected drop is highlighted in each monster's drops. If an item occurs in multiple slots, sorting uses the highest recorded slot rate, without adding different conditions together.

To refresh rates after replacing the catalog, run `python3 scripts/import_drop_rates.py /path/to/gs-parser/data/gersang-full.db --target data/gersang.db`. The importer matches monster and item codes within the same normal/single-player drop mode, preserves existing drop rows, and stores the original integer in `monster_drops.drop_rate_per_100000` (`5500` means `5.5%`). Source metadata is recorded in `meta` under `drop_rates_source`. Map-specific boss rewards and reward pools are not merged into per-monster rates.

The first source build downloads dependencies and can take a few minutes. Keep the repository's `data` folder in place: the program locates the catalog from the executable or working directory. You can also set `GERSANG_DB` to the catalog file's path. Windows preferences are stored under `%LOCALAPPDATA%\gs-widget`; macOS preferences remain beside the active database.

Window positions are saved separately in `window.txt` on Windows and `window-macos.txt` on macOS, so Retina scaling does not affect the Windows layout. The app version (for example `1.0.0`) is separate from the catalog's game data version (for example `V3.4220`).

The bundled pictures work without the game. Optional battle animations need additional client assets; set `GERSANG_CLIENT` to the client root or put that path in `%LOCALAPPDATA%\gs-widget\client.txt` on Windows, or `client.txt` beside the active database on macOS.
