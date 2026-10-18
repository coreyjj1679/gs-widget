---
name: Gersang desktop reference
description: Slint 原生桌面圖鑑；完整、小窗與浮動列共用深灰色票、bronze 重點色與原遊戲素材。
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
  window: "12px"
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

這份文件從目前的 `widget/ui/desktop.slint`、`widget/ui/app.slint` 與 `PRODUCT.md` 抽取，記錄 Windows／macOS 共用的 Slint 原生桌面介面設計。介面以深灰底色、bronze 重點色及清楚的文字層級承載圖鑑資料；分類與內容辨識沿用原遊戲圖像。

Full mode、compact widget 與收合分類列共用原 full mode 的深灰色票與 bronze 重點色。模式切換只改變版面與資訊密度；搜尋、收藏、分類排序、鍵盤操作、關聯連結及設定維持既有行為，小窗保留緊湊排列，浮動列保留半透明呈現。這裡記錄的是目前實作與本次統一樣式的決定。

## Colors

**Primary：** `accent` 用於重點文字、選取圖示與鍵盤焦點；實色重點按鈕使用 `on-accent` 文字。`selected` 與 `selection-low` 提供低亮度的暖色選取背景。

**Neutral：** `sidebar`、`canvas`、`surface-alt`、`surface`、`detail` 區分導覽、工作區、分類、列表與詳細資料。`text` 是主要內容，`secondary` 是說明，`muted` 是捷徑與次要資訊，`soft` 是較柔和的重點文字。`border` 用於分隔；`hover`、`control`、`hover-strong`、`raised` 表達互動狀態。

**跨模式共用：** YAML 色票由 `DesktopTheme` 提供給所有模式，不隨 `full-mode` 或 `collapsed` 切換。Full mode 與 compact widget 使用相同的不透明 `chrome`／`canvas` 底色，整體透明度仍由使用者的 `window-opacity` 設定控制。收合分類列與圖示底板保留半透明，但使用相同深灰色系；圖示周圍仍為透明。各模式的 hover、選取、邊框與說明文字都使用共用語意色票；體力、抗性、元素及狀態色保留原本意義。

## Typography

Windows 使用 Microsoft JhengHei UI，macOS 使用 PingFang TC，由 `ui-font` 控制；兩者是各平台選擇，不是混用的字體堆疊。預設內文為 14px／400，分類標題為 21px／600，入口標題為 15px／600，主要入口為 17px／600。導覽文字為 13px，選取時加粗至 600；說明與標籤多用 12px，捷徑用 11px。

保持中英文切換可讀。導覽與單行列表使用省略；說明、空狀態與詳細內容按可用寬度換行。小窗保留既有標題字級與緊湊密度，完整模式保留較大的標題與內距；共用同一套文字顏色與層級。不要為了塞入更多資料縮小主要內容字級。

## Layout

全視窗預設為 **1280 × 800 logical pixels**，最低目標為 **960 × 640**；不得把 logical pixels 當作 Retina 實體像素。採左側持續導覽與右側內容工作區。視窗寬度低於 1100px 時，側欄由 208px 改為 176px；full mode 頁面內距為 24px，列表／詳情分欄間距為 12px。

一般圖鑑並排呈現分類、可捲動列表與詳細資料；內容工作區低於 900px 時，分類欄由 180px 收為 150px，詳情欄由 320px 收為 260px，中間列表保留伸縮空間。怪物瀏覽保持一行一筆：名稱、類型／元素、六項戰鬥數值與掉落圖示同時可見，選取後在原列下方補充緊湊的屬性、出沒地與掉落連結。怪物目錄與內容間距為 24px，目錄列高 36px、列距 4px。分類搜尋常駐頁首，寬 280px；設定內容限制最大寬度為 688px。

尺寸不足時保留可操作的捲動區、必要欄位及完整文字提示，不套用網頁或手機導覽模式。全視窗／widget 切換、收合與視窗拖曳保留現有入口。

## Elevation & Depth

桌面 shell、導覽與入口元件沒有陰影；深度由表面明度、細分隔線、選取底色及 hover 狀態表達。原遊戲圖像提供內容辨識，不作背景紋理。新增區塊沿用這套層次，避免額外加入光暈、玻璃模糊或裝飾性陰影。

## Shapes

Full mode 與 compact widget 的展開視窗外框都使用 12px 圓角；視窗控制統一為 26px 高、5px 圓角，平時透明、hover 時顯示底色。導覽使用 7px，入口及一般面板使用 8px，總覽收藏區使用 12px。收合分類列保留原本形狀。分隔線與焦點邊框為 1px。`DesktopIcon` 使用共用 24 × 24 viewbox、1.6px 描邊與透明填色；功能符號沿用此筆畫，分類圖示仍用遊戲素材。

## Components

- **DesktopNavRow：** 40px 高，左右內距 10px，圖示區 22px。選取顯示暖色底與粗體標籤；hover 顯示深灰底；焦點顯示 accent 邊框。Enter／Space 可啟動，停用時透明度為 0.5，保留 accessible label 與 enabled 狀態。
- **DesktopEntry：** 預設 80px 高、44px 遊戲圖像、文字與圖像間距 16px、細底線；hover 才出現面底色。總覽已使用 112px 主要入口與 88px 次要入口。標題與說明可省略，動作標籤只在元件寬度大於 380px 時顯示。
- **MonsterTableRow／InlineMonster：** 僅限 full mode。一行一筆，列高 128px、列距 8px；搜尋與收藏沿用。52px 頭像旁顯示名稱、類型及元素，右方以三欄兩列排列體力、魔力、攻擊、兩種抗性及經驗；下方保留掉落圖示與實際數量。每個掉落圖示可直接開啟道具頁，滑鼠停留時顯示完整名稱；缺圖仍保留原位置的可點擊替代圖示，避免道具連結錯位。體力與打擊抗性為暖紅，魔力與魔法抗性為藍；元素沿用小窗的官方屬性圖示，並保留對應的文字及柔和色彩。各數值始終保留文字標籤。展開詳情直接插在所選列下方，以 12px 內距及約 30px 的連結列呈現，實際高度隨換行量測，不加入大段空白。
- **列表／詳細資料：** 保留選取列與關聯記錄的上下文；資料、計數與圖片來自現有 catalog。未選取、收藏為空或沒有篩選結果時，說明下一個可執行動作。
- **搜尋／設定：** 沿用 Slint 原生 `LineEdit`／`TextEdit` 與現有選項控制。`LangChip` 與 `ChoiceBar` 的選取狀態共用 `raised` 底色與主要文字色，讓小窗與完整模式的設定選項保持一致；各模式保留原本的控制排列與操作行為。常駐搜尋限定目前分類；分類排序仍以拖曳與可見捷徑標籤呈現。

## Do's and Don'ts

- **Do** 先沿用 `DesktopTheme` 與現有元件，再新增必要變體；修改共用控制時同時確認 full mode、widget、collapsed 的分支。
- **Do** 使用 `data/client` 的原遊戲素材，缺圖時保留已有文字替代；測試中英文、1280 × 800 與 960 × 640，以及兩個桌面系統的字體。
- **Do** 保留鍵盤焦點、Enter／Space 啟動與既有分類捷徑；讓 selected、hover、focus 各自可辨。
- **Don't** 把裝飾放在表格、數值與關聯資料之前，或把 full mode 的側欄與間距套到 compact widget。
- **Don't** 編造圖鑑數量、使用數據或遊戲素材；可選戰鬥動畫缺失時，不把它描述成必備資產。

## Legacy mode

完整模式怪物頁右上角的小齒輪提供 `Legacy mode`、圖片模式與戰鬥動畫設定，沿用 view.txt 保存偏好；這些選項不放在完整模式的全域設定。`Legacy mode` 預設關閉。它只改變完整模式的怪物瀏覽、搜尋與收藏；widget 與其他圖鑑不受影響。

顯示結構參考 [52巨商怪物分類](https://www.52jushang.com/monster/index.asp) 與 [台灣地面怪物](https://www.52jushang.com/monster/taiwan.asp)：地區索引按國家分組橫向排列，選擇地區後顯示「圖片／名稱與能力／說明」三欄資料。目錄與資料列沿用 app 原本的深灰色票，交錯底色及細分隔線區分記錄；體力／打擊抗性用暖紅、魔力／魔法抗性用藍色，屬性沿用小窗的官方圖示並保留文字。掉落名稱接在「掉落物：」後，以「a、b、c」的行內文字連結排列，英文用逗號分隔，隨說明欄寬度自然換行；每個名稱可開啟道具頁，返回後保留地區。內容仍來自本地 catalog，包括出沒地與未記錄狀態，不複製參考站的遊戲數據。

地區列表右下角固定提供「地區目錄」，可直接選另一個國家或地區，不必返回索引頁。浮層標示目前地區；選擇後收起並回到列表頂端，點外部或按 Escape 也可關閉。

完整模式的普通、Legacy、搜尋與收藏怪物列表共用篩選列：收起時保留條件摘要、結果數及清除入口，展開後可選目前地區／全部怪物、元素、屬性值上下限與掉落物。尚未選地區也能直接篩選全部怪物。掉落物選單可輸入中英文名稱或別名，候選只列出實際有怪物掉落的道具，並顯示掉落怪物數；選定後以道具編號精確匹配。搜尋與收藏的篩選保留原本的搜尋字詞和收藏限制。

元素篩選沿用 widget 的官方火、水、風、雷、地圖示，保留滑鼠提示、文字替代及鍵盤焦點；「全部／無」使用文字。進階條件可收起，提供打擊／魔法抗性的上下限，與元素、ELA 及掉落物取交集。排序可選 ELA、兩種抗性、體力、魔力、經驗及主名稱，並以緊湊的 ↑／↓ 按鈕切換升降序，停留提示與無障礙標籤保留完整方向說明；數值依資料庫原始欄位比較，同值順序固定。主動排序優先於收藏置頂，原排序保留既有行為。

完整模式怪物名稱以中英主副兩行顯示，依語言設定決定主名稱，另一語言用較小的次要文字。副名缺失或與主名相同時不重複顯示。普通模式保留一行一隻的固定列高，長名稱可停留查看完整雙語；Legacy 名稱欄依文字換行自然增高，避免擠壓能力數值。

普通模式怪物目錄頂端固定「所有怪物／收藏」，下方提供國家下拉與獨立的中英文地區搜尋，只有地區清單捲動。國家及地區搜尋只縮小目錄，選取地區才切換右側怪物範圍；與右上方的怪物名稱搜尋分開。地區列高 34px、間距 2px，長名稱可停留查看；目前選取與鍵盤焦點分開標示。Legacy 索引及浮動目錄同樣提供「所有怪物」入口。

普通怪物列表保留完整結果與捲動高度，只在可見範圍上下各兩列建立完整列控制；選中或展開列仍保留，避免「所有怪物」一次建立數百列圖示、提示與右鍵選單造成停頓。

完整模式的普通掉落圖示、展開詳情內的掉落列與 Legacy 掉落文字支援右鍵選單「查看掉落此道具的所有怪物」。此動作以精確道具編號篩選所有地區，清除怪物名稱、收藏及其他篩選限制並保留排序；左鍵繼續開啟道具頁。支援 Menu 鍵、Shift+F10、Enter／Space、Escape 及點外部關閉。

## Controls

Full mode 側欄分類可直接拖曳重排，保留排序儲存和鍵盤替代操作。設定入口使用使用者要求的 ⚙️ 加文字標籤；小窗／完整模式以雙選項滑塊標示目前模式。兩個桌面平台共用 Slint 實作。
