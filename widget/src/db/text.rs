use super::hero::{hero_land_pair, hero_rank_pair, hero_tier_pair};
use super::item::{item_category_name, set_slot_heading, weapon_family_name};
use super::Hit;
use super::paths::client_build;
use super::rows::QuestStep;

pub(in crate::db) const SOURCE_LABEL: &[(&str, &str, &str, &str)] = &[
    ("unified", "統合", "Unified", "통합"),
    ("alchemy", "鍊金", "Alchemy", "연금"),
    ("blacksmith", "鐵匠", "Blacksmith", "대장"),
    ("jeweler", "細工", "Jeweler", "세공"),
    ("npc", "NPC製造", "NPC craft", "NPC제작"),
    ("formation", "陣形", "Formation", "진형"),
    ("baekje", "百濟寶物", "Baekje", "백제보물"),
    ("reinforce", "強化", "Reinforce", "강화"),
    ("main", "一般任務", "Quest", "일반"),
    ("letter", "書信", "Letter", "서신"),
    ("intro", "入門", "Intro", "입문"),
    ("mercenary", "傭兵劇情", "Mercenary", "용병"),
    ("dispatch", "客棧派遣", "Dispatch", "파견"),
];

pub(in crate::db) fn element_letter(letter: &str, lang: &str) -> String {
    let (en, zh) = match letter {
        "F" => ("Fire", "火"),
        "W" => ("Water", "水"),
        "A" => ("Wind", "風"),
        "T" => ("Thunder", "雷"),
        "E" => ("Earth", "地"),
        _ => return letter.to_string(),
    };
    chrome_text(lang, zh, en)
}

pub(in crate::db) fn source_names(source: &str) -> (&'static str, &'static str) {
    SOURCE_LABEL
        .iter()
        .find(|(key, _, _, _)| *key == source)
        .map(|(_, zh, en, _)| (*zh, *en))
        .unwrap_or(("", ""))
}

pub(in crate::db) fn label(source: &str, lang: &str) -> String {
    let (zh, en) = source_names(source);
    if zh.is_empty() && en.is_empty() {
        source.to_string()
    } else {
        chrome_text(lang, zh, en)
    }
}

pub(in crate::db) fn hit_version(key: &str) -> &str {
    key.strip_prefix("patch:")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("")
}

pub(in crate::db) fn patch_number(version: &str) -> i64 {
    version
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

pub(in crate::db) fn dotted_version(version: &str) -> String {
    let number = patch_number(version);
    let text = number.to_string();
    if text.len() >= 5 {
        format!("V{}.{}", &text[..1], &text[1..])
    } else if version.is_empty() {
        String::new()
    } else {
        version.to_string()
    }
}

pub(in crate::db) fn ship_label(version: &str, lang: &str) -> String {
    let key = if patch_number(version) <= client_build() {
        "shipped"
    } else {
        "unshipped"
    };
    phrase(key, lang)
}

pub(in crate::db) fn notes_placeholder(lang: &str) -> Vec<Hit> {
    vec![Hit {
        title: phrase("notes-empty-title", lang),
        subtitle: phrase("notes-empty-sub", lang),
        key: "note:empty".into(),
        icon: None,
                    solo: false,
                    aside: String::new(),
                badge: None,
                    chip: String::new(),
    }]
}

pub(in crate::db) fn server_name(server: &str, lang: &str) -> String {
    let key = match server {
        "live" => "server-live",
        "test" => "server-test",
        _ => "server-all",
    };
    phrase(key, lang)
}

pub(in crate::db) fn english_first(lang: &str) -> bool {
    matches!(lang, "en-zh" | "en")
}

pub(in crate::db) fn chrome(lang: &str) -> &'static str {
    if english_first(lang) { "en" } else { "zh" }
}

pub(in crate::db) fn chrome_text(lang: &str, zh: &str, en: &str) -> String {
    if chrome(lang) == "en" { en } else { zh }.to_string()
}

pub(in crate::db) fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

pub(in crate::db) fn first_name(primary: &str, fallback: &str) -> String {
    let primary = primary.trim();
    if primary.is_empty() { fallback.trim().to_string() } else { primary.to_string() }
}

pub(in crate::db) fn lead(lang: &str, en: &str, zh: &str) -> String {
    if english_first(lang) { first_name(en, zh) } else { first_name(zh, en) }
}

pub(in crate::db) fn trail(lang: &str, en: &str, zh: &str) -> String {
    let en = en.trim();
    let zh = zh.trim();
    if en.is_empty() || zh.is_empty() || same_name(en, zh) {
        return String::new();
    }
    if english_first(lang) { zh.to_string() } else { en.to_string() }
}

pub(in crate::db) fn show(lang: &str, en: &str, zh: &str) -> String {
    let main = lead(lang, en, zh);
    let alt = trail(lang, en, zh);
    if alt.is_empty() { main } else { format!("{main} · {alt}") }
}

pub(in crate::db) fn stack(lang: &str, en: &str, zh: &str) -> String {
    let main = lead(lang, en, zh);
    let alt = trail(lang, en, zh);
    if alt.is_empty() { main } else { format!("{main}\n{alt}") }
}

pub(in crate::db) fn caption(alt: &str, rest: String) -> String {
    if alt.is_empty() {
        rest
    } else if rest.is_empty() {
        alt.to_string()
    } else {
        format!("{alt} · {rest}")
    }
}

pub(in crate::db) fn pick(lang: &str, _ko: &str, en: &str, zh: &str) -> String {
    show(lang, en, zh)
}

pub(in crate::db) fn pick_place(lang: &str, en: &str, zh: &str) -> String {
    show(lang, en, zh)
}

pub(in crate::db) fn commas(value: i64) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let body: String = out.chars().rev().collect();
    if negative { format!("-{body}") } else { body }
}

pub(in crate::db) fn game_breaks(text: String) -> String {
    if !text.contains('\\') {
        return text;
    }
    text.replace("\\r\\n", "\n").replace("\\n", "\n").replace("\\r", "")
}

pub(in crate::db) fn commas_text(text: &str) -> String {
    text.parse::<i64>().map(commas).unwrap_or_else(|_| text.to_string())
}

pub(in crate::db) fn kind_name(kind: &str, lang: &str) -> String {
    let (zh, en) = match kind.to_ascii_lowercase().as_str() {
        "item" | "book" => ("道具", "Item"),
        "wearitem" => ("裝備", "Equip"),
        "useitem" => ("使用", "Use"),
        "monster" => ("怪物", "Monster"),
        "exp" | "expall" => ("經驗", "Exp"),
        "money" => ("金錢", "Money"),
        "credit" | "credit1" | "credit2" | "credit3" | "credit4" | "credit5" => ("信用", "Credit"),
        "quest" => ("任務", "Quest"),
        "level" | "level1" | "level2" | "level3" | "level4" | "level5" => ("等級", "Level"),
        "map" => ("地圖", "Map"),
        "satiety" => ("飽食度", "Satiety"),
        "npc" => ("人物", "NPC"),
        "work" => ("勞動", "Labor"),
        "key" => ("按鍵", "Key"),
        "state" => ("狀態", "State"),
        "user" => ("角色", "Character"),
        "pet" => ("寵物", "Pet"),
        "nation" => ("國籍", "Nation"),
        _ => return kind.to_string(),
    };
    chrome_text(lang, zh, en)
}

pub(in crate::db) fn role_phrase(role: &str, lang: &str) -> String {
    match role {
        "requirement" => phrase("req", lang),
        "objective" => phrase("done", lang),
        "reward" => phrase("reward", lang),
        _ => role.to_string(),
    }
}

pub(in crate::db) fn reward_amount(step: &QuestStep) -> Option<String> {
    let qty = step.qty.trim();
    if !qty.is_empty() && qty != "0" {
        return Some(commas_text(qty));
    }
    let code = step.code.trim();
    if !code.is_empty() && code != "0" {
        return Some(commas_text(code));
    }
    None
}

pub(in crate::db) fn with_count(name: String, qty: &str) -> String {
    if qty.is_empty() || qty == "0" {
        name
    } else {
        format!("{name} × {}", commas_text(qty))
    }
}

pub(in crate::db) fn numbered(label: String, qty: &str) -> Option<(String, String)> {
    if qty.is_empty() || qty == "0" {
        return None;
    }
    Some((format!("{label} {}", commas_text(qty)), String::new()))
}

pub(in crate::db) fn step_hint(label: &str) -> String {
    let trimmed = label.trim();
    if trimmed.is_empty() || trimmed == "0" || trimmed == "설명" {
        String::new()
    } else {
        trimmed.to_string()
    }
}

pub(in crate::db) fn step_value(code: &str, qty: &str) -> String {
    let qty_ok = !qty.is_empty() && qty != "0";
    let code_ok = !code.is_empty() && code != "0";
    match (qty_ok, code_ok) {
        (true, true) => format!("{}  {code}", commas_text(qty)),
        (true, false) => commas_text(qty),
        (false, true) => code.to_string(),
        (false, false) => String::new(),
    }
}

pub(in crate::db) fn named_lead(lang: &str, en: &str, zh: &str, label: &str, code: &str) -> (String, String) {
    let title = lead(lang, en, zh);
    if title.is_empty() {
        let fallback = if label.is_empty() { code.to_string() } else { label.to_string() };
        (fallback, String::new())
    } else {
        (title, trail(lang, en, zh))
    }
}

pub(in crate::db) fn phrase(key: &str, lang: &str) -> String {
    let (zh, en) = match key {
        "code" => ("代碼", "Code"),
        "vendor" => ("扔店價", "Vendor"),
        "weight" => ("重量", "Weight"),
        "item-sort" => ("分類", "Category"),
        "pieces" => ("件", "items"),
        "sets" => ("套", "sets"),
        "set" => ("套裝", "Set"),
        "set-size" => ("件數", "Pieces"),
        "set-charm" => ("咒術", "Charm"),
        "set-unique" => ("專用", "Exclusive"),
        "slots" => ("部位", "Slots"),
        "item-fx" => ("道具", "Item"),
        "set-fx" => ("套裝", "Set"),
        "total-fx" => ("合計", "Total"),
        "crit-dmg" => ("暴擊傷害", "Crit damage"),
        "dmg-cut" => ("減傷", "Damage cut"),
        "missing-set" => ("找不到這套裝備", "Set not found"),
        "enhance" => ("強化", "Enhance"),
        "damage" => ("傷害", "Damage"),
        "move-speed" => ("移速", "Move"),
        "atk-speed" => ("攻速", "Atk speed"),
        "atk-range" => ("射程", "Range"),
        "skill-range" => ("技距", "Skill range"),
        "crit" => ("暴擊", "Crit"),
        "mana-cut" => ("減魔", "Mana"),
        "sight" => ("視野", "Sight"),
        "air-dmg" => ("對空", "Air"),
        "ground-dmg" => ("對地", "Ground"),
        "stigma" => ("聖痕", "Stigma"),
        "merc-def" => ("傭防", "Merc def"),
        "merc-atk" => ("傭攻", "Merc atk"),
        "spell-rate" => ("咒術", "Spell"),
        "reflect" => ("反射", "Reflect"),
        "invuln" => ("無敵", "Invuln"),
        "qty" => ("數量", "Qty"),
        "fee" => ("製作費", "Craft fee"),
        "learn" => ("學習費", "Learn fee"),
        "materials" => ("材料", "Materials"),
        "craft-need" => ("合成條件", "Requirements"),
        "related" => ("相關", "Related"),
        "credit-level" => ("信用等級", "Credit"),
        "pick-one" => ("任選", "Choice"),
        "combine" => ("組合", "Combine"),
        "awaken" => ("覺醒", "Awaken"),
        "remodel" => ("改造", "Remodel"),
        "promote" => ("轉職", "Promote"),
        "upgrade" => ("升級", "Upgrade"),
        "roster" => ("角色", "Roster"),
        "src-roster" => ("角色", "Roster"),
        "open-recipe" => ("打開配方", "Open recipe"),
        "spawn" => ("出沒", "Found in"),
        "kind" => ("類型", "Type"),
        "min-level" => ("最低等級", "Min level"),
        "success" => ("成功率", "Success"),
        "recipes" => ("張", "recipes"),
        "unit" => ("單位", "Unit"),
        "spawn-place" => ("出沒地", "Found in"),
        "hp" => ("體力", "HP"),
        "atk" => ("攻擊", "ATK"),
        "exp" => ("經驗", "EXP"),
        "prev" => ("前置", "Requires"),
        "chain-prev" => ("前置", "Prev"),
        "chain-next" => ("下一", "Next"),
        "repeat" => ("可重複", "Repeats"),
        "quest-count" => ("則", "quests"),
        "starter" => ("新手", "Starter"),
        "combat" => ("戰鬥", "Combat"),
        "repeat-type" => ("重複", "Repeat"),
        "timed" => ("限時", "Timed"),
        "giver" => ("人物", "NPC"),
        "req" => ("條件", "Requires"),
        "done" => ("完成", "Complete"),
        "reward" => ("獎勵", "Reward"),
        "task-kill" => ("打怪物", "Kill"),
        "task-item" => ("要物品", "Need item"),
        "task-talk" => ("對話", "Talk"),
        "task-other" => ("其他", "Other"),
        "missing" => ("找不到", "Not found"),
        "missing-item" => ("找不到這個道具", "Item not found"),
        "missing-recipe" => ("找不到這張配方", "Recipe not found"),
        "missing-quest" => ("找不到這個任務", "Quest not found"),
        "missing-monster" => ("找不到這隻怪物", "Monster not found"),
        "missing-npc" => ("找不到這個 NPC", "NPC not found"),
        "missing-hero" => ("找不到這個角色", "Character not found"),
        "job-chain" => ("轉職", "Class line"),
        "job-base" => ("原本", "Base"),
        "job-first" => ("一轉", "1st"),
        "job-second" => ("二轉", "2nd"),
        "job-remodel" => ("改造", "Remodel"),
        "hire" => ("雇用", "Hire"),
        "skill" => ("技能", "Skill"),
        "heroes" => ("人", "characters"),
        "credit" => ("信用", "Credit"),
        "goods" => ("種", "goods"),
        "nation" => ("國籍", "Nation"),
        "map" => ("地圖", "Map"),
        "coord" => ("座標", "Position"),
        "monsters" => ("隻", "monsters"),
        "mp" => ("魔力", "MP"),
        "hit-resist" => ("打擊抗性", "Hit resist"),
        "magic-resist" => ("魔法抗性", "Magic resist"),
        "element" => ("屬性", "Element"),
        "ela" => ("屬性值", "ELA"),
        "solo-drop" => ("單人限定", "Solo only"),
        "sold-in" => ("在賣", "Sold in"),
        "specialty" => ("特產", "Specialty"),
        "people-here" => ("人物", "People"),
        "lurking" => ("出沒", "Found"),
        "dropped-by" => ("會掉落的怪物", "Dropped by"),
        "craft-how" => ("合成", "Craft"),
        "make" => ("生產", "Produce"),
        "src-sold-in" => ("商店", "Shops"),
        "src-dropped-by" => ("掉落", "Drops"),
        "src-craft-how" => ("合成", "Craft"),
        "src-make" => ("生產", "Produce"),
        "src-used-for" => ("用途", "Uses"),
        "src-also-from" => ("其他", "Other"),
        "use-quest" => ("任務", "Quest"),
        "use-craft" => ("合成", "Craft"),
        "yield" => ("產量", "Yield"),
        "yield-max" => ("上限", "Max"),
        "labor" => ("勞動", "Labor"),
        "tool" => ("工具", "Tool"),
        "used-for" => ("用途", "Used for"),
        "also-from" => ("其他獲取", "Also from"),
        "all-regions" => ("所有地區", "All regions"),
        "land-c" => ("中國", "China"),
        "land-tw" => ("台灣", "Taiwan"),
        "land-k" => ("朝鮮", "Joseon"),
        "land-j" => ("日本", "Japan"),
        "land-ct" => ("中國·台灣", "China · Taiwan"),
        "land-m" => ("蒙古", "Mongolia"),
        "no-droppers" => ("沒有怪物掉落", "No monsters drop this"),
        "craft" => ("合成", "Craft"),
        "detail" => ("說明", "Note"),
        "boss" => ("首領", "Boss"),
        "normal" => ("一般", "Normal"),
        "air" => ("空中", "Air"),
        "ground" => ("地面", "Ground"),
        "drops" => ("掉落", "Drops"),
        "notes-empty-title" => ("尚無更新日誌", "No patch notes"),
        "notes-empty-sub" => ("點開看怎麼更新", "Open for how to update"),
        "notes-help" => (
            "把公告存成 data/notices 裡的 .txt。\n.txt 和 .gts 都換好之後，執行 python -m gersang update，\n或雙擊 update.bat，然後重新開啟這個視窗。",
            "Save notices as .txt files in data/notices.\nAfter the .txt and .gts files are in place, run python -m gersang update\nor double-click update.bat, then reopen this window.",
        ),
        "server-live" => ("本服", "Live"),
        "server-test" => ("測試服", "Test"),
        "server-all" => ("全服", "All"),
        "shipped" => ("已實裝", "In"),
        "unshipped" => ("未實裝", "Not in"),
        _ => (key, key),
    };
    chrome_text(lang, zh, en)
}

pub fn nation_label(id: &str, lang: &str) -> String {
    nation_name(id.parse().unwrap_or(0), lang)
}

pub fn source_label(source: &str, lang: &str) -> String {
    label(source, lang)
}

pub fn item_category_label(key: &str, lang: &str) -> String {
    item_category_name(key, lang)
}

pub fn weapon_family_label(key: &str, lang: &str) -> String {
    weapon_family_name(key, lang)
}

pub fn hero_rank_label(rank: &str, lang: &str) -> String {
    let (en, zh) = hero_rank_pair(rank.parse().unwrap_or(-1));
    if en.is_empty() && zh.is_empty() {
        rank.to_string()
    } else {
        chrome_text(lang, zh, en)
    }
}

pub fn hero_land_label(mask: &str, lang: &str) -> String {
    let (en, zh) = hero_land_pair(mask.parse().unwrap_or(-1));
    chrome_text(lang, zh, en)
}

pub fn hero_tier_label(rank: &str, lang: &str) -> String {
    let (en, zh) = hero_tier_pair(rank.parse().unwrap_or(-1));
    if en.is_empty() && zh.is_empty() {
        rank.to_string()
    } else {
        chrome_text(lang, zh, en)
    }
}

pub(in crate::db) fn nation_pair(id: i64) -> (&'static str, &'static str) {
    match id {
        1 => ("Joseon", "朝鮮"),
        2 => ("Japan", "日本"),
        3 => ("China", "中國"),
        4 => ("Taiwan", "台灣"),
        5 => ("India", "印度"),
        6 => ("Special", "特殊"),
        _ => ("Other", "其他"),
    }
}

pub(in crate::db) fn nation_name(id: i64, lang: &str) -> String {
    let (en, zh) = nation_pair(id);
    show(lang, en, zh)
}

pub(in crate::db) fn element_name(id: i64, lang: &str) -> String {
    let (en, zh) = match id {
        1 => ("Fire", "火"),
        2 => ("Water", "水"),
        3 => ("Wind", "風"),
        4 => ("Thunder", "雷"),
        5 => ("Earth", "地"),
        _ => ("None", "無"),
    };
    show(lang, en, zh)
}

pub(in crate::db) fn element_file(element: i64) -> Option<&'static str> {
    Some(match element {
        1 => "Assets/Textures/UI/Icons/elementfire.AGF",
        2 => "Assets/Textures/UI/Icons/elementwater.AGF",
        3 => "Assets/Textures/UI/Icons/elementair.AGF",
        4 => "Assets/Textures/UI/Icons/elementthun.AGF",
        5 => "Assets/Textures/UI/Icons/elementearth.AGF",
        _ => return None,
    })
}

pub(in crate::db) fn realm_pair(id: &str) -> (&'static str, &'static str) {
    match id {
        "joseon" => ("Joseon", "朝鮮"),
        "japan" => ("Japan", "日本"),
        "china" => ("China", "中國"),
        "taiwan" => ("Taiwan", "台灣"),
        "india" => ("India", "印度"),
        "mongolia" => ("Mongolia", "蒙古"),
        _ => ("", ""),
    }
}

pub fn relation_label(key: &str, lang: &str) -> String {
    if let Some(label) = set_slot_heading(key, lang) {
        return label;
    }
    let phrase_key = match key {
        "sold-in" => "src-sold-in",
        "dropped-by" => "src-dropped-by",
        "craft-how" => "src-craft-how",
        "make" => "src-make",
        "used-for" => "src-used-for",
        "also-from" => "src-also-from",
        "roster" => "src-roster",
        "set" => "set",
        _ => return key.to_string(),
    };
    phrase(phrase_key, lang)
}
