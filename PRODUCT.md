# Gersang reference

<!-- impeccable:product-schema 1 -->

## Platform

Native desktop on Windows and macOS, built with Rust and Slint. This is not a website or mobile app.

## Product Purpose

A bilingual reference for the game 巨商 / Gersang. Full mode supports browsing the catalog and reading details, comparing monsters, drops and equipment, and following quests, recipes and production information. The user confirmed all three workflows matter.

## Operating Context

The app has a full desktop mode, a compact widget mode, and a collapsed floating category bar. The user considers widget mode acceptable; the current redesign focuses on full mode.

## Capabilities and Constraints

- Preserve the local catalog, search, favourites, links, filters, category order, keyboard navigation and settings.
- Keep the shared Slint UI compatible with both Windows and macOS.
- Preserve the compact widget's appearance and interaction model.
- Default full window is 1280 × 800 logical pixels; it must remain usable at 960 × 640.
- Use the existing platform fonts: Microsoft JhengHei UI on Windows and PingFang TC on macOS.
- No game installation is needed for the bundled reference. Optional battle animation assets are not included.

## Evidence on Hand

`data/gersang.db` supplies real catalog records, `data/client` supplies game images, and `widget/ui/app.slint` supplies the working UI and interactions. Do not invent usage statistics or catalog counts.

## Product Principles

- Make categories, lists and details easy to scan together.
- Preserve context while following related records.
- Let game assets identify the content while interface controls stay consistent.
- Keep both desktop operating systems and both window modes working.
