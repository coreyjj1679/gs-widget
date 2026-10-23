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

優先使用 `query_catalog`，把玩家原句直接放進 `query`，例如 `{"query":"雀天要點合？"}`。伺服器會辨識已收錄的術語和取得方式問法，在一次呼叫中回傳持國天王、前置角色、直接材料及原始條件，不必先由 AI 猜出正式名稱。`action` 可指定 `overview`、`acquisition`；已知多個 key 時可用 `keys` 一次查最多 8 個對象，或用 `action: "compare"` 比較至少兩個對象。`language` 支援 `zh`、`en`、`ko`。

回應的 `status` 區分 `resolved`、`ambiguous`、`not_found`。有歧義時只列 `candidates`，由使用者選擇 key；不把模糊搜尋的第一筆當成答案。`resolution` 說明命中的術語與意圖，`match_source` 說明實際使用正式名稱、內建術語或使用者別名。未知的高階／改造修飾不會自動降級成本體。這是可審核的字詞比對，並非通用自然語言理解；未收錄的問法仍可能需要正式名稱或 key。

原有七個工具 `catalog_info`、`search_catalog`、`get_entity`、`find_item_drops`、`get_quest_dependencies`、`get_recipes` 及 `calculate_recipe` 保留相容，供完整分頁資料、詳細條件及成本計算使用。分頁結果提供 `next_offset`，每次最多 50 筆。精確道具別名優先回傳指定成員，例如 SSA 不再混入英文名稱偶然含有 SSA 的其他道具。`query_catalog` 的 `limit`（預設 5、最多 10）限制候選與來源摘要；請依 `truncated`、`remaining_keys` 及 `next_call` 取得其餘記錄；需要先選擇的呼叫另有 `requires_choice:true`。

### 完整關聯、分段內容與篩選

新增 `get_relations` 後共有九個工具。用 `key`、`relation`、`offset`、`limit` 查完整關聯，每頁最多 50 筆；`total` 是該關係的記錄數，`next_call` 可直接用於下一頁，末頁為 `null`。`get_entity.related` 仍是最多 50 筆的摘要，但 `related_total`、`related_truncated` 會反映真正的記錄數；`relations` 提供各關係的完整查詢入口。

- 道具：`shops`、`production`、`quest_rewards`、`uses`、`recipes`、`drops`、`sets`、`characters`。`uses` 是「這件材料用在哪裡」，不是「如何製作它」；配方／生產／任務／角色用途保留各自條件。
- 怪物：`drops`、`quests`；套裝：`members`；NPC：`quests`。
- 城鎮 `town:`：`goods`；設施 `bay:`：`products`；建築 `yard:`：`facilities`。所有這些 key 都能交給 `get_entity` 繼續查。
- 角色、任務、配方：`related`；任務前置關係仍用 `get_quest_dependencies`。

例如 `{"key":"item:20","relation":"shops","limit":20}` 可逐頁列完牛肉的全部商店；`{"key":"item:5507","relation":"uses","offset":200,"limit":20}` 可直接查看時間的粉末後面的用途。

`get_entity.sections` 可選 `overview`、`stats`、`skills`、`effects`、`related`、`production`，省略則保持完整內容。`stats` 適用角色／怪物，`skills` 適用角色，`effects` 適用角色／套裝，`production` 適用道具；不適用的組合會明確報錯。角色能力是數值或 `null`，技能保留名稱與來源描述，套裝效果保留件數／屬性／數值；生產資料保留勞動、產量範圍、材料與工具，未知資料不當作零。`query_catalog` 的 overview／compare 亦接受 `sections`；比較結果按欄位對齊數值，不推測不同資料類別的能力等價。

`search_catalog` 新增 `town`、`facility`、`building` 類別及 `filters`。先讀 `catalog_info.filters` 取得各類別可用值；篩選在分頁之前套用，`next_call` 保留篩選條件。

- `region`：城鎮、怪物、NPC 的來源地區；各類別支援值不同。
- `nation`：角色原始國籍代碼；`element`／`is_boss`：怪物屬性及首領標記。
- `item_category`／`item_subtype`：道具原始分類／子類代碼。
- `min_level`／`max_level`：配方記錄的最低等級範圍，排除未記錄等級；不是裝備穿戴等級。DB 未記錄裝備穿戴等級，對道具使用此篩選會明確報錯。

例如 `{"category":"town","filters":{"region":"japan"}}`，或 `{"category":"monster","filters":{"region":"china","element":1,"is_boss":false}}`。

### 減少 AI 往返與回應大小

`query_catalog` 會按明確問法縮小取得來源：「SSA 邊度打？」只查掉落；合成／製作查配方與生產。可用 `sources` 明確指定 `recipes`、`monster_drops`、`shops`、`production`、`quest_rewards`。一般取得問法仍列全部來源，英雄取得保留其直接條件。完整列表沿 `next_call` 取得，不把沒有出現在摘要中的資料當作不存在。

用 `{"query":"強化的高級白水晶","action":"calculate","quantity":10}` 可在一次呼叫內辨識道具並計算直接材料；`auto` 搭配 `quantity` 亦會選擇計算。`prices` 格式與 `calculate_recipe` 相同，沒有提供的價格仍是未知。若名稱或配方有多個選項，會回傳候選及帶 `requires_choice:true` 的下一步，選定後傳 `recipe_id`，不自行猜測配方。此計算不自動展開所有前置材料，也不扣除庫存或估算失敗重試。

工具提供 `outputSchema`、穩定的 `error_code` 與可執行的分頁／選擇參數；仍同時回傳文字 JSON 和 `structuredContent`，維持 MCP 客戶端相容性。`NOT_FOUND`、`INVALID_ARGUMENTS`、`UNSUPPORTED_QUERY`、`RESULT_TOO_LARGE` 與 `QUERY_FAILED` 分開表示不同失敗原因。

### 維護遊戲術語

[`data/terminology.json`](data/terminology.json) 是有來源記錄的詞彙表，與使用者 `aliases.txt` 分開。正式三語名稱直接來自 DB；社群俗稱由公開攻略、既有對應或玩家確認後批量整理，無須由每位使用者重做。每個詞條記錄可辨識的拼法、正式三語名稱、帶類別的 stable key、是否為道具群組，以及來源證據。使用者既有道具別名仍優先生效，不會被詞彙表覆寫。

正常發行版仍內嵌詞彙表；完整模式的「搜尋全部」也用內嵌詞彙表辨識同一批俗稱，外部詞彙檔目前只影響 MCP。要獨立更新，可把完整 `terminology.json` 放在 `gersang.db` 同一目錄，或用環境變數 `GERSANG_TERMINOLOGY` 指定檔案；重啟 MCP 即載入，不必重新編譯或修改 DB。指定檔案優先於 DB 旁的檔案；兩者均未配置時使用內嵌版本。這是完整詞彙包的替換，不是把零散詞條合併入內建版本，維護時應以目前完整檔案為底稿。檔案只在啟動時讀取和驗證，查詢時不讀檔或連網。`catalog_info.terminology` 可確認實際來源、詞條數和拼法數。

外部檔案上限 1 MiB，會驗證格式、版本、來源及目標 key。已配置但損壞／不存在的檔案會明確報錯；不會悄悄忽略自訂內容。刪除 DB 旁的選配檔案並移除環境變數可恢復內嵌版本。一般程式更新維持現有封裝格式，詞彙包可另行更新。

- `雀天` → `hero:T49`（持國天王）；`雀天套` → `set:66`；`雀天手` → `item:6240`。
- `龍天` → `hero:T51`（增長天王）；`明冥`／`明明` → `hero:T88`（大威德明王）。`明明` 也有日常用法，故使用 `subject_aliases`：只辨識獨立名稱或句首接明確問法，例如 `明明要點合？`，避免把 `我明明有材料` 當成角色。
- 社群來源另覆蓋日明／日冥、朝明／朝冥、台明／台冥、印明／印冥，以及 MP女、小燕、權2、龜車、祖3、雷3。來源網址與版本範圍保存在各詞條；例如 `祖3` 指覺醒憤怒羅漢，`龜車` 指基本龜甲車，不把稱呼自動套到其他形態。
- 覺醒角色、覺醒套裝及各裝備有獨立詞條，採用長詞優先比對，不與基本形態合併。
- `點合`、`合成`、`製作`、取得／掉落等問法標記為取得方式意圖。意圖標籤不代表 DB 一定有完整配方；沒有記錄的條件不推測。

新增詞條時，先確認正式名稱及 key，記錄玩家確認、既有程式對應或可信來源；地區、年代或形態不明的俗稱不猜測。不同目標共用俗稱時可列為獨立詞條，讓查詢回傳 `ambiguous` 候選；未收錄的俗稱回傳 `not_found`，不會從對話自動學習或寫入詞彙表。物品和角色即使 ID 相似也不能混用，必須保留 `item:`／`hero:` 等類別前綴。`cargo test --manifest-path widget/Cargo.toml terminology` 會驗證來源、重複詞條及目標是否存在。

## 使用範圍

- AI 客戶端自動啟動 `gersang-widget --mcp`，巨商視窗可以關閉。不需安裝額外 runtime，MCP 也不會向 AI 供應商另行呼叫模型 API。
- 只讀取本機圖鑑、內建或外部詞彙表及既有別名；不遷移設定、不修改收藏、不保存計算文件、不操控遊戲，也不提供任意 SQL／檔案存取工具。
- MCP 不會打開網路監聽埠或檢查更新。AI 客戶端自身仍按其設定處理送入對話的查詢結果。
- 圖鑑是離線快照，成功結果附帶 `catalog_version`。沒有即時市價，沒有記錄的掉落率不作推測。
- `find_item_drops` 提供掉落率：`drop_rate` 是 0～1 的機率、`drop_rate_percent` 是百分比、`drop_rate_per_100000` 是原始十萬分率，例如 `5500`＝`0.055`＝`5.5%`。未知值為 `null`；`drops` 保留各欄位及單人限定條件。同一怪物有多個掉落欄位時，頂層掉落率為 `null`，請讀取 `drops`，不把多個欄位推算為綜合機率。
- 成本只計直接材料及製作費；未填單價保持未知，明確填 `0` 才代表免費。不計學習費及失敗重試；不完整／強化配方會標示無法計算。
- 角色取得方式只列取得該形態的直接條件，不把下一階段材料混入，也不自動展開所有前置角色的總成本。材料清單空白代表未記錄，並非免費；需要時沿前置 key 繼續查詢。道具取得摘要列出製作、掉落、商店、生產及任務來源，保留各來源的範圍與截斷提示。
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

The smoke test launches the actual binary with redirected stdin/stdout from an unrelated working directory, checks all nine tools (including original-question acquisition, filtered search, complete relation paging, sectioned details, one-call calculation and batch comparison) and four protocol versions, opens concurrent clients, checks EOF shutdown and missing-data startup, and verifies that catalog/preferences remain unchanged. Use `--packaged` to additionally check database discovery beside the executable. The Windows release workflow runs it against the extracted ZIP, including the GUI-subsystem binary's inherited stdio handles.

The server uses the official Rust MCP SDK (`rmcp`); diagnostics go to stderr and stdout is reserved for protocol messages. Input schemas reject unknown fields, queries and pages are bounded, and tool failures return readable `isError` results.

### Measuring latency

```sh
python3 scripts/benchmark_mcp.py --exe widget/target/release/gersang-widget --database data/gersang.db --profile release --output /tmp/mcp-benchmark.json
```

Use the same build profile, database and machine for comparisons. This benchmark measures subprocess startup through the first catalog response, then repeated real stdio calls, result sizes and matched keys. It does not measure Cursor/model reasoning or tool selection, and does not flush OS file caches. Check retrieval correctness as well as milliseconds: a fast empty or noisy result can still cause several slow AI turns.

The server loads its catalog index once per process, caches tool schemas, normalized name lookups and the quest graph, and builds the calculator catalog lazily after input validation. Canonical matching uses a name index bounded by question length instead of scanning every catalog name on each call. Drop/recipe/material-use pages format only the requested window. It remains a single read-only local process; no extra model API, network service or vector database is required.
