# Windows build and verification

## Build

Use Rust 1.92 or newer and Visual Studio Build Tools with **Desktop development with C++**, including the Windows SDK (`rc.exe`). Keep the repository's `data` directory alongside the project; the executable searches upward for `data/gersang.db`.

In PowerShell, from the repository root:

```powershell
rustc --version
rustup show active-toolchain
cargo build --locked --release --manifest-path widget/Cargo.toml
.\widget\target\release\gersang-widget.exe
```

The normal Windows Rust installation uses the MSVC toolchain. If a custom toolchain is active, use the matching MSVC toolchain already installed on that Windows machine. The build embeds the existing `ui/app.ico` through `winresource`; it does not require a separate packaging script.

For automated checks:

```powershell
cargo test --locked --manifest-path widget/Cargo.toml window::tests
cargo test --locked --manifest-path widget/Cargo.toml
```

`battle_frames_skip_blanks_and_reuse_the_decoded_frame` requires the optional M96 battle sprite. If those game assets are absent, run the remaining suite explicitly:

```powershell
cargo test --locked --manifest-path widget/Cargo.toml -- --skip battle_frames_skip_blanks_and_reuse_the_decoded_frame
```

## Verification status

The Windows changes were prepared on macOS. Source review, shared geometry tests, and a host-side Rust metadata check do not replace a Windows build, Windows library linking, or an actual Win32 runtime test. The checks below remain the Windows verification procedure.

No additional DPI manifest or window subclass was added: the installed Slint 1.18.1 / winit 0.30.13 backend already enables per-monitor DPI awareness, applies window-level changes, and handles frameless resizing. Tooltips use Slint's built-in control. Windows retains Microsoft JhengHei UI; system font fallback supplies other supported glyphs. Actual emoji rendering depends on the fonts available on that Windows installation.

## Manual checks

1. **Catalog and window modes.** Open a monster and an item, search in Chinese and English, follow a related item, then use Full / 完整, Widget / 小窗, and the **–** collapse control. Expand again and confirm the category, selected record, and saved size remain useful. In Legacy mode, confirm the drop names wrap as inline links and the floating region menu switches lists without returning to the index. The collapsed bar is not an OS-minimized window.

2. **Size and scaling.** Check full mode at 1280 × 800 and 960 × 640 logical pixels. Resize below 960 logical pixels wide or 640 high: existing behavior switches back to widget mode. Check 100%, 150%, and 200% display scaling where available. Text, rows, drop icons, and hover targets should remain aligned. A display whose usable area is smaller than the full-mode minimum cannot contain that minimum size.

3. **Multiple monitors.** Put a secondary monitor to the left or above the primary display in Windows display settings. Move the app there, resize it, close it, and reopen it in both widget and full modes. It should restore on that display, including negative desktop coordinates. Repeat with different scaling on the two displays. Expand near a screen edge and confirm the title controls remain reachable and the taskbar is excluded. Disconnect the saved monitor and reopen: the window should move into the nearest available work area. Windows geometry is stored in `data/window.txt` beside the active database.

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
