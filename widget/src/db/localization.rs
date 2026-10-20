//! Korean translations for application labels. Database names keep their own
//! language selection and fallback rules in text.rs.
use super::language::primary_lang;

pub fn ui_text_ref<'a>(lang: &str, zh: &'a str, en: &'a str) -> &'a str {
    match primary_lang(lang) {
        "zh" => zh,
        "ko" => korean_label(zh, en).unwrap_or(en),
        _ => en,
    }
}

pub fn ui_text(lang: &str, zh: &str, en: &str) -> String {
    if primary_lang(lang) == "ko" {
        if let Some(text) = korean_label(zh, en) {
            return text.to_string();
        }
        if let Some(text) = dynamic_korean(en) { return text; }
    }
    ui_text_ref(lang, zh, en).to_string()
}

fn korean_label(zh: &str, en: &str) -> Option<&'static str> {
    // A few short English labels have different meanings in different panels.
    match (zh, en) {
        ("防禦力", "Armor") => Some("방어력"),
        ("符", "Charm") => Some("부적"),
        ("傭兵劇情", "Mercenary") => Some("용병 퀘스트"),
        _ => translated(en)
            .or_else(|| super::ui_korean::translated(en))
            .or_else(|| OPERATIONAL_ERRORS.iter().find(|(key, _, _)| *key == en).map(|(_, _, ko)| *ko)),
    }
}

/// Localize application error messages while retaining external diagnostics,
/// filenames, and version numbers verbatim.
pub fn localized_error(lang: &str, message: &str) -> String {
    if let Some((error, path)) = message.split_once("；原有檔案已保留 / Original file preserved: ") {
        return format!("{} · {}: {path}", localized_error(lang, error), ui_text(lang, "原有檔案已保留", "Original file preserved"));
    }
    if let Some((zh, en)) = message.split_once(" / ") {
        // Only legacy bilingual messages have a Chinese left half; a slash in
        // an OS diagnostic or a path is not a localization separator.
        if zh.chars().any(|ch| ('\u{3400}'..='\u{9fff}').contains(&ch)) {
            if let Some(version) = zh.strip_prefix("不支援的計算版本 ") {
                return format!("{} {version}", ui_text(lang, "不支援的計算版本", "Unsupported calculation version"));
            }
            if let Some((label, detail)) = en.split_once(": ") {
                return format!("{}: {detail}", ui_text(lang, zh, label));
            }
            return ui_text(lang, zh, en);
        }
    }
    for (zh, en) in [
        ("無法建立匯出資料夾", "Cannot create export folder"),
        ("無法讀取匯出時間", "Cannot read export time"),
        ("無法產生 JPG", "Cannot generate JPG"),
        ("無法儲存 JPG", "Cannot save JPG"),
        ("JPG 已儲存，但無法開啟圖片", "JPG saved, but the image could not be opened"),
        ("無法建立 JPG", "Cannot create JPG"),
        ("無法排版成本卡", "Cannot lay out cost card"),
        ("同名匯出太多，請稍後重試。", "Too many exports with this name. Try again later."),
        ("無法建立成本卡圖片。", "Cannot create cost card image."),
        ("成本卡圖片尺寸不正確。", "Invalid cost card image dimensions."),
        ("請輸入非負整數，例如 1,000；留空代表尚未填價。", "Enter a non-negative whole number, such as 1,000; leave blank for an unknown price."),
        ("數量或單價過大。", "Quantity or unit price is too large."),
        ("製作數量必須是大於 0 的整數。", "Crafting quantity must be a positive whole number."),
    ] {
        if message == zh || message == en || korean_label(zh, en) == Some(message) {
            return ui_text(lang, zh, en);
        }
        if let Some(detail) = message.strip_prefix(zh).and_then(|rest| rest.strip_prefix('：')) {
            return format!("{}: {detail}", ui_text(lang, zh, en));
        }
    }
    operational_error(lang, message).unwrap_or_else(|| message.to_string())
}

fn operational_error(lang: &str, message: &str) -> Option<String> {
    let language = primary_lang(lang);
    let selected = |en, zh, ko| match language { "zh" => zh, "ko" => ko, _ => en };
    for (en, zh, ko) in OPERATIONAL_ERRORS {
        if message == *en || message == *zh || message == *ko {
            return Some(selected(*en, *zh, *ko).to_string());
        }
    }
    for suffix in ["The previous application was restored", "The previous version was restored"] {
        if let Some(error) = message.strip_suffix(suffix).and_then(|text| text.strip_suffix(". ")) {
            return Some(format!("{} · {}", localized_error(lang, error), operational_error(lang, suffix)?));
        }
    }
    if let Some((error, path)) = message.split_once("\nRecovery files: ") {
        return Some(format!("{}\n{}: {path}", localized_error(lang, error), operational_error(lang, "Recovery files")?));
    }
    if let Some((directory, detail)) = message.strip_prefix("Cannot create update files in ")
        .and_then(|text| text.split_once(". Extract the app to a writable folder: ")) {
        return Some(format!("{} {directory}: {detail}", selected(
            "Cannot create update files; extract the app to a writable folder",
            "無法建立更新檔案，請將程式解壓到可寫入目錄",
            "업데이트 파일을 만들 수 없습니다. 앱을 쓰기 가능한 폴더에 압축 해제하세요")));
    }
    if let Some(executable) = message.strip_prefix("Run the unpacked Windows release as ")
        .and_then(|text| text.strip_suffix(" to update it")) {
        return Some(format!("{} {executable}", selected(
            "To update, run the unpacked Windows executable:",
            "請執行解壓後的 Windows 程式來更新：",
            "업데이트하려면 압축 해제한 Windows 실행 파일을 실행하세요:")));
    }
    if let Some(name) = message.strip_prefix("ZIP entry ").and_then(|text| text.strip_suffix(" has an invalid size")) {
        return Some(format!("{} {name}", selected("Invalid ZIP entry size:", "ZIP 內容大小無效：", "ZIP 항목 크기가 잘못되었습니다:")));
    }
    // Prefer the longest recognized prefix so detailed messages do not lose
    // their meaning to shorter labels such as "Staged update is missing".
    OPERATIONAL_ERRORS.iter().filter_map(|(en, zh, ko)| {
        let detail = message.strip_prefix(en)?;
        (detail.starts_with(": ") || detail.starts_with(' '))
            .then_some((en.len(), format!("{}{detail}", selected(*en, *zh, *ko))))
    }).max_by_key(|(length, _)| *length).map(|(_, text)| text)
}

fn translated(en: &str) -> Option<&'static str> {
    Some(match en {
        "Code" => "코드",
        "Vendor" => "상점 판매가",
        "Weight" => "무게",
        "Category" => "분류",
        "items" => "개",
        "sets" => "세트",
        "Set" => "세트",
        "Sets" => "세트",
        "Pieces" => "부품 수",
        "Charm" => "주술",
        "Exclusive" => "전용",
        "Slots" => "부위",
        "Item" => "아이템",
        "Total" => "합계",
        "Crit damage" => "치명타 피해",
        "Damage cut" => "피해 감소",
        "Set not found" => "세트를 찾을 수 없습니다",
        "Enhance" => "강화",
        "Damage" => "피해",
        "Move" => "이동 속도",
        "Atk speed" => "공격 속도",
        "Range" => "사거리",
        "Skill range" => "스킬 사거리",
        "Crit" => "치명타",
        "Mana" => "마력 감소",
        "Sight" => "시야",
        "Air" => "공중",
        "Ground" => "지상",
        "Stigma" => "성흔",
        "Merc def" => "용병 방어력",
        "Merc atk" => "용병 공격력",
        "Spell" => "주술",
        "Reflect" => "반사",
        "Invuln" => "무적",
        "Qty" => "수량",
        "Craft fee" => "제작 비용",
        "Learn fee" => "학습 비용",
        "Materials" => "재료",
        "Requirements" => "필요 조건",
        "Related" => "관련 정보",
        "Credit" => "신용",
        "Choice" => "선택",
        "Combine" => "조합",
        "Awaken" => "각성",
        "Awakened" => "각성",
        "Remodel" => "개조",
        "Promote" => "전직",
        "Promoted" => "전직 몬스터",
        "Upgrade" => "승급",
        "Roster" => "캐릭터",
        "Open recipe" => "제작법 열기",
        "Found in" => "출현 지역",
        "Type" => "유형",
        "Min level" => "최소 레벨",
        "Success" => "성공률",
        "recipes" => "개 제작법",
        "Unit" => "단위",
        "HP" => "체력",
        "ATK" => "공격력",
        "EXP" => "경험치",
        "Exp" => "경험치",
        "XP" => "경험치",
        "Requires" => "필요 조건",
        "Prev" => "이전",
        "Next" => "다음",
        "Repeats" => "반복 가능",
        "quests" => "개 퀘스트",
        "Starter" => "초보자",
        "Combat" => "전투",
        "Repeat" => "반복",
        "Timed" => "기간 제한",
        "NPC" => "NPC",
        "Complete" => "완료",
        "Reward" => "보상",
        "Kill" => "처치",
        "Need item" => "필요 아이템",
        "Talk" => "대화",
        "Other" => "기타",
        "Not found" => "찾을 수 없습니다",
        "Item not found" => "아이템을 찾을 수 없습니다",
        "Recipe not found" => "제작법을 찾을 수 없습니다",
        "Quest not found" => "퀘스트를 찾을 수 없습니다",
        "Monster not found" => "몬스터를 찾을 수 없습니다",
        "NPC not found" => "NPC를 찾을 수 없습니다",
        "Character not found" => "캐릭터를 찾을 수 없습니다",
        "Class line" => "전직 계열",
        "Base" => "기본",
        "1st" => "1차",
        "2nd" => "2차",
        "Hire" => "고용",
        "Skill" => "스킬",
        "characters" => "명",
        "Goods" => "상품",
        "goods" => "종",
        "Nation" => "국적",
        "Map" => "지도",
        "Position" => "좌표",
        "monsters" => "마리",
        "MP" => "마력",
        "Hit resist" => "타격 저항",
        "Magic resist" => "마법 저항",
        "Element" => "속성",
        "ELA" => "속성값",
        "Solo only" => "단독 전투 전용",
        "Sold in" => "판매처",
        "Specialty" => "특산품",
        "People" => "인물",
        "Found" => "출현",
        "Dropped by" => "드롭 몬스터",
        "Craft" => "제작",
        "Produce" => "생산",
        "Shops" => "상점",
        "Drops" => "드롭",
        "Uses" => "용도",
        "Quest" => "퀘스트",
        "Yield" => "생산량",
        "Max" => "최대",
        "Labor" => "노동력",
        "Tool" => "도구",
        "Used for" => "사용처",
        "Also from" => "기타 획득처",
        "All regions" => "모든 지역",
        "China" => "중국",
        "Taiwan" => "대만",
        "Joseon" => "조선",
        "Japan" => "일본",
        "China · Taiwan" => "중국 · 대만",
        "Mongolia" => "몽골",
        "India" => "인도",
        "No monsters drop this" => "드롭하는 몬스터가 없습니다",
        "Note" => "설명",
        "Boss" => "보스",
        "Normal" => "일반",
        "No patch notes" => "업데이트 내역이 없습니다",
        "Open for how to update" => "업데이트 방법 보기",
        "Live" => "본 서버",
        "Test" => "테스트 서버",
        "All" => "전체",
        "In" => "적용됨",
        "Not in" => "미적용",
        "Equip" => "장비",
        "Use" => "사용",
        "Monster" => "몬스터",
        "Money" => "돈",
        "Level" => "레벨",
        "Satiety" => "포만감",
        "Key" => "키",
        "State" => "상태",
        "Character" => "캐릭터",
        "Pet" => "펫",
        "Non-elemental" => "무속성",
        "None" => "없음",
        "Fire" => "불",
        "Water" => "물",
        "Wind" => "바람",
        "Thunder" => "뇌전",
        "Earth" => "땅",
        "Special" => "특수",
        "Item not recorded" => "아이템 정보 없음",
        "Monster not recorded" => "몬스터 정보 없음",
        "Defeat" => "토벌",
        "Talk to" => "대화",
        "Party XP" => "전체 경험치",
        "Cr" => "신용",
        "$" => "냥",
        "more" => "개 더",
        "No rewards recorded" => "보상 정보가 없습니다",
        "Objective not specified" => "목표 정보 없음",
        "Objectives" => "퀘스트 목표",
        "Prerequisite" => "선행 퀘스트",
        "Objective" => "목표 분류",
        "Location" => "위치",
        "Objective types" => "목표 유형",
        "Not recorded" => "기록 없음",
        "Level not recorded" => "레벨 정보 없음",
        "Required level" => "필요 레벨",
        "without a recorded level" => "개 레벨 정보 없음",
        "Region not recorded" => "지역 정보 없음",
        "Unknown NPC" => "NPC 정보 없음",
        "Unknown region" => "지역 정보 없음",
        "Unconnected quests" => "연결되지 않은 퀘스트",
        "starting quests" => "개 시작 퀘스트",
        "Reference" => "참조",
        "Starts at" => "시작점",
        "Quest chain" => "퀘스트 연계",
        "unresolved references" => "개 미확인 참조",
        "Region" => "지역",
        "Chain" => "연계",
        "Beginner" => "하급",
        "Intermediate" => "중급",
        "Advanced" => "상급",
        "Highest" => "최상급",
        "Riding unknown" => "기마술 정보 없음",
        "Main character" => "주인공",
        "Party" => "전체 파티",
        "Scope not recorded" => "대상 정보 없음",
        "Movement bonus unknown" => "이동 속도 보너스 정보 없음",
        "Element not recorded" => "속성 정보 없음",
        "Grade not recorded" => "등급 정보 없음",
        "ATK min" => "최소 공격력",
        "ATK max" => "최대 공격력",
        "STR" => "힘",
        "AGI" => "민첩",
        "DEX" => "민첩",
        "VIT" => "생명력",
        "INT" => "지력",
        "Max HP" => "최대 체력",
        "Max MP" => "최대 마력",
        "Attack" => "공격력",
        "Defense" => "방어력",
        "Move speed" => "이동 속도",
        "Attack speed" => "공격 속도",
        "Upgrade materials" => "승급 재료",
        "Acquisition materials" => "획득 재료",
        "Required spirit" => "선행 정령",
        "Required intimacy" => "필요 친밀도",
        "Upgrade to this spirit" => "이 정령으로 승급",
        "Required mount" => "필요 탈것",
        "Next form" => "다음 단계",
        "Previous form" => "이전 단계",
        "Upgrade path" => "승급 경로",
        "Big Dipper" => "북두칠성",
        "Dubhe" => "천추",
        "Merak" => "천선",
        "Phecda" => "천기",
        "Megrez" => "천권",
        "Alioth" => "옥형",
        "Mizar" => "개양",
        "Alkaid" => "요광",
        "Socketed" => "홈이 있는",
        "Weapons" => "무기",
        "Helmets" => "투구",
        "Armor" => "갑옷",
        "Orbs" => "보주",
        "Consumables" => "소모품",
        "Scrolls" => "이동 주문서",
        "Boxes" => "상자",
        "Shoes" => "신발",
        "Gloves" => "장갑",
        "Belts" => "허리띠",
        "Clothes" => "의복",
        "Rings" => "반지",
        "Bracelets" => "팔찌",
        "Leggings" => "각반",
        "Food" => "음식",
        "Necklaces" => "목걸이",
        "Earrings" => "귀걸이",
        "Medicine" => "약품",
        "Tools" => "도구",
        "Charms" => "부적",
        "Guardians" => "무신",
        "Beasts" => "신수",
        "Animals" => "동물",
        "Statues" => "석상",
        "Weapon" => "무기",
        "Head" => "머리",
        "Hands" => "손",
        "Waist" => "허리",
        "Ring ×2" => "반지 ×2",
        "Necklace" => "목걸이",
        "Earring" => "귀걸이",
        "Bracelet" => "팔찌",
        "Axe" => "도끼",
        "Spear" => "창",
        "Bow" => "활",
        "Sling" => "새총",
        "Cannon" => "대포",
        "Gun" => "총",
        "Dart" => "표창",
        "Fan" => "부채",
        "Needle" => "침",
        "Claw" => "손톱",
        "Beads" => "염주",
        "Mirror" => "거울",
        "Blade" => "도",
        "Sword" => "검",
        "Doll" => "인형",
        "Bell" => "방울",
        "Lute" => "월금",
        "Tablet" => "홀",
        "1st general" => "1차 장수",
        "2nd general" => "2차 장수",
        "Mercenary" => "용병",
        "Divine" => "신수",
        "Fiend" => "흉수",
        "Four Kings" => "사천왕",
        "Legend" => "전설",
        "Mounts" => "탈것",
        "Spirit" => "정령",
        "Summon" => "환수",
        "Wisdom King" => "명왕",
        "All quests" => "전체 퀘스트",
        "Daily quests" => "일일 퀘스트",
        "Weekly quests" => "주간 퀘스트",
        "Timed quests" => "기간 퀘스트",
        "Starter quests" => "초보자 퀘스트",
        "Standard quests" => "일반 퀘스트",
        "Item objectives" => "아이템 목표",
        "Monster combat" => "몬스터 토벌",
        "Mixed objectives" => "복합 목표",
        "Other conditions" => "기타 조건",
        "Unspecified objectives" => "목표 정보 없음",
        "Guides" => "안내 퀘스트",
        "Other sources" => "기타 출처",
        "Browse by cadence and source" => "반복 주기와 출처별로 보기",
        "Quests with a daily repeat cycle" => "매일 반복 가능한 퀘스트",
        "Quests with a weekly repeat cycle" => "매주 반복 가능한 퀘스트",
        "Time restricted; full reset rules unavailable" => "시간 제한 있음 · 전체 초기화 규칙 정보 없음",
        "Getting started quests" => "초보자 입문 퀘스트",
        "General quests and commissions" => "일반 퀘스트와 의뢰",
        "Quests with item objectives" => "아이템 수집 목표가 있는 퀘스트",
        "Quests with monster objectives" => "몬스터 토벌 목표가 있는 퀘스트",
        "Quests with both combat and item objectives" => "토벌과 아이템 목표를 모두 포함한 퀘스트",
        "Challenges, battle ranks and other objectives" => "도전, 전투 단계 및 기타 목표",
        "Objective type unavailable; see quest details" => "목표 유형 정보 없음 · 퀘스트 내용을 확인하세요",
        "Letter quest series" => "서신 퀘스트 시리즈",
        "Progression and game-system guides" => "성장 안내와 게임 시스템 학습",
        "Mercenary quest series" => "용병 퀘스트 시리즈",
        "Inn dispatch assignments" => "객잔 파견 임무",
        "Quests from other sources" => "기타 출처의 퀘스트",
        "Items" => "아이템",
        "Work" => "작업",
        "Other objectives" => "기타 목표",
        "Repeat cycle" => "반복 주기",
        "Daily" => "매일",
        "Weekly" => "매주",
        "By NPC" => "NPC별",
        "By quest chain" => "퀘스트 연계별",
        "By region" => "지역별",
        "NPC/map data is absent or only identifies the generic main map" => "NPC·지도 정보가 없거나 공통 지도만 기록되어 있습니다",
        "The quest-giving NPC is not recorded" => "퀘스트를 주는 NPC 정보가 없습니다",
        "No connected prerequisite or follow-up is recorded" => "연결된 선행·후속 퀘스트 정보가 없습니다",
        "No connected prerequisite or follow-up is recorded." => "연결된 선행·후속 퀘스트 정보가 없습니다.",
        "Ambiguous follow-up:" => "중복된 후속 퀘스트:",
        "Ambiguous prerequisite:" => "중복된 선행 퀘스트:",
        "Ambiguous reference" => "중복된 참조",
        "Contains a dependency cycle" => "순환 의존 관계 포함",
        "Cycle detected" => "순환 의존 관계 감지",
        "Dependency cycle" => "순환 의존 관계",
        "Current quest" => "현재 퀘스트",
        "Follow-up" => "후속 퀘스트",
        "Missing quest data" => "퀘스트 정보 누락",
        "Other branch" => "다른 분기",
        "Quest dependencies could not be loaded." => "퀘스트 의존 관계를 불러올 수 없습니다.",
        "Quest not found." => "퀘스트를 찾을 수 없습니다.",
        "Requires:" => "선행 조건:",
        "Unresolved" => "미확인 참조",
        "Unresolved follow-up:" => "미확인 후속 퀘스트:",
        "Unresolved prerequisite:" => "미확인 선행 퀘스트:",
        "Added" => "추가",
        "Fixes" => "수정",
        "Improvements" => "개선",
        "Unified" => "통합",
        "Alchemy" => "연금",
        "Blacksmith" => "대장",
        "Jeweler" => "세공",
        "NPC craft" => "NPC 제작",
        "Formation" => "진형",
        "Baekje" => "백제보물",
        "Reinforce" => "강화",
        "Letter" => "서신",
        "Intro" => "입문",
        "Dispatch" => "파견",
        "0 means free; blank means unknown" => "0은 무료, 빈칸은 가격 미정입니다",
        "Copy prices; recalculate for the required quantity" => "단가를 복사하고 필요한 수량으로 다시 계산합니다",
        "Cost per output" => "개당 비용",
        "Crafting fees" => "제작 비용",
        "Crafting quantity must be a positive whole number." => "제작 수량은 0보다 큰 정수여야 합니다.",
        "Currency: coins; per-item cost uses actual output" => "단위: 냥 · 개당 비용은 실제 생산량 기준입니다",
        "Enter a non-negative whole number, such as 1,000; leave blank for an unknown price." => "1,000과 같은 0 이상의 정수를 입력하세요. 가격 미정이면 비워 두세요.",
        "Enhancement" => "강화 단계",
        "GERSANG  /  CRAFTING COST" => "거상 / 제작 비용",
        "Game coins · supplied unit prices" => "게임 화폐 · 입력한 단가 기준",
        "Indented costs are included in their parent subtotal." => "들여쓴 비용은 상위 소계에 포함되어 있습니다.",
        "MATERIAL BREAKDOWN" => "재료 명세",
        "No success rate is recorded; costs cover planned batches, excluding failed attempts." => "성공률 정보가 없습니다. 비용은 예정된 제작 횟수 기준이며 실패 비용은 제외합니다.",
        "Not yet saved to your collection" => "아직 보관함에 저장되지 않았습니다",
        "OFFLINE · USER-SUPPLIED PRICES" => "오프라인 계산 · 사용자 입력 가격",
        "Offline recipe snapshot · Currency: coins" => "오프라인 제작법 사본 · 단위: 냥",
        "Planned output" => "예상 생산량",
        "Quantity or unit price is too large." => "수량 또는 단가가 너무 큽니다.",
        "Recipe learning fees are excluded." => "제작법 학습 비용은 제외합니다.",
        "Recipe success" => "제작 성공률",
        "Saved" => "저장됨",
        "Sub-recipe expanded and recalculated for the required quantity. Its prices can be edited independently." => "하위 제작법을 펼치고 필요한 수량으로 다시 계산했습니다. 단가는 개별 수정할 수 있습니다.",
        "Sub-recipes also exclude failed attempts and learning fees. Computed material prices allocate whole-batch costs, including surplus, to the required quantity." => "하위 제작법도 실패 및 학습 비용을 제외합니다. 재료 단가는 잉여 생산량을 포함한 전체 제작 비용을 필요한 수량에 나누어 계산합니다.",
        "Subtotal" => "소계",
        "This recipe has no material costs." => "이 제작법에는 재료 비용이 없습니다.",
        "This saved sheet is no longer available. Choose again." => "저장된 비용표를 찾을 수 없습니다. 다시 선택하세요.",
        "Top-level fee" => "주 제작법 비용",
        "Unit price" => "단가",
        "Unit prices are user estimates, not market quotes." => "단가는 사용자의 예상 가격이며 시세가 아닙니다.",
        "Unknown portions are excluded; parent subtotals include priced children." => "가격 미정 항목은 제외합니다. 상위 소계에는 가격이 입력된 하위 항목이 포함됩니다.",
        "Unpriced" => "가격 미정",
        "Unsaved changes" => "저장하지 않은 변경 사항",
        "User-supplied prices. Failed attempts and learning fees excluded." => "사용자 입력 단가입니다. 실패 및 학습 비용은 제외합니다.",
        "User-supplied prices. Recipe learning fees excluded." => "사용자 입력 단가입니다. 제작법 학습 비용은 제외합니다.",
        "actual output" => "실제 생산량",
        "batch" => "회당",
        "batches" => "회",
        "cont." => "계속",
        "cost excludes failed attempts." => "실패 비용은 제외합니다.",
        "fee" => "비용",
        "fee per batch" => "회당 제작 비용",
        "includes crafting fee" => "제작 비용 포함",
        "leaf materials priced" => "개 기본 재료 가격 입력됨",
        "leaf materials still unpriced" => "개 기본 재료 가격 미정",
        "output" => "생산량",
        "output per batch" => "회당 생산량",
        "surplus" => "잉여량",
        "— Price not supplied; known cost excludes these materials." => "— 단가 미입력 · 알려진 비용에 이 재료들은 포함되지 않습니다.",
        "Total cost" => "총비용",
        "Known cost · unpriced materials remain" => "알려진 비용 · 가격 미정 재료 있음",
        "Code copied. Paste it into Import code to restore this cost sheet offline." => "공유 코드를 복사했습니다. 코드 가져오기에 붙여 넣으면 오프라인으로 비용표를 복원할 수 있습니다.",
        "Imported the offline cost sheet. Edit it or save it to your collection." => "오프라인 비용표를 가져왔습니다. 수정하거나 보관함에 저장하세요.",
        "Exported" => "내보냄",
        "Imported a new folder containing" => "새 폴더를 가져왔습니다. 포함된 항목:",
        "cost sheets; existing collections were preserved." => "개 비용표 · 기존 보관함은 유지됩니다.",
        "Cost sheet saved to your collection." => "비용표를 보관함에 저장했습니다.",
        "Folder code copied, containing" => "폴더 공유 코드를 복사했습니다. 포함된 항목:",
        "cost sheets. The entire folder can be imported offline." => "개 비용표 · 전체 폴더를 오프라인으로 가져올 수 있습니다.",
        "Immunity" => "면역",
        "Lightning barrage" => "뇌전탄 세례",
        "Ice burst" => "빙화 폭발",
        "Bull rush" => "황소 돌진",
        "Sword dance" => "검무",
        "Bajirao's blessing" => "재상의 가호",
        "Improved cannon" => "천자총 개량",
        "Thunderbird charge" => "뇌조 돌격",
        "Torrent" => "격류진",
        "Quick draw" => "일섬",
        "Akbar's blessing" => "대제의 가호",
        "Save notices as .txt files in data/notices.\nAfter the .txt and .gts files are in place, run python -m gersang update\nor double-click update.bat, then reopen this window." => "공지를 data/notices에 .txt 파일로 저장하세요.\n.txt와 .gts 파일을 준비한 뒤 python -m gersang update를 실행하거나\nupdate.bat를 두 번 클릭하고 이 창을 다시 여세요.",
        "Invalid or overly long name" => "이름이 비어 있거나 너무 길거나 허용되지 않는 문자가 포함되어 있습니다",
        "Folder not found" => "폴더를 찾을 수 없습니다",
        "Saved calculation not found" => "저장된 계산을 찾을 수 없습니다",
        "Unsupported library version" => "지원하지 않는 보관함 버전입니다",
        "Invalid library or too many folders" => "보관함 데이터가 잘못되었거나 폴더가 너무 많습니다",
        "Invalid, duplicate, or oversized folder" => "폴더 데이터가 잘못되었거나 중복되었거나 너무 큽니다",
        "Library supports up to 256 calculations" => "보관함에는 최대 256개 계산을 저장할 수 있습니다",
        "Invalid saved calculation or duplicate ID" => "저장된 계산이 잘못되었거나 ID가 중복되었습니다",
        "Library ID range exhausted" => "보관함 ID를 더 이상 생성할 수 없습니다",
        "Folder name already exists" => "같은 이름의 폴더가 이미 있습니다",
        "Keep at least one folder" => "폴더를 하나 이상 유지하세요",
        "Move or remove its calculations first" => "폴더 안의 계산을 먼저 이동하거나 삭제하세요",
        "Unsupported folder share version" => "지원하지 않는 폴더 공유 버전입니다",
        "Invalid folder name or more than 64 calculations" => "폴더 이름이 잘못되었거나 계산이 64개를 초과합니다",
        "Invalid calculation name" => "계산 이름이 잘못되었습니다",
        "Cannot encode library" => "보관함을 인코딩할 수 없습니다",
        "Library exceeds 4 MiB" => "보관함 크기가 4 MiB를 초과합니다",
        "Invalid library format or version" => "보관함 형식 또는 버전이 잘못되었습니다",
        "Cannot encode folder" => "폴더를 인코딩할 수 없습니다",
        "Folder share exceeds 2 MiB" => "폴더 공유 데이터가 2 MiB를 초과합니다",
        "Invalid folder share format or version" => "폴더 공유 형식 또는 버전이 잘못되었습니다",
        "Cannot compress folder" => "폴더를 압축할 수 없습니다",
        "Folder share code is too large" => "폴더 공유 코드가 너무 큽니다",
        "Folder share code is too long" => "폴더 공유 코드가 너무 깁니다",
        "Unsupported folder share code format or version" => "지원하지 않는 폴더 공유 코드 형식 또는 버전입니다",
        "Folder share code is incomplete or damaged" => "폴더 공유 코드가 불완전하거나 손상되었습니다",
        "Cannot decompress folder share code" => "폴더 공유 코드를 압축 해제할 수 없습니다",
        "Oversized or invalid folder share data" => "폴더 공유 데이터가 너무 크거나 잘못되었습니다",
        "Cannot read library" => "보관함을 읽을 수 없습니다",
        "Library is not a regular file" => "보관함 경로가 일반 파일이 아닙니다",
        "Original file preserved" => "원본 파일이 보존되었습니다",
        "Cannot save library" => "보관함을 저장할 수 없습니다",
        "Quantity or cost exceeds the supported range" => "수량 또는 비용이 지원 범위를 초과합니다",
        "Quantity exceeds the supported range" => "수량이 지원 범위를 초과합니다",
        "No complete crafting recipe found" => "완전한 제작법을 찾을 수 없습니다",
        "Cannot encode calculation" => "계산을 인코딩할 수 없습니다",
        "Calculation data is too large" => "계산 데이터가 너무 큽니다",
        "Invalid calculation data" => "계산 데이터 형식이 잘못되었습니다",
        "Unsupported calculation version" => "지원하지 않는 계산 버전입니다",
        "Invalid share code" => "공유 코드 형식이 잘못되었습니다",
        "Share code is too long" => "공유 코드가 너무 깁니다",
        "Unsupported share code format or version" => "지원하지 않는 공유 코드 형식 또는 버전입니다",
        "Share code is incomplete or damaged" => "공유 코드가 불완전하거나 손상되었습니다",
        "Cannot decompress share code" => "공유 코드를 압축 해제할 수 없습니다",
        "Oversized or invalid share code data" => "공유 코드 데이터가 너무 크거나 잘못되었습니다",
        "Share code version does not match its data" => "공유 코드 버전과 데이터가 일치하지 않습니다",
        "Cannot read calculation" => "계산을 읽을 수 없습니다",
        "Cannot save calculation" => "계산을 저장할 수 없습니다",
        "Material path not found" => "재료 경로를 찾을 수 없습니다",
        "Calculations support at most 8 nested levels" => "재료는 최대 8단계까지 펼칠 수 있습니다",
        "Legacy calculations cannot contain nested recipes" => "이전 버전 계산에는 하위 제작법을 포함할 수 없습니다",
        "Invalid recipe data or quantity" => "제작법 데이터 또는 수량이 잘못되었습니다",
        "Calculations support at most 512 material rows" => "재료는 최대 512행까지 포함할 수 있습니다",
        "Nested recipes contain an item cycle" => "하위 제작법에서 같은 아이템을 순환 참조합니다",
        "Duplicate or invalid material data" => "재료 데이터가 중복되었거나 잘못되었습니다",
        "Nested recipe output does not match the material" => "하위 제작법의 생산품이 재료와 일치하지 않습니다",
        "Select an item first" => "먼저 아이템을 선택하세요",
        "Cannot create export folder" => "내보내기 폴더를 만들 수 없습니다",
        "Cannot read export time" => "내보내기 시간을 읽을 수 없습니다",
        "Cannot generate JPG" => "JPG를 생성할 수 없습니다",
        "Cannot save JPG" => "JPG를 저장할 수 없습니다",
        "JPG saved, but the image could not be opened" => "JPG를 저장했지만 이미지를 열 수 없습니다",
        "Cannot create JPG" => "JPG 파일을 만들 수 없습니다",
        "Too many exports with this name. Try again later." => "같은 이름의 내보내기 파일이 너무 많습니다. 잠시 후 다시 시도하세요.",
        "Cannot lay out cost card" => "비용 카드를 배치할 수 없습니다",
        "Cannot create cost card image." => "비용 카드 이미지를 만들 수 없습니다.",
        "Invalid cost card image dimensions." => "비용 카드 이미지 크기가 잘못되었습니다.",
        "This breakdown is too large for readable JPG cards. Share its offline code instead." => "명세가 너무 커서 읽기 쉬운 JPG 카드로 내보낼 수 없습니다. 오프라인 공유 코드를 사용하세요.",
        "This calculation has text too long for a readable JPG card. Share its offline code instead." => "계산 내용이 너무 길어 읽기 쉬운 JPG 카드에 담을 수 없습니다. 오프라인 공유 코드를 사용하세요.",
        "The collection changed in another window and was reloaded. Review it and retry." => "다른 창에서 보관함이 변경되어 다시 불러왔습니다. 확인 후 다시 시도하세요.",
        "Different fey statues" => "서로 다른 요괴 석상",
        "All monsters" => "모든 몬스터",
        "You have unsaved aliases. Save or cancel them in the Aliases tab before restarting." => "저장하지 않은 별칭이 있습니다. 다시 시작하기 전에 별칭 탭에서 저장하거나 취소하세요.",
        "Copied: " => "복사됨: ",
        "Could not copy. Try again." => "복사하지 못했습니다. 다시 시도하세요.",
        "No translated name to copy." => "복사할 번역 이름이 없습니다.",
        "Copied to clipboard" => "클립보드에 복사했습니다",
        "Couldn't copy. Select the text and copy it manually." => "복사하지 못했습니다. 텍스트를 선택하여 직접 복사하세요.",
        "Applies to" => "연마 대상",
        "Fortune fee" => "점괘 비용",
        "Enchant fee" => "주술 연마 비용",
        "Success weight" => "성공 가중치",
        "Good fortune share" => "길괘 배분",
        "Enchant materials" => "주술 재료",
        "Enchant scrolls" => "주술 비법",
        "Enchant" => "주술",
        "Base equipment" => "기본 장비",
        "Enchanted results" => "주술 장비",
        "Applicable equipment" => "적용 장비",
        "Jackpot results" => "대성공 결과",
        "Restoration scroll" => "복원 비법",
        _ => return None,
    })
}

fn dynamic_korean(en: &str) -> Option<String> {
    if let Some(level) = en.strip_prefix("Riding level ").and_then(|text| text.strip_suffix(" materials")) {
        return Some(format!("기마술 {level}레벨 재료"));
    }
    for (prefix, korean) in [
        ("Grade ", "등급 "), ("Riding ", "기마술 "),
        ("Attack +", "공격력 +"), ("Reflect ", "피해 반사 "),
        ("Absorb HP ", "체력 흡수 "), ("Mana cost -", "마력 소모 -"),
        ("Absorb mana ", "마력 흡수 "), ("Air damage +", "대공 피해 +"),
        ("Ground damage +", "대지 피해 +"), ("Damage +", "피해 +"),
        ("HP regen ", "체력 회복 "), ("Mana regen ", "마력 회복 "),
        ("Regen ", "회복 "), ("Gale ", "질풍 "),
        ("Element +", "속성 +"),
    ] {
        if let Some(value) = en.strip_prefix(prefix) {
            return Some(format!("{korean}{}", value.replace(" (earth +", " (땅 +").replace(" / 2s", " / 2초").replace(" / 3s", " / 3초")));
        }
    }
    if let Some((scope, value)) = en.split_once(" speed ") {
        return Some(format!("{scope} 이동 속도 {value}"));
    }
    None
}

const OPERATIONAL_ERRORS: &[(&str, &str, &str)] = &[
    ("Too many download redirects", "下載重新導向過多", "다운로드 리디렉션이 너무 많습니다"),
    ("Update redirected outside GitHub HTTPS", "更新重新導向至 GitHub HTTPS 以外的網址", "업데이트가 GitHub HTTPS 외부로 리디렉션되었습니다"),
    ("Staged file failed SHA-256 verification", "暫存檔案未通過 SHA-256 驗證", "준비된 파일이 SHA-256 검증을 통과하지 못했습니다"),
    ("Cannot back up", "無法備份", "백업할 수 없습니다"),
    ("Cannot observe application shutdown", "無法確認程式關閉", "앱 종료 상태를 확인할 수 없습니다"),
    ("Cannot wait for application shutdown", "無法等待程式關閉", "앱 종료를 기다릴 수 없습니다"),
    ("Cannot lock this installation for updating", "無法鎖定此安裝以進行更新", "업데이트를 위해 설치를 잠글 수 없습니다"),
    ("Another update is already in progress for this application folder", "此程式目錄已有另一項更新進行中", "이 앱 폴더에서 다른 업데이트가 이미 진행 중입니다"),
    ("Cannot open application lock", "無法開啟程式鎖定檔", "앱 잠금 파일을 열 수 없습니다"),
    ("Close every other gs-widget window and disconnect Gersang MCP in your AI apps before updating. No installed files were changed", "更新前請關閉其他 gs-widget 視窗，並中斷 AI 程式中的 Gersang MCP 連線。已安裝檔案未變更", "업데이트 전에 다른 gs-widget 창을 모두 닫고 AI 앱의 Gersang MCP 연결을 해제하세요. 설치된 파일은 변경되지 않았습니다"),
    ("Cannot initialize HTTPS", "無法初始化 HTTPS", "HTTPS를 초기화할 수 없습니다"),
    ("In-app updates currently support Windows x64 only", "程式內更新目前只支援 Windows x64", "앱 내 업데이트는 현재 Windows x64만 지원합니다"),
    ("Cannot contact GitHub", "無法連接 GitHub", "GitHub에 연결할 수 없습니다"),
    ("GitHub temporarily refused the update check (rate limit); try again later", "GitHub 暫時拒絕更新檢查（請求過多），請稍後重試", "요청 한도로 인해 GitHub가 업데이트 확인을 거부했습니다. 잠시 후 다시 시도하세요"),
    ("GitHub update check failed", "GitHub 更新檢查失敗", "GitHub 업데이트 확인에 실패했습니다"),
    ("Cannot read release metadata", "無法讀取版本資料", "릴리스 정보를 읽을 수 없습니다"),
    ("GitHub release metadata exceeded its size limit", "GitHub 版本資料超出大小限制", "GitHub 릴리스 정보가 크기 제한을 초과했습니다"),
    ("Invalid GitHub release metadata", "GitHub 版本資料無效", "GitHub 릴리스 정보가 잘못되었습니다"),
    ("Release tag must start with v", "版本標籤必須以 v 開頭", "릴리스 태그는 v로 시작해야 합니다"),
    ("Release tag is not a semantic version", "版本標籤格式不正確", "릴리스 태그의 버전 형식이 잘못되었습니다"),
    ("Only stable release versions without build metadata are supported", "僅支援不含建置資訊的正式版本", "빌드 메타데이터가 없는 정식 버전만 지원합니다"),
    ("Invalid installed application version", "目前安裝版本無效", "설치된 앱 버전이 잘못되었습니다"),
    ("Release has duplicate Windows update assets", "版本包含重複的 Windows 更新檔", "릴리스에 중복된 Windows 업데이트 파일이 있습니다"),
    ("Update asset URL does not match this repository and version", "更新檔網址與此專案及版本不符", "업데이트 파일 주소가 저장소 및 버전과 일치하지 않습니다"),
    ("Update archive has an invalid size", "更新壓縮檔大小無效", "업데이트 압축 파일 크기가 잘못되었습니다"),
    ("GitHub has not supplied a SHA-256 digest for this asset; try again later", "GitHub 尚未提供更新檔 SHA-256，請稍後重試", "GitHub가 아직 파일의 SHA-256을 제공하지 않았습니다. 잠시 후 다시 시도하세요"),
    ("GitHub supplied an invalid SHA-256 digest", "GitHub 提供的 SHA-256 無效", "GitHub가 제공한 SHA-256이 잘못되었습니다"),
    ("Cannot locate the application directory", "找不到程式目錄", "앱 폴더를 찾을 수 없습니다"),
    ("Cannot access the application directory", "無法存取程式目錄", "앱 폴더에 접근할 수 없습니다"),
    ("Cannot allocate a unique update directory", "無法建立獨立更新目錄", "고유한 업데이트 폴더를 만들 수 없습니다"),
    ("Update download failed", "更新下載失敗", "업데이트 다운로드에 실패했습니다"),
    ("Update download size differs from GitHub metadata", "更新下載大小與 GitHub 資料不符", "다운로드 크기가 GitHub 정보와 일치하지 않습니다"),
    ("Update download was interrupted", "更新下載中斷", "업데이트 다운로드가 중단되었습니다"),
    ("Update download exceeded its declared size", "更新下載超出聲明大小", "업데이트 다운로드가 명시된 크기를 초과했습니다"),
    ("Cannot save update; check free disk space", "無法儲存更新，請檢查磁碟空間", "업데이트를 저장할 수 없습니다. 디스크 여유 공간을 확인하세요"),
    ("Update failed SHA-256 or size verification; installed files were not changed", "更新 SHA-256 或大小驗證失敗；已安裝檔案未變更", "업데이트의 SHA-256 또는 크기 검증에 실패했습니다. 설치된 파일은 변경되지 않았습니다"),
    ("Invalid archive path", "壓縮檔路徑無效", "압축 파일 경로가 잘못되었습니다"),
    ("Unsafe Windows archive path", "Windows 壓縮檔路徑不安全", "안전하지 않은 Windows 압축 파일 경로입니다"),
    ("Reserved Windows archive path", "壓縮檔使用 Windows 保留路徑", "압축 파일에 Windows 예약 경로가 있습니다"),
    ("Archive includes a file outside the application data allow-list", "壓縮檔包含程式允許範圍外的檔案", "압축 파일에 앱 허용 목록 밖의 파일이 있습니다"),
    ("Invalid update ZIP", "更新 ZIP 無效", "업데이트 ZIP 파일이 잘못되었습니다"),
    ("Update ZIP contains an invalid number of files", "更新 ZIP 的檔案數量無效", "업데이트 ZIP의 파일 수가 잘못되었습니다"),
    ("Encrypted archives, links and special files are not supported", "不支援加密壓縮檔、連結與特殊檔案", "암호화된 압축 파일, 링크 및 특수 파일은 지원하지 않습니다"),
    ("ZIP contains a symlink or special file", "ZIP 包含符號連結或特殊檔案", "ZIP에 심볼릭 링크 또는 특수 파일이 있습니다"),
    ("ZIP contains duplicate or case-colliding paths", "ZIP 包含重複或大小寫衝突的路徑", "ZIP에 중복되거나 대소문자가 충돌하는 경로가 있습니다"),
    ("ZIP expanded size overflow", "ZIP 解壓大小溢位", "ZIP 압축 해제 크기가 범위를 초과했습니다"),
    ("ZIP expanded size exceeded its limit", "ZIP 解壓大小超出限制", "ZIP 압축 해제 크기가 제한을 초과했습니다"),
    ("ZIP file is also used as a directory", "ZIP 內檔案路徑同時被用作目錄", "ZIP에서 같은 경로가 파일과 폴더로 사용됩니다"),
    ("ZIP is missing client assets", "ZIP 缺少客戶端資源", "ZIP에 클라이언트 리소스가 없습니다"),
    ("Invalid archive destination", "壓縮檔目的地無效", "압축 해제 대상 경로가 잘못되었습니다"),
    ("ZIP entry exceeded its declared size", "ZIP 內容超出聲明大小", "ZIP 항목이 명시된 크기를 초과했습니다"),
    ("Update cannot replace linked paths", "更新無法替換連結路徑", "업데이트가 링크 경로를 교체할 수 없습니다"),
    ("Update found a special file", "更新發現特殊檔案", "업데이트 중 특수 파일을 발견했습니다"),
    ("Application directory is not a regular directory", "程式目錄不是一般目錄", "앱 폴더가 일반 폴더가 아닙니다"),
    ("Duplicate staged file", "暫存更新檔案重複", "준비된 업데이트 파일이 중복되었습니다"),
    ("Staged file changed size", "暫存檔案大小已變更", "준비된 파일의 크기가 변경되었습니다"),
    ("Staged file changed after verification", "暫存檔案在驗證後被變更", "검증 후 준비된 파일이 변경되었습니다"),
    ("Invalid staged filename", "暫存檔案名稱無效", "준비된 파일 이름이 잘못되었습니다"),
    ("Unexpected staged file", "發現非預期的暫存檔案", "예상하지 못한 준비 파일입니다"),
    ("Staged update is missing client assets", "暫存更新缺少客戶端資源", "준비된 업데이트에 클라이언트 리소스가 없습니다"),
    ("Update executable has no valid DOS header", "更新程式缺少有效 DOS 標頭", "업데이트 실행 파일에 유효한 DOS 헤더가 없습니다"),
    ("Update executable is not a Windows executable", "更新程式不是 Windows 執行檔", "업데이트 파일이 Windows 실행 파일이 아닙니다"),
    ("Update executable has an invalid PE header offset", "更新程式 PE 標頭位置無效", "업데이트 실행 파일의 PE 헤더 위치가 잘못되었습니다"),
    ("Update executable has a truncated PE header", "更新程式 PE 標頭不完整", "업데이트 실행 파일의 PE 헤더가 불완전합니다"),
    ("Update executable must be a Windows x64 PE32+ application", "更新程式必須是 Windows x64 PE32+ 程式", "업데이트 실행 파일은 Windows x64 PE32+ 앱이어야 합니다"),
    ("New application cannot run its installation check", "新版程式無法執行安裝檢查", "새 앱이 설치 검사를 실행할 수 없습니다"),
    ("New application failed its installation check", "新版程式未通過安裝檢查", "새 앱이 설치 검사를 통과하지 못했습니다"),
    ("New application did not finish its installation check within 15 seconds", "新版程式未在 15 秒內完成安裝檢查", "새 앱이 15초 안에 설치 검사를 마치지 못했습니다"),
    ("This update helper has already started", "更新輔助程式已啟動", "업데이트 도우미가 이미 시작되었습니다"),
    ("Cannot reset updater state", "無法重設更新狀態", "업데이트 상태를 초기화할 수 없습니다"),
    ("Cannot prepare update helper", "無法準備更新輔助程式", "업데이트 도우미를 준비할 수 없습니다"),
    ("Cannot start update helper", "無法啟動更新輔助程式", "업데이트 도우미를 시작할 수 없습니다"),
    ("Update helper could not start", "更新輔助程式無法啟動", "업데이트 도우미를 시작하지 못했습니다"),
    ("Update helper did not become ready. The application has not been changed", "更新輔助程式尚未就緒，程式未變更", "업데이트 도우미가 준비되지 않았습니다. 앱은 변경되지 않았습니다"),
    ("Cannot stop update helper; retry is disabled until the application restarts", "無法停止更新輔助程式；請重新啟動程式後再試", "업데이트 도우미를 중지할 수 없습니다. 앱을 다시 시작한 후 재시도하세요"),
    ("Cannot confirm helper shutdown", "無法確認更新輔助程式已關閉", "업데이트 도우미의 종료를 확인할 수 없습니다"),
    ("Cannot find update helper directory", "找不到更新輔助程式目錄", "업데이트 도우미 폴더를 찾을 수 없습니다"),
    ("Invalid update helper location", "更新輔助程式位置無效", "업데이트 도우미 위치가 잘못되었습니다"),
    ("Missing update plan", "缺少更新計劃", "업데이트 계획이 없습니다"),
    ("Update plan must belong to this helper", "更新計劃不屬於此輔助程式", "업데이트 계획이 이 도우미의 것이 아닙니다"),
    ("Cannot find installation directory", "找不到安裝目錄", "설치 폴더를 찾을 수 없습니다"),
    ("Update plan is too large", "更新計劃過大", "업데이트 계획이 너무 큽니다"),
    ("Invalid update plan", "更新計劃無效", "업데이트 계획이 잘못되었습니다"),
    ("Unsupported update plan", "不支援的更新計劃", "지원하지 않는 업데이트 계획입니다"),
    ("Update was cancelled before the app closed", "更新在程式關閉前已取消", "앱이 닫히기 전에 업데이트가 취소되었습니다"),
    ("Update was cancelled; installed files were not changed", "更新已取消；已安裝檔案未變更", "업데이트가 취소되었습니다. 설치된 파일은 변경되지 않았습니다"),
    ("New version is installed but could not start", "新版已安裝，但無法啟動", "새 버전이 설치되었지만 시작하지 못했습니다"),
    ("Cannot start the new app", "無法啟動新版程式", "새 앱을 시작할 수 없습니다"),
    ("GERSANG_DB must point to an existing gersang.db file", "GERSANG_DB 必須指向現有的 gersang.db 檔案", "GERSANG_DB는 존재하는 gersang.db 파일을 가리켜야 합니다"),
    ("Release is missing", "版本缺少檔案", "릴리스에 파일이 없습니다"),
    ("ZIP is missing required file", "ZIP 缺少必要檔案", "ZIP에 필수 파일이 없습니다"),
    ("Staged update is missing", "暫存更新缺少檔案", "준비된 업데이트에 파일이 없습니다"),
    ("Cannot inspect", "無法檢查", "확인할 수 없습니다"),
    ("Cannot remove", "無法刪除", "삭제할 수 없습니다"),
    ("Cannot restore", "無法復原", "복원할 수 없습니다"),
    ("Cannot install", "無法安裝", "설치할 수 없습니다"),
    ("Refusing to remove linked path", "無法刪除連結路徑", "링크 경로를 삭제할 수 없습니다"),
    ("Corrupt ZIP entry", "ZIP 內容損壞", "손상된 ZIP 항목입니다"),
    ("Recovery files", "復原檔案", "복구 파일"),
    ("The previous application was restored", "已復原先前的程式", "이전 앱이 복원되었습니다"),
    ("The previous version was restored", "已復原先前版本", "이전 버전이 복원되었습니다"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_only_the_primary_language() {
        assert_eq!(ui_text("zh-ko", "材料", "Materials"), "材料");
        assert_eq!(ui_text("en-ko", "材料", "Materials"), "Materials");
        assert_eq!(ui_text("ko-zh", "材料", "Materials"), "재료");
        assert_eq!(ui_text("ko", "防禦力", "Armor"), "방어력");
        assert_eq!(ui_text("ko", "盔甲", "Armor"), "갑옷");
        assert_eq!(ui_text("ko", "符", "Charm"), "부적");
        assert_eq!(ui_text("ko", "非靜態資料", "Unknown catalogue text"), "Unknown catalogue text");
    }

    #[test]
    fn dynamic_game_labels_keep_their_values() {
        assert_eq!(ui_text("ko", "馬術 5 級材料", "Riding level 5 materials"), "기마술 5레벨 재료");
        assert_eq!(ui_text("ko", "體力回復 100 / 2秒", "HP regen 100 / 2s"), "체력 회복 100 / 2초");
        assert_eq!(ui_text("ko", "屬性 +3（土 +4）", "Element +3 (earth +4)"), "속성 +3 (땅 +4)");
        assert_eq!(ui_text("ko", "全隊移速 +5", "전체 파티 speed +5"), "전체 파티 이동 속도 +5");
    }

    #[test]
    fn legacy_errors_preserve_diagnostics_paths_and_versions() {
        let error = "無法儲存計算 / Cannot save calculation: permission denied";
        assert_eq!(localized_error("zh", error), "無法儲存計算: permission denied");
        assert_eq!(localized_error("en", error), "Cannot save calculation: permission denied");
        assert_eq!(localized_error("ko-en", error), "계산을 저장할 수 없습니다: permission denied");
        let error = "不支援的計算版本 99 / Unsupported calculation version；原有檔案已保留 / Original file preserved: /tmp/my calculation.json";
        assert_eq!(localized_error("ko", error), "지원하지 않는 계산 버전입니다 99 · 원본 파일이 보존되었습니다: /tmp/my calculation.json");
        assert_eq!(localized_error("en", "無法儲存 JPG：disk full"), "Cannot save JPG: disk full");
        assert_eq!(localized_error("ko", "無法儲存 JPG：disk full"), "JPG를 저장할 수 없습니다: disk full");
        assert_eq!(localized_error("ko", "external / service: unavailable"), "external / service: unavailable");
    }

    #[test]
    fn input_errors_can_change_language_after_validation() {
        let zh = "製作數量必須是大於 0 的整數。";
        let ko = localized_error("ko", zh);
        assert_eq!(ko, "제작 수량은 0보다 큰 정수여야 합니다.");
        assert_eq!(localized_error("en", &ko), "Crafting quantity must be a positive whole number.");
    }

    #[test]
    fn update_errors_translate_app_messages_and_retain_external_details() {
        assert_eq!(localized_error("ko", "In-app updates currently support Windows x64 only"), "앱 내 업데이트는 현재 Windows x64만 지원합니다");
        assert_eq!(localized_error("ko", "Cannot contact GitHub: connection timed out"), "GitHub에 연결할 수 없습니다: connection timed out");
        assert_eq!(localized_error("zh", "Release is missing gs-widget-v9-windows-x64.zip"), "版本缺少檔案 gs-widget-v9-windows-x64.zip");
        assert_eq!(localized_error("ko", "GERSANG_DB must point to an existing gersang.db file"), "GERSANG_DB는 존재하는 gersang.db 파일을 가리켜야 합니다");
        assert_eq!(localized_error("ko", "Cannot install file.exe: access denied. The previous application was restored"), "설치할 수 없습니다 file.exe: access denied · 이전 앱이 복원되었습니다");
    }
}
