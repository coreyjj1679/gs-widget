# MCP：連接 Codex、Cursor 與 Claude

在巨商圖鑑切換到 **完整 / Full** 模式，按左側最底部的 **MCP**。選擇你的 AI 程式，再按頁面的步驟連接。頁面會按目前安裝位置產生設定，毋須自己修改範例路徑。

Version 1.1.0 and later include a local stdio MCP server in the existing executable. Open **Full → MCP** for installation-specific copyable settings. Older releases do not include MCP.

## Windows 用戶

先把 ZIP 完整解壓縮到固定位置，再開啟 `gersang-widget.exe`。MCP 已包含在同一個執行檔，不需要另外安裝 Node.js、Python 或額外的 MCP 程式。

頁面會自動填入 Windows 絕對路徑及 `.exe` 副檔名，JSON／TOML 內會自動把 `\` 轉義成 `\\`；直接複製整份設定即可。Codex GUI 的 Command 欄使用普通路徑，Arguments 另外填 `--mcp`，不要把兩者拼成一行或自行加引號。

產生的設定適用於 **Windows 原生** AI 程式。Codex 如已切換成 WSL agent，請切回 Windows native 再使用此設定；Cursor Remote SSH／WSL 及其他遠端環境亦不會直接共用本機路徑。[官方 Windows 環境說明](https://learn.chatgpt.com/docs/windows/windows-app)

## Codex 桌面版

1. 開啟 **Settings → MCP servers → Add server**，名稱填 `gersang`，類型選 **STDIO**。
2. 從巨商的 MCP 頁複製 **Command**（完整執行檔路徑）及 **Arguments**（`--mcp`）。在環境變數加入 `GERSANG_DB`，值使用頁面上的資料庫路徑。
3. 儲存並重新啟動伺服器，開一個本機對話，確認 `gersang` 工具可用。

也可以按「改用 config.toml 設定」，把產生的區塊合併至 `~/.codex/config.toml`（Windows：`%USERPROFILE%\.codex\config.toml`）。若已有 `mcp_servers.gersang`，更新該區塊，避免重複定義；保留其他伺服器與設定。桌面版、CLI 與 IDE extension 使用相容的 MCP 設定。[Codex 官方 MCP 說明](https://developers.openai.com/codex/mcp)

## Cursor 桌面版

1. 在 Cursor 開啟全域設定檔 `~/.cursor/mcp.json`（Windows：`%USERPROFILE%\.cursor\mcp.json`）；沒有就建立。
2. 從巨商的 **MCP → Cursor** 複製設定。若檔案已有 `mcpServers`，只把 `gersang` 那一項合併進去，保留其他項目。
3. 儲存，在 **Customize → MCPs** 或該版本的 MCP 設定頁啟用 `gersang`，再在 **Agent** 對話使用。

專案級 `.cursor/mcp.json` 可以覆蓋同名全域設定；若連到舊路徑，檢查專案設定。[Cursor 官方 MCP 說明](https://cursor.com/help/customization/mcp)

## Claude Desktop

1. 開啟桌面程式的 **Settings → Developer → Edit Config**，使用它實際開啟的 `claude_desktop_config.json`。
2. 從巨商的 **MCP → Claude → Claude Desktop** 複製設定，把 `gersang` 合併至 `mcpServers`，保留其他設定。
3. 完全結束並重新開啟 Claude Desktop，在對話工具中啟用 `gersang`。

一般位置為 macOS 的 `~/Library/Application Support/Claude/claude_desktop_config.json`，Windows 的 `%APPDATA%\Claude\claude_desktop_config.json`；不同安裝渠道可能不同，優先用 **Edit Config** 找到檔案。[MCP 官方本機連線教學](https://modelcontextprotocol.io/docs/develop/connect-local-servers)

## Claude Code

在 **MCP → Claude → Claude Code** 複製指令，在同一台電腦的 Terminal 或 Windows PowerShell 執行一次。指令以 user scope 加入設定，路徑已處理空格及引號。重新啟動 Claude Code，以 `/mcp` 檢查 `gersang`。[Claude Code 官方說明](https://code.claude.com/docs/en/mcp)

## 可以問甚麼？

- 「用巨商 MCP 查詢 SSA 有哪些怪物會掉落，並列出出沒地點。」
- 「比較這兩隻怪的體力、抗性、經驗與掉落。」
- 「找出這個任務的前置任務和後續分支。」
- 「做 10 件這個裝備需要多少材料？按我提供的單價計算成本。」

工具包括 `catalog_info`、`search_catalog`、`get_entity`、`find_item_drops`、`get_quest_dependencies`、`get_recipes` 及 `calculate_recipe`。先搜尋取得正式 key／ID，再查詳情。分頁結果提供 `next_offset`，每次最多 50 筆；怪物詳情另外提供可比較的原始數值。

## 使用範圍

- AI 客戶端自動啟動 `gersang-widget --mcp`，巨商視窗可以關閉。不需安裝額外 runtime，MCP 也不會向 AI 供應商另行呼叫模型 API。
- 只讀取本機圖鑑及既有別名；不遷移設定、不修改收藏、不保存計算文件、不操控遊戲，也不提供任意 SQL／檔案存取工具。
- MCP 不會打開網路監聽埠或檢查更新。AI 客戶端自身仍按其設定處理送入對話的查詢結果。
- 圖鑑是離線快照，成功結果附帶 `catalog_version`。沒有即時市價，沒有記錄的掉落率不作推測。
- `find_item_drops` 提供掉落率：`drop_rate` 是 0～1 的機率、`drop_rate_percent` 是百分比、`drop_rate_per_100000` 是原始十萬分率，例如 `5500`＝`0.055`＝`5.5%`。未知值為 `null`；`drops` 保留各欄位及單人限定條件。同一怪物有多個掉落欄位時，頂層掉落率為 `null`，請讀取 `drops`，不把多個欄位推算為綜合機率。
- 成本只計直接材料及製作費；未填單價保持未知，明確填 `0` 才代表免費。不計學習費及失敗重試；不完整／強化配方會標示無法計算。
- 這是同機桌面／CLI 連接。Claude 網頁版的自訂 connector 需要雲端可到達的遠端 MCP，不能直接使用本機執行檔。[Claude connector 官方說明](https://support.claude.com/en/articles/11176164-use-connectors-to-extend-claude-s-capabilities)

## 排解連線問題

- **找不到執行檔**：重新開啟巨商的 MCP 頁，複製目前路徑；保持執行檔和 `data` 資料夾完整。Windows ZIP 要先完整解壓縮。
- **資料庫錯誤**：確認 `GERSANG_DB` 指向存在的 `gersang.db`。明確設定了錯誤路徑時，伺服器會退出並把錯誤寫到 stderr，不會悄悄改用另一份資料。
- **沒有工具**：儲存設定後重啟該伺服器或 AI 程式，開新對話，確認用的是本機 Agent 且 `gersang` 已啟用。不要把本機指令貼到遠端 URL 欄。
- **Windows 更新被阻擋**：MCP 和圖鑑視窗共用安裝鎖，避免更新時替換使用中的資料庫。更新前在每個 AI 程式中中斷 `gersang`，更新完再連接。
- **別名或資料沒有更新**：MCP 每次啟動讀取一份快照；重新啟動 MCP 以載入最新資料。

## Development verification

```sh
cargo test --locked --manifest-path widget/Cargo.toml mcp
cargo build --locked --manifest-path widget/Cargo.toml
python3 scripts/smoke_mcp.py --exe widget/target/debug/gersang-widget --database data/gersang.db
```

The smoke test launches the actual binary with redirected stdin/stdout from an unrelated working directory, checks all seven tools and four protocol versions, opens concurrent clients, checks EOF shutdown and missing-data startup, and verifies that catalog/preferences remain unchanged. Use `--packaged` to additionally check database discovery beside the executable. The Windows release workflow runs it against the extracted ZIP, including the GUI-subsystem binary's inherited stdio handles.

The server uses the official Rust MCP SDK (`rmcp`); diagnostics go to stderr and stdout is reserved for protocol messages. Input schemas reject unknown fields, queries and pages are bounded, and tool failures return readable `isError` results.
