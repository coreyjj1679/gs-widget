---
name: Gersang desktop reference
description: Slint 原生桌面全視窗圖鑑；深灰介面、bronze 重點色與原遊戲素材。
colors:
  text: "#edf1f7"
  secondary: "#a8b2c1"
  muted: "#929dad"
  accent: "#d8b77b"
  soft: "#c9d2df"
  border: "#303945"
  selected: "#33312c"
  hover: "#29313c"
  surface: "#1d232c"
  surface-alt: "#1a2028"
  sidebar: "#11151b"
  control: "#27303b"
  hover-strong: "#374251"
  inset: "#15191f"
  raised: "#303945"
  selection-low: "#292721"
  on-accent: "#211b12"
  chrome: "#15191f"
  canvas: "#15191f"
  detail: "#192029"
typography:
  body-windows:
    fontFamily: "Microsoft JhengHei UI"
    fontSize: "14px"
    fontWeight: 400
  body-macos:
    fontFamily: "PingFang TC"
    fontSize: "14px"
    fontWeight: 400
  title:
    fontSize: "21px"
    fontWeight: 600
  entry:
    fontSize: "15px"
    fontWeight: 600
  entry-prominent:
    fontSize: "17px"
    fontWeight: 600
  navigation:
    fontSize: "13px"
    fontWeight: 400
  navigation-selected:
    fontSize: "13px"
    fontWeight: 600
  label:
    fontSize: "12px"
  shortcut:
    fontSize: "11px"
rounded:
  window-control: "5px"
  navigation: "7px"
  surface: "8px"
  favourites: "12px"
spacing:
  tight: "4px"
  small: "8px"
  inset: "12px"
  row: "16px"
  page: "24px"
components:
  navigation-selected:
    backgroundColor: "{colors.selected}"
    textColor: "{colors.text}"
    typography: "{typography.navigation-selected}"
    rounded: "{rounded.navigation}"
    height: "40px"
  navigation-hover:
    backgroundColor: "{colors.hover}"
    textColor: "{colors.secondary}"
    rounded: "{rounded.navigation}"
    height: "40px"
  entry:
    backgroundColor: "transparent"
    textColor: "{colors.text}"
    typography: "{typography.entry}"
    rounded: "{rounded.surface}"
    height: "80px"
  entry-hover:
    backgroundColor: "{colors.hover}"
    rounded: "{rounded.surface}"
  detail-panel:
    backgroundColor: "{colors.detail}"
    rounded: "{rounded.surface}"
---

# Design System: Gersang

## Overview

這份文件從目前的 `widget/ui/desktop.slint`、`widget/ui/app.slint` 與 `PRODUCT.md` 抽取，記錄 Windows／macOS 共用的 Slint 原生桌面全視窗設計。介面以深灰底色、bronze 重點色及清楚的文字層級承載圖鑑資料；分類與內容辨識沿用原遊戲圖像。

適用範圍是 full mode。保留搜尋、收藏、分類排序、鍵盤操作、關聯連結及設定，也保留 compact widget 與收合分類列的現有外觀與互動。這裡的視覺描述來自實作，不代表額外取得的使用者品牌偏好。

## Colors

**Primary：** `accent` 用於重點文字、選取圖示與鍵盤焦點；實色重點按鈕使用 `on-accent` 文字。`selected` 與 `selection-low` 提供低亮度的暖色選取背景。

**Neutral：** `sidebar`、`canvas`、`surface-alt`、`surface`、`detail` 區分導覽、工作區、分類、列表與詳細資料。`text` 是主要內容，`secondary` 是說明，`muted` 是捷徑與次要資訊，`soft` 是較柔和的重點文字。`border` 用於分隔；`hover`、`control`、`hover-strong`、`raised` 表達互動狀態。

**模式隔離：** YAML 色票只記錄 `DesktopTheme.active == true` 的值。啟用條件維持 `full-mode && !collapsed`；共用元件使用語意色票，inactive 分支保留 compact widget 的原暖棕色與透明背景。不要把 full mode 色碼直接灌入所有模式。

## Typography

Windows 使用 Microsoft JhengHei UI，macOS 使用 PingFang TC，由 `ui-font` 控制；兩者是各平台選擇，不是混用的字體堆疊。預設內文為 14px／400，分類標題為 21px／600，入口標題為 15px／600，主要入口為 17px／600。導覽文字為 13px，選取時加粗至 600；說明與標籤多用 12px，捷徑用 11px。

保持中英文切換可讀。導覽與單行列表使用省略；說明、空狀態與詳細內容按可用寬度換行。不要為了塞入更多資料縮小主要內容字級。

## Layout

全視窗預設為 **1280 × 800 logical pixels**，最低目標為 **960 × 640**；不得把 logical pixels 當作 Retina 實體像素。採左側持續導覽與右側內容工作區。視窗寬度低於 1100px 時，側欄由 208px 改為 176px；full mode 頁面內距為 24px，列表／詳情分欄間距為 12px。

一般圖鑑並排呈現分類、可捲動列表與詳細資料；內容工作區低於 900px 時，分類欄由 180px 收為 150px，詳情欄由 320px 收為 260px，中間列表保留伸縮空間。怪物瀏覽保持一行一筆：名稱、類型／元素、六項戰鬥數值與掉落圖示同時可見，選取後在原列下方補充緊湊的屬性、出沒地與掉落連結。怪物目錄與內容間距為 24px，目錄列高 36px、列距 4px。分類搜尋常駐頁首，寬 280px；設定內容限制最大寬度為 688px。

尺寸不足時保留可操作的捲動區、必要欄位及完整文字提示，不套用網頁或手機導覽模式。全視窗／widget 切換、收合與視窗拖曳保留現有入口。

## Elevation & Depth

桌面 shell、導覽與入口元件沒有陰影；深度由表面明度、細分隔線、選取底色及 hover 狀態表達。原遊戲圖像提供內容辨識，不作背景紋理。新增區塊沿用這套層次，避免額外加入光暈、玻璃模糊或裝飾性陰影。

## Shapes

視窗控制使用 5px 圓角，導覽使用 7px，入口及一般面板使用 8px，總覽收藏區使用 12px。分隔線與焦點邊框為 1px。`DesktopIcon` 使用共用 24 × 24 viewbox、1.6px 描邊與透明填色；功能符號沿用此筆畫，分類圖示仍用遊戲素材。

## Components

- **DesktopNavRow：** 40px 高，左右內距 10px，圖示區 22px。選取顯示暖色底與粗體標籤；hover 顯示深灰底；焦點顯示 accent 邊框。Enter／Space 可啟動，停用時透明度為 0.5，保留 accessible label 與 enabled 狀態。
- **DesktopEntry：** 預設 80px 高、44px 遊戲圖像、文字與圖像間距 16px、細底線；hover 才出現面底色。總覽已使用 112px 主要入口與 88px 次要入口。標題與說明可省略，動作標籤只在元件寬度大於 380px 時顯示。
- **MonsterTableRow／InlineMonster：** 僅限 full mode。一行一筆，寬列高 128px、窄列高 160px，列距 8px；搜尋與收藏沿用。52px 頭像旁顯示名稱、類型及元素，右方以三欄兩列排列體力、魔力、攻擊、兩種抗性及經驗；下方保留掉落圖示與實際數量。每個掉落圖示旁顯示掉落率，寬列最多十個並排，窄列以五個一組換行；未知機率以「—」顯示，停留提示說明未記錄。圖示可直接開啟道具頁，滑鼠停留時顯示完整名稱與機率；缺圖仍保留原位置的可點擊替代圖示，避免道具連結錯位。體力與打擊抗性為暖紅，魔力與魔法抗性為藍；元素沿用小窗的官方屬性圖示，並保留對應的文字及柔和色彩。各數值始終保留文字標籤。展開詳情直接插在所選列下方，以 12px 內距及約 30px 的連結列呈現，實際高度隨換行量測，不加入大段空白。
- **列表／詳細資料：** 保留選取列與關聯記錄的上下文；資料、計數與圖片來自現有 catalog。未選取、收藏為空或沒有篩選結果時，說明下一個可執行動作。
- **搜尋／設定：** 沿用 Slint 原生 `LineEdit`／`TextEdit` 與現有選項控制。常駐搜尋限定目前分類；分類排序仍以拖曳與可見捷徑標籤呈現。

## Do's and Don'ts

- **Do** 先沿用 `DesktopTheme` 與現有元件，再新增必要變體；修改共用控制時同時確認 full mode、widget、collapsed 的分支。
- **Do** 使用 `data/client` 的原遊戲素材，缺圖時保留已有文字替代；測試中英文、1280 × 800 與 960 × 640，以及兩個桌面系統的字體。
- **Do** 保留鍵盤焦點、Enter／Space 啟動與既有分類捷徑；讓 selected、hover、focus 各自可辨。
- **Don't** 把裝飾放在表格、數值與關聯資料之前，或把 full mode 的側欄與間距套到 compact widget。
- **Don't** 編造圖鑑數量、使用數據或遊戲素材；可選戰鬥動畫缺失時，不把它描述成必備資產。

## Legacy mode

完整模式怪物頁右上角的小齒輪提供 `Legacy mode`、圖片模式與戰鬥動畫設定，沿用 view.txt 保存偏好；這些選項不放在完整模式的全域設定。`Legacy mode` 預設關閉。它只改變完整模式的怪物瀏覽、搜尋與收藏；widget 與其他圖鑑不受影響。

顯示結構參考 [52巨商怪物分類](https://www.52jushang.com/monster/index.asp) 與 [台灣地面怪物](https://www.52jushang.com/monster/taiwan.asp)：地區索引按國家分組橫向排列，選擇地區後顯示「圖片／名稱與能力／說明」三欄資料。目錄與資料列沿用 app 原本的深灰色票，交錯底色及細分隔線區分記錄；體力／打擊抗性用暖紅、魔力／魔法抗性用藍色，屬性沿用小窗的官方圖示並保留文字。掉落名稱接在「掉落物：」後，以「a、b、c」的行內文字連結排列，英文用逗號分隔，隨說明欄寬度自然換行；每個名稱旁以括號顯示掉落率（未知顯示「未記錄／Not recorded」），名稱可開啟道具頁，返回後保留地區。內容仍來自本地 catalog，包括出沒地與未記錄狀態，不複製參考站的遊戲數據。

地區列表右下角固定提供「地區目錄」，可直接選另一個國家或地區，不必返回索引頁。浮層標示目前地區；選擇後收起並回到列表頂端，點外部或按 Escape 也可關閉。

完整模式的普通、Legacy、搜尋與收藏怪物列表共用篩選列：收起時保留條件摘要、結果數及清除入口，展開後可選目前地區／全部怪物、元素、屬性值上下限與掉落物。尚未選地區也能直接篩選全部怪物。掉落物選單可輸入中英文名稱、別名或道具編號（例如 #11608），候選只列出實際有怪物掉落的道具，分兩行顯示名稱與 #編號，右側顯示掉落怪物數。已選候選保留 bronze 邊框；重新開啟時保留已選道具與搜尋文字，可直接修改查詢尋找其他道具。選定後以道具編號精確匹配，自動依該道具掉落率由高至低排列。搜尋與收藏的篩選保留原本的搜尋字詞和收藏限制。

元素篩選沿用 widget 的官方火、水、風、雷、地圖示，保留滑鼠提示、文字替代及鍵盤焦點；「全部／無」使用文字。進階條件可收起，提供打擊／魔法抗性的上下限，與元素、ELA 及掉落物取交集。排序可選 ELA、兩種抗性、體力、魔力、經驗及主名稱，並以緊湊的 ↑／↓ 按鈕切換升降序，停留提示與無障礙標籤保留完整方向說明；數值依資料庫原始欄位比較，同值順序固定。主動排序優先於收藏置頂，原排序保留既有行為。選定精確掉落物後才提供「掉落率」排序；未記錄機率在升冪或降冪都排最後，同怪物有多個符合欄位時取最高機率、不加總，規則可由排序方向提示查看。改寫或清除掉落物時，僅將掉落率排序回復為原排序，其他排序選項維持。普通列以 bronze 邊框與較醒目的機率標出所選道具；Legacy 以文字粗體、底色與底線標出，保持原列高與虛擬捲動幾何。

完整模式怪物名稱以中英主副兩行顯示，依語言設定決定主名稱，另一語言用較小的次要文字。副名缺失或與主名相同時不重複顯示。普通模式保留一行一隻的固定列高，長名稱可停留查看完整雙語；Legacy 名稱欄依文字換行自然增高，避免擠壓能力數值。

普通模式怪物目錄頂端固定「所有怪物／收藏」，下方提供國家下拉與獨立的中英文地區搜尋，只有地區清單捲動。國家及地區搜尋只縮小目錄，選取地區才切換右側怪物範圍；與右上方的怪物名稱搜尋分開。地區列高 34px、間距 2px，長名稱可停留查看；目前選取與鍵盤焦點分開標示。Legacy 索引及浮動目錄同樣提供「所有怪物」入口。

普通怪物列表保留完整結果與捲動高度，只在可見範圍上下各兩列建立完整列控制；選中或展開列仍保留，避免「所有怪物」一次建立數百列圖示、提示與右鍵選單造成停頓。

完整模式的普通掉落圖示、展開詳情內的掉落列與 Legacy 掉落文字支援右鍵選單「查看掉落此道具的所有怪物」。此動作以精確道具編號篩選所有地區，清除怪物名稱、收藏及其他篩選限制，並自動以該道具掉落率由高至低排序；左鍵繼續開啟道具頁。支援 Menu 鍵、Shift+F10、Enter／Space、Escape 及點外部關閉。

## Controls

Full mode 側欄分類可直接拖曳重排，保留排序儲存和鍵盤替代操作。設定入口使用使用者要求的 ⚙️ 加文字標籤；小窗／完整模式以雙選項滑塊標示目前模式。兩個桌面平台共用 Slint 實作。

詳情頁的道具、任務、角色、NPC、怪物及配方產物名稱支援左鍵複製；中英主副名分開操作，跟隨語言設定換序。`CopyName` 以記錄 key 取得該語言的名稱，不從混有編號、數量、說明的顯示文字截取。滑鼠停留顯示完整名稱與複製提示，可用 Enter／Space／Ctrl+C 複製；名稱不加 hover 或焦點邊框，成功或失敗只以短暫 toast 回饋。保留列表、掉落與關聯名稱原有的跳頁動作。道具詳情補上另一語言名稱，沒有翻譯或兩者相同時不重複顯示。

Toast 在內容區底部置中；完整模式扣除左側導覽寬度，小窗則以整個視窗置中，與底部狀態列保留 12px 間距。

角色目錄加入「坐騎」及「精靈」。分類選用同類中固定一個可用角色頭像，避免每次刷新改變。完整模式先在右側顯示可捲動預覽，提供「展開詳細頁」；小窗直接使用同一角色詳情。一般角色的名稱、頭像與戰鬥預覽置頂，其後是依寬度分成 2–4 欄的能力值，再顯示技能說明及轉職／升級材料；精靈則優先顯示技能效果。長說明可換行，關聯名稱保留跳頁。

角色的地區目錄在完整模式中展開一層階級，沿用「階級」頁籤的次序，階級的數量及頭像只取該地區角色。點地區列出其全部角色，先依階級排序；點階級則只列出該地區、該階級的角色。小窗保留「地區 → 階級 → 角色」的逐層瀏覽。

一般角色和精靈顯示資料庫基礎能力；坐騎顯示能力加成與馬術要求，未知值省略。圖片欄位缺少本機素材時明確標示，動畫只播放真實客戶端素材，保留播放／暫停。收起視窗或切換到設定、計算器及 MCP 時停止更新動畫。

坐騎列表直接顯示所需馬術等級和移速加成（含作用對象），按馬術等級升序；缺值標明未記錄。精靈先依精靈系別分組，再按階級升序；以 PetInfo 的系別分類，不能用低階精靈常為無屬性的戰鬥屬性代替。分組標題屬於同一資料列的視覺標記，不加入假記錄或影響總數。精靈詳情優先顯示技能效果，基礎能力預設收起，仍可展開。完整模式與小窗共用含可換行摘要的列表列，圖片垂直置中。

精靈升級條件顯示在目標精靈頁的「升級至此精靈」：前置精靈、所需親密度及材料數量一併呈現。例如五階顯示四階升至五階的條件，四階則顯示三階升至四階；另保留下一階段的連結。第一階段沒有前置資料時省略此段。

共用資料列優先保留名稱欄空間，右側補充資料限制寬度；隱藏摘要不能把名稱欄壓成零寬。隨程式提供 15 款坐騎頭像、坐騎本體動畫及 33 隻精靈動畫，播放時跳過全透明影格。坐騎預覽不合成遊戲的額外光效圖層。
