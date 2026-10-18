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

For maintainers, first merge and push the release commit to `main`, then push its stable version tag, such as `v1.0.0`. Only version-tag pushes trigger the Windows GitHub Actions workflow. It verifies the tagged commit is already in `origin/main` history, runs the tests and ZIP packaging, then creates a Release titled `v1.0.0` with an empty description. The tag must match `widget/Cargo.toml`; the process does not generate patch notes. Ordinary pushes to `main`, feature or development branches and pull requests do not run this workflow; manual dispatch is disabled.

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

Use **Widget / 小窗** for the compact interface, or **–** for the floating category bar. The compact mode keeps its existing warm palette. Both systems share the Slint interface; macOS uses PingFang TC and Windows uses Microsoft JhengHei UI. See [DESIGN.md](DESIGN.md) for the implemented design tokens and layout rules.

## Data

The first source build downloads dependencies and can take a few minutes. Keep the repository's `data` folder in place: the program locates the catalog from the executable or working directory. You can also set `GERSANG_DB` to the catalog file's path. Windows preferences are stored under `%LOCALAPPDATA%\gs-widget`; macOS preferences remain beside the active database.

Window positions are saved separately in `window.txt` on Windows and `window-macos.txt` on macOS, so Retina scaling does not affect the Windows layout. The app version (for example `1.0.0`) is separate from the catalog's game data version (for example `V3.4220`).

The bundled pictures work without the game. Optional battle animations need additional client assets; set `GERSANG_CLIENT` to the client root or put that path in `%LOCALAPPDATA%\gs-widget\client.txt` on Windows, or `client.txt` beside the active database on macOS.
