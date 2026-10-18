# MCP connection page

Scope: an ordinary extension of the native Rust/Slint app, exposed only at the bottom of full-mode navigation. Users connect the installed catalog to Codex, Cursor, Claude Desktop or Claude Code. Windows is the primary user platform; macOS is the available local preview host.

## Direction contract

THESIS: Make the next connection step explicit and copyable on the user's own computer.

WORLD: Inherit the existing Gersang full-mode desktop shell, DesktopTheme semantic colors, bronze selection, platform UI fonts, and native input controls. PRODUCT.md and DESIGN.md govern this extension; no brand redesign or approved image comp applies.

MODE: Read/Operate. Select an agent, read three steps, copy actual configuration. Configuration is product-relevant technical content; users need it to connect.

FIRST VIEWPORT: MCP title and short purpose, three agent tabs, local/read-only status, setup steps and the first usable copy action. Long JSON and details scroll independently of navigation.

INTERACTION: Exclusive tabs and Claude Desktop/Code choice; keyboard focus with Enter/Space; visible copy success/failure; Escape returns home. Full-mode exit removes the MCP page. No decorative motion.

RESPONSIVE: Preserve the 1280×800 and 960×640 logical desktop layouts, both Chinese and English. Windows configuration includes native .exe paths, JSON/TOML escaping and PowerShell quoting. Do not alter compact-widget styling or existing quest work.
