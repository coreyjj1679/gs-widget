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

一般圖鑑並排呈現分類、可捲動列表與詳細資料；內容工作區低於 900px 時，分類欄由 180px 收為 150px，詳情欄由 320px 收為 260px，中間列表保留伸縮空間。怪物瀏覽沿用可捲動比較列與行內詳情。分類搜尋常駐頁首，寬 280px；設定內容限制最大寬度為 688px。

尺寸不足時保留可操作的捲動區、必要欄位及完整文字提示，不套用網頁或手機導覽模式。全視窗／widget 切換、收合與視窗拖曳保留現有入口。

## Elevation & Depth

桌面 shell、導覽與入口元件沒有陰影；深度由表面明度、細分隔線、選取底色及 hover 狀態表達。原遊戲圖像提供內容辨識，不作背景紋理。新增區塊沿用這套層次，避免額外加入光暈、玻璃模糊或裝飾性陰影。

## Shapes

視窗控制使用 5px 圓角，導覽使用 7px，入口及一般面板使用 8px，總覽收藏區使用 12px。分隔線與焦點邊框為 1px。`DesktopIcon` 使用共用 24 × 24 viewbox、1.6px 描邊與透明填色；功能符號沿用此筆畫，分類圖示仍用遊戲素材。

## Components

- **DesktopNavRow：** 40px 高，左右內距 10px，圖示區 22px。選取顯示暖色底與粗體標籤；hover 顯示深灰底；焦點顯示 accent 邊框。Enter／Space 可啟動，停用時透明度為 0.5，保留 accessible label 與 enabled 狀態。
- **DesktopEntry：** 預設 80px 高、44px 遊戲圖像、文字與圖像間距 16px、細底線；hover 才出現面底色。總覽已使用 112px 主要入口與 88px 次要入口。標題與說明可省略，動作標籤只在元件寬度大於 380px 時顯示。
- **列表／詳細資料：** 保留選取列與關聯記錄的上下文；資料、計數與圖片來自現有 catalog。未選取、收藏為空或沒有篩選結果時，說明下一個可執行動作。
- **搜尋／設定：** 沿用 Slint 原生 `LineEdit`／`TextEdit` 與現有選項控制。常駐搜尋限定目前分類；分類排序仍以拖曳與可見捷徑標籤呈現。

## Do's and Don'ts

- **Do** 先沿用 `DesktopTheme` 與現有元件，再新增必要變體；修改共用控制時同時確認 full mode、widget、collapsed 的分支。
- **Do** 使用 `data/client` 的原遊戲素材，缺圖時保留已有文字替代；測試中英文、1280 × 800 與 960 × 640，以及兩個桌面系統的字體。
- **Do** 保留鍵盤焦點、Enter／Space 啟動與既有分類捷徑；讓 selected、hover、focus 各自可辨。
- **Don't** 把裝飾放在表格、數值與關聯資料之前，或把 full mode 的側欄與間距套到 compact widget。
- **Don't** 編造圖鑑數量、使用數據或遊戲素材；可選戰鬥動畫缺失時，不把它描述成必備資產。
