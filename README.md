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

Day-to-day development happens directly on `develop` while this project has one developer. Keep `main` as the released baseline; move changes from `develop` to `main` only when preparing a release. A normal push to `develop` does not publish a release.

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

Use **Full / 完整** for the desktop workspace: persistent category navigation, category search, monster comparisons, and a list beside the selected record's details. The overview provides quick entries and saved monsters. Full mode supports windows down to 960 × 640 logical pixels.

Use **Widget / 小窗** for the compact interface, or **–** for the floating category bar. Full and widget modes share the same slate colors, bronze accents and control styling while keeping their different layouts and density. The floating bar uses the same palette with a translucent background; your window opacity setting still applies. Both systems share the Slint interface; macOS uses PingFang TC and Windows uses Microsoft JhengHei UI. See [DESIGN.md](DESIGN.md) for the implemented design tokens and layout rules.

Item names use a primary line and a smaller alternate-language line, following your Chinese-first or English-first language setting. Missing or identical translations are shown only once. Widget mode keeps compact **Details / Related** tabs (or **Pieces** for sets).

Full-mode monster drop icons and compact Legacy drop names show both languages on hover. Expanded drop lists show the primary and secondary names on separate lines, following the selected language order.

In full mode, catalog search keeps the category directory and current preview visible. Search covers the whole category; clearing it restores the previous browsing section and scroll position. Empty results offer a clear-search action. Select a category on the left to leave search and browse it. Crafting, production, and usage groups can be expanded directly in item previews. Item, set, quest, recipe, NPC, hero, and other supported record previews provide **Open details**, and Back returns to the selected preview.

In full mode, open an item or set to see its picture and bilingual name in a horizontal header, with attributes on the left and sources or pieces on the right. Both columns stay visible and scroll independently. **Tab** switches keyboard focus between them without resetting their scroll positions. The catalog preview also provides **Open details** to reach this page.

Full-mode settings expand with the window. Look, Bar, Keys and App use grouped columns when space allows, then stack at narrower widths; alias rows use the available width with the same editor. The App page places update controls beside the changelog. All nine bar entries remain reachable by scrolling at 960 × 640. Resizing keeps the scroll position within the new content bounds, while widget settings keep their compact layout.

## Data

The first source build downloads dependencies and can take a few minutes. Keep the repository's `data` folder in place: the program locates the catalog from the executable or working directory. You can also set `GERSANG_DB` to the catalog file's path. Windows preferences are stored under `%LOCALAPPDATA%\gs-widget`; macOS preferences remain beside the active database.

Window positions are saved separately in `window.txt` on Windows and `window-macos.txt` on macOS, so Retina scaling does not affect the Windows layout. The app version (for example `1.0.0`) is separate from the catalog's game data version (for example `V3.4220`).

The bundled pictures work without the game. Optional battle animations need additional client assets; set `GERSANG_CLIENT` to the client root or put that path in `%LOCALAPPDATA%\gs-widget\client.txt` on Windows, or `client.txt` beside the active database on macOS.

## Performance diagnostics

Run these with an optimized build (`cargo build --locked --release --manifest-path widget/Cargo.toml`) when investigating a user's machine. Compare the same catalog, renderer, window size, build profile, and hardware. Debug-build timings are useful for regression comparisons but are not release latency estimates.

- `gersang-widget --profile-catalog catalog-profile.json` writes local JSON with catalog startup, every category and populated section, fuzzy search, monster-card preparation, and cold/warm image decoding timings. It runs without opening a window. Normal first-launch preference initialization/migration applies. The image sample is deterministically sorted and capped at 1,000 distinct frames; failed decodes are counted. “Cold” means a fresh process cache, not a cleared OS disk cache.
- `gersang-widget --profile-ui ui-profile.json` opens a temporary native window using the Slint software renderer and benchmarks the actual Slint catalog with **synthetic** 50/500/5,000-row lists at 1280×800 and 960×640. It writes JSON plus adjacent PPM snapshots at the top/middle/end. It does not load or save personal settings. This measures model replacement, layout and CPU snapshot rasterization, **not** screen presentation, database work or asynchronous image loading.

To time actual navigation, searches and scrolling, set `GERSANG_PROFILE` to a writable JSONL file and start the app normally. For example, in PowerShell from an extracted Windows package:

```powershell
$env:GERSANG_PROFILE = "$env:TEMP\gs-widget-performance.jsonl"
Start-Process .\gersang-widget.exe -Wait
Remove-Item Env:GERSANG_PROFILE
```

For the headless report on Windows, use `Start-Process .\gersang-widget.exe -ArgumentList '--profile-catalog', 'catalog-profile.json' -Wait` because the application is a GUI executable. On macOS, use `GERSANG_PROFILE=/tmp/gs-widget-performance.jsonl ./widget/target/release/gersang-widget`.

Each JSONL entry contains `process_id`, `since_start_ms`, `stage`, `elapsed_ms`, and `count`. Reproduce the slow action, then close the app. Relevant stages are:

- `navigation.<category>`: identifies the category associated with the following spans without recording the query.
- `startup.catalog_initialize`: user-data initialization and catalog opening.
- `list.query`, `list.monster_cards`, `list.rows`, `list.model_set`, `list.publish`, `list.reload_board`, `detail.open`: nested wall-time spans for data lookup, row preparation, model assignment and navigation. Do not add overlapping spans together.
- `icons.queued`: pending-job count, with zero duration. `icon.decode`, `icon.queue_wait`, `icon.paint`: decoding batches, event-loop queue delay, and applying images to models. Count is jobs or targets, not unique files.
- `ui.render_span`: wall time between the renderer's Before/AfterRendering callbacks. `ui.update_to_render`: earliest instrumented update through the next AfterRendering callback. Neither includes display presentation or guarantees that every icon has finished loading. A backend without notifier support emits `ui.render_notifier_unavailable` instead; no render timing is fabricated.
- `ui.event_loop_delay`: excess delay over a 50 ms UI timer, recorded when at least 20 ms late. OS scheduling, app suspension and sleep also affect it, so keep the app visible and the machine awake while recording.

Logging is opt-in, local and best effort; it records no search text, names or personal settings and sends nothing over the network. A bounded background writer avoids blocking the UI; a saturated queue or abrupt shutdown may drop diagnostics. Disable `GERSANG_PROFILE` after reproducing the issue to stop log growth. The headless catalog report excludes GUI costs, while the synthetic UI report excludes icons and real catalog work; use the runtime trace to diagnose actual interaction latency.

For algorithm comparisons using the bundled assets:

```sh
cargo test --locked --manifest-path widget/Cargo.toml profile_icon_row_updates -- --ignored --nocapture
cargo test --locked --manifest-path widget/Cargo.toml profile_item_frame_decode -- --ignored --nocapture
```

These manual benchmarks compare the previous and current algorithms in one process and check output equivalence. They are not end-to-end loading measurements.
