# Gersang

A bilingual item and monster reference for Windows and macOS. You do not need the game installed.

The catalog is `data/gersang.db`. Pictures are in `data/client`.

## Windows

Install:

1. [Git](https://git-scm.com/download/win)
2. [Rust](https://rustup.rs), version **1.92 or newer**. Open a new terminal and check with `rustc --version`. If it is older, run `rustup update stable`.
3. [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/), with the **Desktop development with C++** workload. Rust needs that linker, and the build also needs `rc.exe` from the Windows SDK.

Clone, build, and open:

```
git clone https://github.com/coreyjj1679/gs-widget.git
cd gs-widget\widget
cargo build --release
target\release\gersang-widget.exe
```

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

The first build downloads dependencies and can take a few minutes. Keep the repository's `data` folder in place: the program searches upward from the current directory and executable for `data/gersang.db` or `gersang.db`. You can also set `GERSANG_DB` to the catalog file's path. Preferences are saved beside that database.

Window positions are saved separately in `window.txt` on Windows and `window-macos.txt` on macOS, so Retina scaling does not affect the Windows layout.

The bundled pictures work without the game. Optional battle animations need additional client assets; set `GERSANG_CLIENT` to the client root or put that path in `data/client.txt`, as on Windows.
