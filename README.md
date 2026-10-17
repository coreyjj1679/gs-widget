# Gersang

A bilingual item and monster reference. Windows only. You do not need the game installed.

The catalog is `data/gersang.db`. Pictures are in `data/client`.

## Install

1. [Git](https://git-scm.com/download/win)
2. [Rust](https://rustup.rs), version **1.92 or newer**. Open a new terminal and check with `rustc --version`. If it is older, run `rustup update stable`.
3. [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/), with the **Desktop development with C++** workload. Rust needs that linker, and the build also needs `rc.exe` from the Windows SDK.

## Clone, build, open

```
git clone https://github.com/coreyjj1679/gs-widget.git
cd gs-widget\widget
cargo build --release
target\release\gersang-widget.exe
```

The first build downloads dependencies and can take a few minutes. The program finds `data/gersang.db` by walking up from the executable, so leave it in `widget\target\release`.
