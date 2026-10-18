# Windows releases, updates, and verification

## Install and update

Download `gs-widget-v1.0.0-windows-x64.zip` (or a newer version) from [GitHub Releases](https://github.com/coreyjj1679/gs-widget/releases/latest). Extract the whole archive into a writable folder such as `%LOCALAPPDATA%\Programs\gs-widget`, then open `gersang-widget.exe`. The ZIP contains the executable, `data/gersang.db`, `data/client-version.txt`, and the bundled `data/client` pictures. GitHub's automatically generated **Source code** ZIP is not the runnable app.

Each launch checks for updates in the background and shows a notice at the bottom left of the navigation bar when a newer stable version is available. Offline or failed startup checks remain quiet and do not affect the local catalog, pictures or personal settings. Open **Settings → App / 設定 → 程式** to see the installed app version or check manually; a failed manual check displays an error and a retry option.

Choose **Download update / 下載更新** when you want the new version, then **Restart and update / 重新啟動並更新** when it is ready. Browsing remains available during checks and downloads. A restart always requires that final button press. The updater checks the Release asset's SHA-256 digest returned by GitHub over HTTPS before accepting the ZIP; it does not require Git or Rust on the user's computer.

Favourites, aliases and preferences live under `%LOCALAPPDATA%\gs-widget`, outside the installed app. An existing installation's settings are migrated from beside the active catalog on first launch; existing destination files take precedence and source files are retained. Do not delete the old `data` folder until you have launched the new app and checked your settings. Close the app before copying the profile folder as a backup. The app and game catalog have separate version numbers.

Use a normal writable folder rather than `Program Files` for this portable ZIP. If the app cannot write to its installation directory, the update reports an error; move the extracted app to a writable folder and retry. A GitHub/network error leaves the current installation usable. The app has no GitHub token embedded; a public repository and publicly downloadable Release assets are required for the update check.

The updater temporarily keeps the previous executable and catalog assets in `.gs-widget-update-<id>/backup` beneath the installation directory. File replacement failures trigger rollback. If rollback cannot finish (for example, another process still locks a file), the error reports the recovery directory. Preserve a copy of that directory, close every running app copy, and follow `RECOVERY.txt`: actual files present in `backup` take precedence over `journal.json`, which can be one move behind after an interruption. Restore only those backed-up program/catalog units; do not delete an installed unit just because its backup is absent. Replace `data/client` as a whole when restoring it, but never replace the entire `data` folder or `%LOCALAPPDATA%\gs-widget`. Keep recovery files until the app is working again.

## Publish a version

The workflow is `.github/workflows/windows.yml`. Only a **push of a `v*` tag** triggers it; ordinary branch pushes (including `main`, feature and development branches), pull requests, and manual dispatch do not. Before building, it fetches complete history and verifies the checked-out tag's commit is already part of `origin/main` using Git ancestry. A tag on an unmerged feature commit fails the gate. A tagged ancestor of the current `main` tip is eligible, including an annotated tag.

An eligible tag builds and tests Windows x64, creates the ZIP, checks its contents and PE imports, runs the extracted app's headless installation check and offline GUI smoke check, and stores a `windows-x64` Actions artifact for 14 days. The GUI check uses software rendering and a refused loopback proxy; it requires a live window and initialized alias profile, then closes the app normally. The publication job rechecks the tag against the built commit and the freshly fetched `origin/main` before creating the Release.

For the first release, review and validate the implementation from `feat/full-mode`, ensure `widget/Cargo.toml` contains `version = "1.0.0"` and `widget/Cargo.lock` contains the matching package version, then merge and push that work to `main`. Tag only after the release commit is present on the remote main branch:

```sh
git switch main
git pull --ff-only origin main
git tag v1.0.0
git push origin v1.0.0
```

For `v1.1.0`, update `widget/Cargo.toml` to `1.1.0`, refresh and commit `widget/Cargo.lock` with the change, run the local checks below, and merge and push the changes to `main` before tagging that release commit as `v1.1.0`. The tag-triggered workflow runs all Windows checks before publication. Use all three version components: `v1.1.0`, not `v1.1`. Tag/version mismatches and prerelease versions fail validation. To check ancestry locally after fetching the remote branches and tags, run `python scripts/check_release_ref.py --tag v1.1.0` with that tagged commit checked out; this command verifies eligibility without publishing anything.

The workflow uses Rust **1.92.0**, `--locked`, the MSVC x64 target and a static C runtime. Python **3.12** performs packaging and publication. The ZIP is named `gs-widget-v<version>-windows-x64.zip`; an explicit file allowlist prevents personal preferences, aliases, source code, caches and game notice files from being included. The tests skip only the existing optional `battle_frames_skip_blanks_and_reuse_the_decoded_frame` test, which needs an unbundled game sprite.

The release job uses GitHub Actions' built-in token with `contents: write`; no PAT or extra signing secret is needed. The Release title is exactly the tag and its description is empty (`generate_release_notes: false`). It creates a draft, uploads the ZIP, checks GitHub's SHA-256 asset digest against the local archive, and only then publishes. Existing releases are never overwritten. If a job fails after creating a draft, inspect that draft and the failed run, delete the incomplete draft if appropriate, and rerun the job. Do not replace an already published version's assets; publish a new patch version instead. GitHub's API digest verifies the downloaded bytes; the EXE is not Authenticode-signed.

Relevant API contracts: [creating and updating Releases](https://docs.github.com/en/rest/releases/releases), [Release asset metadata and digests](https://docs.github.com/en/rest/releases/assets), and [Rust static C runtimes](https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes).

## Build

Use Rust 1.92 or newer and Visual Studio Build Tools with **Desktop development with C++**, including the Windows SDK (`rc.exe`). Keep the repository's `data` directory alongside the project; the executable searches upward for `data/gersang.db`.

In PowerShell, from the repository root:

```powershell
rustc --version
rustup show active-toolchain
cargo build --locked --release --manifest-path widget/Cargo.toml --target x86_64-pc-windows-msvc
.\widget\target\x86_64-pc-windows-msvc\release\gersang-widget.exe
```

The normal Windows Rust installation uses the MSVC toolchain. If a custom toolchain is active, use the matching MSVC toolchain already installed on that Windows machine. The build embeds the existing `ui/app.ico` through `winresource`. The repository's `.cargo/config.toml` enables the static C runtime for this target, and the packager rejects a binary that still imports the Visual C++ runtime DLLs.

For automated checks:

```powershell
cargo test --locked --manifest-path widget/Cargo.toml --target x86_64-pc-windows-msvc window::tests
cargo test --locked --manifest-path widget/Cargo.toml --target x86_64-pc-windows-msvc
```

`battle_frames_skip_blanks_and_reuse_the_decoded_frame` requires the optional M96 battle sprite. If those game assets are absent, run the remaining suite explicitly:

```powershell
cargo test --locked --manifest-path widget/Cargo.toml --target x86_64-pc-windows-msvc -- --skip battle_frames_skip_blanks_and_reuse_the_decoded_frame
```

To package locally, install Python 3.11 or newer, then run from the repository root after the release build:

```powershell
python -m unittest discover -s scripts/tests -v
python scripts/package_windows.py --expected-tag v1.0.0
```

The resulting ZIP is in `dist`. For a non-publishing smoke test, extract it to a temporary directory, set `LOCALAPPDATA` to a separate temporary profile, and run its EXE with `--check-installation` from a different working directory. This checks the packaged catalog and assets without creating a GUI. The automated workflow does this with `Start-Process` and checks the exit code.

To run the offline GUI check locally with PowerShell 7, use a temporary profile and an extracted release:

```powershell
.\scripts\smoke_windows_offline.ps1 -ExePath C:\Temp\gs-widget\gersang-widget.exe -LocalAppData C:\Temp\gs-widget-test-profile -WorkingDirectory C:\Temp
```

The script temporarily sets `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` to `http://127.0.0.1:1`, clears `NO_PROXY`, and selects Slint's `winit-software` backend. It waits up to 30 seconds for the normal window and `aliases.txt`, verifies the app remains alive and responsive for another 35 seconds (covering the update check's 30-second timeout), then requests a normal close. Every wait is bounded; cleanup kills a stuck process and restores the caller's environment. This is an offline startup/responsiveness check, not a visual layout or complete update-installation test. A Windows desktop session is required for local execution.

## Isolated dummy update test: v1.0.0 → v1.0.1

`.github/workflows/windows-update-smoke.yml` is a separate, non-publishing Windows test. It runs only for pushes to `codex/test-windows-update`, or manual dispatch once the workflow is available on the default branch. The production `windows.yml` release trigger is unchanged. The test downloads the published v1.0.0 ZIP and checks its pinned SHA-256, changes the application version to 1.0.1 only in the disposable CI checkout, then builds and packages that candidate. It creates no tag, Release or public update asset; existing users continue to see the real published version.

The smoke script uses the **published v1.0.0 executable** as the update helper, an actual running v1.0.0 GUI as its parent, a complete staged payload manifest, and an isolated `LOCALAPPDATA` profile. It checks tampered-payload rejection, normal app shutdown, managed-file replacement, backup contents, actual v1.0.1 GUI restart, and preservation of personal files. Test installation paths contain spaces. Both GUIs use a refused network proxy and software rendering. The Actions evidence artifact records results and hashes for 14 days. Separate build and smoke jobs hand off the dummy ZIP through an explicitly labelled test-only Actions artifact retained for one day, allowing a failed smoke job to be rerun against the same build. This ZIP is never published as a Release.

Harness-only changes can reuse the pinned candidate from the verified build while that artifact exists and the application, data and packager sources are identical. Its producing commit and artifact digest must match; expiry or a source change triggers a fresh build. The fixture starts in widget mode to avoid a hosted desktop's size constraints automatically switching a full-mode window; remembered full-mode geometry is still checked along with all other personal files.

To repeat locally in a Windows PowerShell 7 desktop session, supply a downloaded baseline and a separately built dummy candidate. The test root must be a new disposable directory:

```powershell
.\scripts\smoke_windows_update.ps1 -BaselineZip C:\Temp\gs-widget-v1.0.0-windows-x64.zip -CandidateZip C:\Temp\gs-widget-v1.0.1-windows-x64.zip -TestRoot "C:\Temp\gs widget dummy update"
```

This exercises the released helper and real process restart. It stages the candidate directly, so it does **not** test discovery of a newer public Release or the GUI download/restart buttons. Release selection, archive validation, rollback and locking also run in the Rust test suite. Complete the following acceptance test against the next genuine published version to verify the full public-feed and button flow.

## Update acceptance test: v1.0.0 → v1.1.0

Use a Windows x64 test account or VM with a backed-up profile. Successful local or hosted compilation does not replace this runtime test.

1. Download the published `v1.0.0` ZIP, extract it to a folder whose name contains a space, and launch it without any development tools installed. Confirm Chinese and English content and pictures load.
2. Save several monsters, create an alias, switch language and shortcut/pin settings, move and resize the window, then close and reopen it. Confirm those choices are saved under `%LOCALAPPDATA%\gs-widget`.
3. Publish `v1.1.0` using the same workflow. Launch the installed **v1.0.0** app and confirm its background check shows a notice at the bottom left of the navigation bar. Open that notice, download the update, and continue browsing. Also check the manual **Settings → App** flow. Confirm the downloaded version is `1.1.0` and the app does not restart before you press the restart button.
4. Restart and update. Verify the reopened app shows `1.1.0`, the catalog and pictures load, and all favourites, aliases, language, shortcuts, pin mode, window position and view preferences remain. Restart once more and check again.
5. Launch while offline: the background check must stay quiet while local records, pictures, favourites and settings work normally. Manually check while offline and confirm an error is shown. Also disconnect during download and retry after reconnecting; no partial download should become installable. A missing/invalid digest or malformed package must fail before replacing the installation (covered by automated tests as well).
6. In a disposable test install, prevent a file replacement with a second process holding a catalog or EXE open, then attempt the update. Confirm the helper reports failure and the old version and profile remain available. Release the lock, reopen the app and retry. Never perform deliberate corruption or lock tests on a profile without a backup.
7. For migration, prepare an older install with favourites, aliases and settings beside `data/gersang.db`, and use a fresh temporary `LOCALAPPDATA`. Launch the new app against that catalog once. Confirm migrated values, unchanged old files, and that a second launch does not overwrite newly changed profile values. Also confirm an existing destination profile takes precedence.

## Verification status

The Windows changes were prepared on macOS. Source review, shared geometry tests, and a host-side Rust metadata check do not replace a Windows build, Windows library linking, or an actual Win32 runtime test. The checks below remain the Windows verification procedure.

No additional DPI manifest or window subclass was added: the installed Slint 1.18.1 / winit 0.30.13 backend already enables per-monitor DPI awareness, applies window-level changes, and handles frameless resizing. Tooltips use Slint's built-in control. Windows retains Microsoft JhengHei UI; system font fallback supplies other supported glyphs. Actual emoji rendering depends on the fonts available on that Windows installation.

## Manual checks

1. **Catalog and window modes.** Open a monster and an item, search in Chinese and English, follow a related item, then use Full / 完整, Widget / 小窗, and the **–** collapse control. Expand again and confirm the category, selected record, and saved size remain useful. In Legacy mode, confirm the drop names wrap as inline links and the floating region menu switches lists without returning to the index. The collapsed bar is not an OS-minimized window.

2. **Size and scaling.** Check full mode at 1280 × 800 and 960 × 640 logical pixels. Resize below 960 logical pixels wide or 640 high: existing behavior switches back to widget mode. Check 100%, 150%, and 200% display scaling where available. Text, rows, drop icons, and hover targets should remain aligned. A display whose usable area is smaller than the full-mode minimum cannot contain that minimum size.

3. **Multiple monitors.** Put a secondary monitor to the left or above the primary display in Windows display settings. Move the app there, resize it, close it, and reopen it in both widget and full modes. It should restore on that display, including negative desktop coordinates. Repeat with different scaling on the two displays. Expand near a screen edge and confirm the title controls remain reachable and the taskbar is excluded. Disconnect the saved monitor and reopen: the window should move into the nearest available work area. Windows geometry is stored in `%LOCALAPPDATA%\gs-widget\window.txt`.

4. **Window level and hit testing.** In Automatic pin mode, the widget stays above an ordinary application and full mode behaves as a normal window. Verify Always and Never pin settings. Drag by the title area, resize all four edges and corners, and click controls beside those areas. Clicking a drop icon or a window button must not start dragging the window; normal clicks inside the app must not pass through to another application.

5. **Drop tooltips.** Hover several drop icons, including icons near the right and bottom of the window. Confirm the correct item name is readable, the tooltip disappears when the pointer leaves, and clicking the icon still opens the item. Repeat after scrolling, changing display scale, switching full/widget modes, and changing pin settings. The tooltip must not steal keyboard focus or obstruct the next click.

6. **Hotkeys.** With another ordinary application focused, press F1–F9 to open the corresponding visible categories. Holding a key should not repeatedly trigger it. Disable shortcuts, confirm those keys no longer activate the widget, then enable them again; repeat soon after launching. A shortcut already registered by another application may be unavailable. Use the category buttons in that case. Also minimize the widget through Windows, then press a registered shortcut: the window should restore. If two widget instances are open, an instance must not focus the other one merely because both have the same title.

7. **Clipboard and fonts.** Copy a detail containing Chinese text and line breaks, then paste into Notepad and compare the content. Check Chinese/English headings, stars, arrows, and any emoji in both modes for missing-glyph boxes or clipped baselines. Emoji appearance may differ from macOS; the game images should remain the original assets.

8. **Monster page controls.** Open the gear beside monster search and switch Legacy, portrait/battle, and animation settings. These controls belong to the monster page in full mode. Open Filters, search for a drop item, and select a suggestion; confirm its monsters appear even before choosing a region. Switch between the current region and all monsters, then combine a monster name, element, and drop. Check that clearing filters works, the dropdown closes with Escape or an outside click, and returning from an item keeps the selected region.

9. **Advanced filters and ordering.** Select the official Thunder element icon, sort by ELA or either resistance, and reverse the direction. Check that values follow the selected numeric order even when some monsters are favourites. Combine hit and magic resistance ranges under Advanced, then collapse the filters to recover list space. Repeat in Legacy and search results. Switch Chinese-first / English-first and confirm the main and secondary monster names exchange languages; long names must remain available through hover text without covering the statistics.

10. **Region directory and drop shortcuts.** In ordinary full mode, use the pinned All / Saved entries, choose a country, and search for a region in either language. Directory search should leave the current monster results unchanged until a region is selected. Use Tab, arrow keys and Enter in the directory. Right-click a drop icon, a drop in the expanded monster details, or a Legacy drop name and choose Find monsters with this drop: the page should show every matching monster across regions, clear previous name/element/resistance limits, and retain sorting. Test Menu or Shift+F10, Escape, outside-click dismissal, and confirm ordinary left-click still opens the item.

When reporting a problem, include the Windows version, display arrangement and scaling, full/widget mode, pin setting, steps, and a screenshot. Include the failing build or test output for compiler errors.

## Native fixes in this change

- Clipboard writes now use the current process's widget window as owner. Passing NULL to `OpenClipboard` before `EmptyClipboard` causes `SetClipboardData` to fail under the documented contract. [Microsoft OpenClipboard documentation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openclipboard)
- The hotkey worker creates its message queue before publishing its thread ID, so a settings update cannot post to a missing queue. A compile-time check preserves the SDK's `MSG` layout: 48 bytes on 64-bit Windows and 28 on 32-bit Windows (`lPrivate` is guarded by `_MAC` in the SDK). [PostThreadMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postthreadmessagew), [Microsoft Windows SDK header](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/RecompiledIdlHeaders/um/WinUser.h#L1746)
- Window lookup checks the process ID, minimized windows are restored before foreground activation, and saved positions are fitted to their target monitor's work area. After moving across displays, fitting uses the actual native frame size so DPI changes do not leave the window below the taskbar. Windows foreground restrictions still apply. [SetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow)
