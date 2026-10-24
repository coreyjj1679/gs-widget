//! Formation bonus points, independent from gold-cost calculations.
//!
//! Base attributes and recruitment edges come from the bundled catalog. Rules
//! and level growth are explicitly sourced below; absent history is never
//! mistaken for a known zero. All division is integer division after summing
//! the inputs for ONE promotion, then that result feeds the next promotion.
use std::collections::{BTreeMap, BTreeSet};

use super::character_calculator::CharacterCalcCandidate;
use super::character_calculator_browse::CharacterCalcGroup;
use super::hero::{hero_names, hero_rank_order};
use super::{hero_rank_label, Catalog};

#[path = "bonus_minimum.rs"]
mod minimum;
pub use minimum::{calculate_minimum_perfect_plan, BonusMinimumPlan};

const GROWTH_SOURCE: &str =
    "Client ExpForLevelUp.dat (gersang.gcs member 7); QuestText.dat:00002026";
const GENERAL_SOURCE: &str = "https://www.52jushang.com/qita/hongli.asp";
const MONSTER_SOURCE: &str = "https://www.52jushang.com/yongbing/5monster.asp";
const INDIA_SOURCE: &str = "https://www.gersangjjang.com/point/budong.asp";
const INDIA_CONVERSION_SOURCE: &str =
    "Client JobChangeList_Nepar_India.txt; OnlineText.dat:00015131–00015139";
const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 128;
const MAX_POINTS: i64 = 1_000_000;

pub fn bonus_text<'a>(lang: &str, zh: &'a str, en: &'a str, ko: &'a str) -> &'a str {
    match super::primary_lang(lang) {
        "ko" => ko,
        "en" => en,
        _ => zh,
    }
}

/// Client JobChangeList_Nepar_India.txt (gersang.gcs member 749), rows with
/// Upgrade Step=0 and four Need Mon alternatives, all requiring level 100.
/// The client uses advanced-mercenary IDs; member 770 maps each advanced ID to
/// the ordinary catalog character ID below. U88 is already a catalog ID.
/// Bundled OnlineText.dat 00015131..00015139 independently names these routes.
/// These are choices, never four simultaneous synthesis ingredients.
fn indian_predecessors(code: &str) -> &'static [&'static str] {
    match code {
        // U47, U51, U116, U62
        "D81" => &["U01", "U11", "U100", "U59"],
        // U44, U76, U111, U53
        "D115" => &["U02", "U66", "U92", "U13"],
        // U115, U49, U75, U64
        "D117" => &["U86", "U09", "U65", "U55"],
        // U114, U113, U110, U112
        "F7" => &["U94", "U96", "U90", "U98"],
        // U48, U78, U52, U61
        "F5" => &["U03", "U68", "U12", "U57"],
        // U45, U50, U63, U77
        "F11" => &["U05", "U10", "U58", "U67"],
        // U46, U88, U60, U79
        "F13" => &["U08", "U88", "U56", "U69"],
        // U114, U63, U77, U53
        "F34" => &["U94", "U58", "U67", "U13"],
        // U47, U49, U62, U75
        "F36" => &["U01", "U09", "U59", "U65"],
        _ => &[],
    }
}

/// Explicit source requirements from the bundled client's MercenaryList.txt
/// (member 748), GeneralList.txt (739), MercenaryList_indo.txt (749), and
/// RenovateMecenary.txt (758). SoldierUp.txt (770) maps advanced ordinary IDs
/// to catalog IDs. India training's 80-level rule takes precedence over the
/// generic table for T44/T45; MI_GetJob.txt notes 15304/15305 confirm it.
/// Unknown recipes retain their separately documented arithmetic domain.
fn promotion_minimum(target: &str, source: &str) -> Option<i64> {
    match (target, source) {
        ("D116", "D115")
        | ("D118", "D117")
        | ("D82", "D81")
        | ("F10", "F13")
        | ("F12", "F11")
        | ("F35", "F34")
        | ("F37", "F36")
        | ("F6", "F5")
        | ("F8", "F7")
        | ("U30", "U02")
        | ("U33", "U01")
        | ("U39", "U10")
        | ("U42", "U13")
        | ("U70", "U55")
        | ("U71", "U59")
        | ("U72", "U56")
        | ("U81", "U69")
        | ("U83", "U67")
        | ("U91", "U90")
        | ("U95", "U94")
        | ("U97", "U96") => Some(40),
        ("U101", "U100")
        | ("U32", "U05")
        | ("U35", "U03")
        | ("U36", "U08")
        | ("U37", "U11")
        | ("U40", "U09")
        | ("U43", "U12")
        | ("U73", "U57")
        | ("U74", "U58")
        | ("U80", "U65")
        | ("U82", "U66")
        | ("U84", "U68")
        | ("U87", "U86")
        | ("U89", "U88")
        | ("U93", "U92")
        | ("U99", "U98") => Some(50),
        ("T2", "U32")
        | ("T20", "U42")
        | ("T24", "D82")
        | ("T25", "D116")
        | ("T26", "D118")
        | ("T27", "F6")
        | ("T28", "F8")
        | ("T37", "F10")
        | ("T38", "F12")
        | ("T44", "F35")
        | ("T45", "F37")
        | ("T5", "U83")
        | ("U104", "U39")
        | ("U105", "U97")
        | ("U106", "U91")
        | ("U109", "U99") => Some(80),
        ("T1", "U87")
        | ("T19", "U35")
        | ("T21", "U71")
        | ("T22", "U82")
        | ("T3", "U40")
        | ("T4", "U89")
        | ("T6", "U80")
        | ("T7", "U74")
        | ("T8", "U73")
        | ("U102", "U30")
        | ("U103", "U95")
        | ("U107", "U81")
        | ("U108", "U93") => Some(90),
        ("D115", "U02")
        | ("D115", "U13")
        | ("D115", "U66")
        | ("D115", "U92")
        | ("D117", "U09")
        | ("D117", "U55")
        | ("D117", "U65")
        | ("D117", "U86")
        | ("D78", "M49")
        | ("D79", "M06")
        | ("D81", "U01")
        | ("D81", "U100")
        | ("D81", "U11")
        | ("D81", "U59")
        | ("F1", "M54")
        | ("F11", "U05")
        | ("F11", "U10")
        | ("F11", "U58")
        | ("F11", "U67")
        | ("F13", "U08")
        | ("F13", "U56")
        | ("F13", "U69")
        | ("F13", "U88")
        | ("F14", "U85")
        | ("F15", "M33")
        | ("F2", "M37")
        | ("F3", "M34")
        | ("F34", "U13")
        | ("F34", "U58")
        | ("F34", "U67")
        | ("F34", "U94")
        | ("F36", "U01")
        | ("F36", "U09")
        | ("F36", "U59")
        | ("F36", "U65")
        | ("F4", "M82")
        | ("F5", "U03")
        | ("F5", "U12")
        | ("F5", "U57")
        | ("F5", "U68")
        | ("F7", "U90")
        | ("F7", "U94")
        | ("F7", "U96")
        | ("F7", "U98")
        | ("F9", "M61")
        | ("T29", "U36")
        | ("T30", "U101")
        | ("T31", "U72")
        | ("T32", "U43")
        | ("T33", "U33")
        | ("T34", "U37")
        | ("T35", "U84")
        | ("T36", "U70")
        | ("T40", "U102")
        | ("T41", "U104")
        | ("T42", "U108")
        | ("T43", "U107")
        | ("T46", "U103")
        | ("T47", "U109")
        | ("T48", "U105")
        | ("T53", "U106")
        | ("T54", "T2")
        | ("T55", "T7")
        | ("T56", "T6")
        | ("T57", "T3")
        | ("T58", "T19")
        | ("T59", "T21")
        | ("T60", "T22")
        | ("T61", "T25")
        | ("T63", "T4")
        | ("T67", "T24")
        | ("T68", "T8")
        | ("T69", "T20")
        | ("T75", "T5")
        | ("T76", "T38")
        | ("T78", "T26")
        | ("T82", "T37")
        | ("T91", "T44")
        | ("T96", "T45")
        | ("T98", "T28")
        | ("T99", "T27")
        | ("V01", "T1") => Some(100),
        // Current MI_GetJob notes 42045..42048 and 42551/42552/42784/42785.
        ("V06", "T49")
        | ("V08", "T50")
        | ("V09", "T51")
        | ("V07", "T52")
        | ("V26", "T89")
        | ("V27", "T88")
        | ("V28", "T87")
        | ("V29", "T86") => Some(260),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct BonusNode {
    /// Path identity, not character identity: repeated ingredients stay independent.
    pub key: String,
    pub code: String,
    pub title: String,
    pub base_stats: i64,
    pub rank: i64,
    pub nation: i64,
    /// All of these ingredients are required (AND).
    pub children: Vec<BonusNode>,
    /// Alternative former mercenaries: exactly one is used (OR).
    pub predecessor_options: Vec<BonusNode>,
    pub divisor: i64,
    pub white_divisor: i64,
    pub rule: String,
    pub source: String,
    pub white_enabled: bool,
    pub manual_required: bool,
    pub warning: String,
    /// Some conversions use source levels instead of source attribute totals.
    pub uses_levels: bool,
    pub min_level: i64,
    pub max_level: i64,
    pub starting_bonus: i64,
}

impl BonusNode {
    pub fn allows_level(&self, level: i64) -> bool {
        (self.min_level..=self.max_level).contains(&level)
            // QuestText.dat 00006676: sealable statues begin at 100 and are
            // recorded in ten-level steps. Generic leaves model those statues.
            && (!self.code.starts_with("statue:") || level % 10 == 0)
    }

    pub fn default_input(&self) -> BonusInput {
        BonusInput {
            level: self.max_level.min(250).max(self.min_level).to_string(),
            ..BonusInput::default()
        }
    }

    /// Blank chooses the first listed route; an unknown explicit code never
    /// silently changes the user's selected history.
    pub fn selected_predecessor(&self, input: &BonusInput) -> Option<&BonusNode> {
        if input.predecessor.trim().is_empty() {
            self.predecessor_options.first()
        } else {
            self.predecessor_options
                .iter()
                .find(|option| option.code == input.predecessor)
        }
    }

    pub fn effective_children(&self, inputs: &BTreeMap<String, BonusInput>) -> Vec<&BonusNode> {
        if self.predecessor_options.is_empty() {
            self.children.iter().collect()
        } else {
            let defaults = self.default_input();
            self.selected_predecessor(inputs.get(&self.key).unwrap_or(&defaults))
                .into_iter()
                .collect()
        }
    }
}

#[derive(Clone, Debug)]
pub struct BonusInput {
    pub level: String,
    /// Blank computes from predecessors. Explicit 0 is a known zero.
    pub inherited: String,
    /// Current white-robe points, separate from inherited formation points.
    pub white: String,
    /// Source character code for a one-of-many conversion; blank uses the first.
    pub predecessor: String,
    /// Explicitly confirms all current white-robe points were earned before
    /// the four-country mercenary converted to India, without later resets.
    pub white_before_india: bool,
}

impl Default for BonusInput {
    fn default() -> Self {
        Self {
            level: "250".into(),
            inherited: String::new(),
            white: "0".into(),
            predecessor: String::new(),
            white_before_india: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BonusLine {
    pub key: String,
    pub level: i64,
    pub base_stats: i64,
    pub level_points: i64,
    /// This form's incoming formation bonus, excluding white-robe points.
    pub bonus: i64,
    pub white: i64,
    /// Pure attributes, excluding white-robe points and equipment.
    pub total_stats: i64,
}

#[derive(Clone, Debug)]
pub struct BonusResult {
    pub bonus: i64,
    /// Postorder: each prerequisite precedes the form it produces.
    pub lines: Vec<BonusLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BonusMaximumScope {
    /// Verified ceiling without rebirth, including full white-robe points when
    /// their effect is represented by this target's complete formula.
    NoRebirth,
    /// Useful baseline only: the current model cannot prove every white-robe
    /// history. Equality must not be presented as globally perfect.
    NoWhiteRobeOrRebirth,
}

#[derive(Clone, Debug)]
pub struct BonusMaximum {
    pub bonus: i64,
    pub scope: BonusMaximumScope,
}

impl BonusMaximum {
    pub fn supports_perfect(&self) -> bool {
        self.scope == BonusMaximumScope::NoRebirth
    }
}

/// Maximum for the selected route, independently of incomplete level drafts or
/// manually entered historical bonuses. Each used material reaches its own
/// cap, including 200-level statues and 260-level Four/Wisdom Kings.
pub fn calculate_max_bonus(
    tree: &BonusNode,
    inputs: &BTreeMap<String, BonusInput>,
) -> Result<BonusMaximum, String> {
    let full_white_legend = tree.rank == 11 && !tree.children.is_empty();
    let scope = if full_white_legend || matches!(tree.rank, 4..=9 | 12) {
        BonusMaximumScope::NoRebirth
    } else {
        BonusMaximumScope::NoWhiteRobeOrRebirth
    };
    let maximum = maximum_bonus_inputs(tree, inputs)?;
    Ok(BonusMaximum {
        bonus: calculate_bonus(tree, &maximum)?.bonus,
        scope,
    })
}

/// Shared all-cap reference for both maximum verification and exact planning.
/// Historical overrides are intentionally cleared; only route choices survive.
fn maximum_bonus_inputs(
    tree: &BonusNode,
    inputs: &BTreeMap<String, BonusInput>,
) -> Result<BTreeMap<String, BonusInput>, String> {
    let full_white_legend = tree.rank == 11 && !tree.children.is_empty();
    fn fill(
        node: &BonusNode,
        source: &BTreeMap<String, BonusInput>,
        maximum: &mut BTreeMap<String, BonusInput>,
        full_white: bool,
        level_only: bool,
        depth: usize,
    ) -> Result<(), String> {
        if depth > MAX_DEPTH || maximum.len() >= MAX_NODES || maximum.contains_key(&node.key) {
            return Err("紅利鏈資料無效 / Invalid bonus chain / 잘못된 보너스 경로".into());
        }
        if !level_only && node.manual_required {
            return Err(invalid(node,
                "缺少已確認的前置資料，無法計算上限 / Verified predecessor data is unavailable; the maximum cannot be calculated / 선행 정보 미확인으로 최대 보너스를 계산할 수 없습니다"));
        }
        let input = BonusInput {
            level: node.max_level.to_string(),
            white: if full_white && node.white_enabled {
                "300"
            } else {
                "0"
            }
            .into(),
            white_before_india: full_white && node.white_enabled && node.nation == 10000,
            predecessor: source
                .get(&node.key)
                .map(|value| value.predecessor.clone())
                .unwrap_or_default(),
            ..BonusInput::default()
        };
        if !level_only
            && !node.predecessor_options.is_empty()
            && node.selected_predecessor(&input).is_none()
        {
            return Err(invalid(node,
                "請選擇有效的轉職來源 / Choose a valid conversion source / 유효한 전직 용병을 선택하세요"));
        }
        maximum.insert(node.key.clone(), input);
        if !level_only {
            for child in node.effective_children(source) {
                fill(
                    child,
                    source,
                    maximum,
                    full_white,
                    node.uses_levels,
                    depth + 1,
                )?;
            }
        }
        Ok(())
    }
    let mut maximum = BTreeMap::new();
    fill(tree, inputs, &mut maximum, full_white_legend, false, 0)?;
    Ok(maximum)
}

/// Cumulative unallocated attribute points earned since level one.
/// Client QuestText.dat 00002026 supplies the base +4 per level. The production
/// ExpForLevelUp.dat (gcs member 7) supplies the extra stat points in these bands.
/// All 259 increments from level 2 through 260 match that client table.
pub fn bonus_level_points(level: i64) -> Result<i64, String> {
    if !(1..=260).contains(&level) {
        return Err("等級須為 1–260 / Level must be 1–260 / 레벨은 1–260이어야 합니다".into());
    }
    const BANDS: &[(i64, i64, i64)] = &[
        (2, 119, 4),
        (120, 129, 5),
        (130, 134, 6),
        (135, 139, 7),
        (140, 144, 8),
        (145, 149, 9),
        (150, 159, 10),
        (160, 169, 11),
        (170, 174, 12),
        (175, 179, 13),
        (180, 195, 14),
        (196, 200, 15),
        (201, 205, 16),
        (206, 210, 17),
        (211, 215, 18),
        (216, 220, 19),
        (221, 250, 20),
        (251, 260, 21),
    ];
    Ok(BANDS
        .iter()
        .map(|&(start, end, points)| (level.min(end) - start + 1).max(0) * points)
        .sum())
}

fn integer(raw: &str, maximum: i64) -> Option<i64> {
    let raw = raw.trim();
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<i64>().ok().filter(|value| *value <= maximum)
}

fn invalid(node: &BonusNode, label: &str) -> String {
    format!("{} · {label}", node.title)
}

pub fn calculate_bonus(
    tree: &BonusNode,
    inputs: &BTreeMap<String, BonusInput>,
) -> Result<BonusResult, String> {
    fn walk(
        node: &BonusNode,
        inputs: &BTreeMap<String, BonusInput>,
        depth: usize,
        seen: &mut BTreeSet<String>,
        lines: &mut Vec<BonusLine>,
        root: bool,
        level_only: bool,
        indian_white_history: i64,
    ) -> Result<BonusLine, String> {
        if depth > MAX_DEPTH
            || !seen.insert(node.key.clone())
            || seen.len() > MAX_NODES
            || node.base_stats < 0
            || node.base_stats > MAX_POINTS
        {
            return Err("紅利鏈資料無效 / Invalid bonus chain / 잘못된 보너스 경로".into());
        }
        let defaults = node.default_input();
        let input = inputs.get(&node.key).unwrap_or(&defaults);
        // The output is a newly formed level-one character; its level is not an input.
        let level = if root {
            1
        } else {
            integer(&input.level, node.max_level)
                .filter(|v| node.allows_level(*v))
                .ok_or_else(|| {
                    if node.code.starts_with("statue:") {
                        return invalid(node, "石像等級須為 100–200，以 10 級為單位 / Statue levels must be 100–200 in steps of 10 / 석상 레벨은 100–200, 10레벨 단위");
                    }
                    invalid(
                        node,
                        &format!(
                            "等級須為 {}–{} / Level must be {}–{} / 레벨 {}–{}",
                            node.min_level,
                            node.max_level,
                            node.min_level,
                            node.max_level,
                            node.min_level,
                            node.max_level
                        ),
                    )
                })?
        };
        let level_points = bonus_level_points(level)?;
        let white = if root || level_only || input.white.trim().is_empty() {
            0
        } else {
            integer(&input.white, 300).ok_or_else(|| invalid(node,
                "白衣點數須為 0–300 整數 / White-robe points must be an integer from 0 to 300 / 백의 수치는 0–300 정수"))?
        };
        if white > 0 && !node.white_enabled {
            return Err(invalid(node, "此階段不接受白衣輸入 / White-robe points are not an input at this stage / 이 단계에는 백의 수치를 입력할 수 없습니다"));
        }
        let manual = if root || level_only || input.inherited.trim().is_empty() {
            None
        } else {
            Some(integer(&input.inherited, MAX_POINTS).ok_or_else(|| invalid(node,
                "繼承紅利須為非負整數 / Inherited bonus must be a nonnegative integer / 상속 보너스는 0 이상의 정수"))?)
        };
        if node.rank == 0
            && node.nation == 10000
            && manual.is_some_and(|value| value != 0 && !(100..=250).contains(&value))
        {
            return Err(invalid(node,
                "印度傭兵的轉換紅利須為 100–250，白衣重置後填 0 / Indian conversion bonus must be 100–250, or 0 after a white-robe reset / 인도 전환 보너스는 100–250 또는 백의 초기화 후 0"));
        }
        if !level_only && manual.is_none() && node.manual_required {
            return Err(invalid(node, &node.warning));
        }
        if manual.is_none() && white > 0 && node.nation == 10000 && !input.white_before_india {
            return Err(invalid(node,
                "請確認白衣在四國兵階段完成，或填入實際繼承紅利（不含白衣） / Confirm white-robe training preceded India, or enter the actual inherited bonus excluding white-robe points / 인도 전환 전 백의 완료를 확인하거나 백의를 제외한 실제 상속 보너스를 입력하세요"));
        }
        let indian_white_history = if node.nation == 10000 && input.white_before_india && white > 0
        {
            white
        } else {
            indian_white_history
        };
        let mut children = Vec::new();
        // A known incoming value replaces its earlier history. Its hidden drafts
        // must not block an otherwise complete calculation for an existing unit.
        if manual.is_none() && !level_only {
            if !node.predecessor_options.is_empty() && node.selected_predecessor(input).is_none() {
                return Err(invalid(node,
                    "請選擇有效的轉職來源 / Choose a valid conversion source / 유효한 전직 용병을 선택하세요"));
            }
            for child in node.effective_children(inputs) {
                children.push(walk(
                    child,
                    inputs,
                    depth + 1,
                    seen,
                    lines,
                    false,
                    node.uses_levels,
                    indian_white_history,
                )?);
            }
        }
        let bonus = match manual {
            Some(value) => value,
            None if children.is_empty() => node.starting_bonus,
            None => {
                if node.divisor <= 0 {
                    return Err(invalid(node, "尚無已確認公式，請填入實際繼承紅利 / Formula unverified; enter the actual inherited bonus / 공식 미확인: 실제 상속 보너스를 입력하세요"));
                }
                let pure = children
                    .iter()
                    .try_fold(0_i64, |sum, child| {
                        sum.checked_add(if node.uses_levels {
                            child.level
                        } else {
                            child.total_stats
                                + if node.nation == 10000 && matches!(node.rank, 1 | 2) {
                                    indian_white_history
                                } else {
                                    0
                                }
                        })
                    })
                    .ok_or_else(|| {
                        invalid(node, "數值過大 / Value too large / 수치가 너무 큽니다")
                    })?;
                if node.white_divisor > 0 {
                    // Single final floor of pure / 20 + white / 4. Never floor
                    // each material separately: fractions may combine to a point.
                    let whites: i64 = children.iter().map(|child| child.white).sum();
                    let numerator = pure
                        .checked_mul(node.white_divisor)
                        .and_then(|v| {
                            whites
                                .checked_mul(node.divisor)
                                .and_then(|w| v.checked_add(w))
                        })
                        .ok_or_else(|| {
                            invalid(node, "數值過大 / Value too large / 수치가 너무 큽니다")
                        })?;
                    numerator / (node.divisor * node.white_divisor)
                } else {
                    pure / node.divisor
                }
            }
        };
        let total_stats = node
            .base_stats
            .checked_add(level_points)
            .and_then(|v| v.checked_add(bonus))
            .ok_or_else(|| invalid(node, "數值過大 / Value too large / 수치가 너무 큽니다"))?;
        let line = BonusLine {
            key: node.key.clone(),
            level,
            base_stats: node.base_stats,
            level_points,
            bonus,
            white,
            total_stats,
        };
        lines.push(line.clone());
        Ok(line)
    }
    let mut lines = Vec::new();
    let line = walk(
        tree,
        inputs,
        0,
        &mut BTreeSet::new(),
        &mut lines,
        true,
        false,
        0,
    )?;
    Ok(BonusResult {
        bonus: line.bonus,
        lines,
    })
}

impl Catalog {
    pub fn bonus_tree(&self, code: &str, lang: &str) -> Result<BonusNode, String> {
        let mut count = 0;
        self.bonus_node(
            code,
            code.to_string(),
            lang,
            false,
            &mut Vec::new(),
            &mut count,
        )
    }

    fn bonus_node(
        &self,
        code: &str,
        key: String,
        lang: &str,
        white_enabled: bool,
        path: &mut Vec<String>,
        count: &mut usize,
    ) -> Result<BonusNode, String> {
        *count += 1;
        if path.len() >= MAX_DEPTH || *count > MAX_NODES || path.iter().any(|part| part == code) {
            return Err(bonus_text(
                lang,
                "角色轉職鏈有循環或過長",
                "Character chain contains a cycle or exceeds the limit",
                "캐릭터 경로에 순환이 있거나 너무 깁니다",
            )
            .into());
        }
        let hero = self
            .index
            .heroes
            .iter()
            .find(|hero| hero.code == code)
            .ok_or_else(|| format!("Unknown character: {code}"))?;
        if hero.rank >= 13 {
            return Err(bonus_text(
                lang,
                "此類兵種不適用合成紅利",
                "This class has no formation bonus",
                "이 유형에는 합성 보너스가 없습니다",
            )
            .into());
        }
        let stats: (Option<i64>, Option<i64>, Option<i64>, Option<i64>) = self.conn.query_row(
            "SELECT strength,dexterity,vitality,intelligence FROM character_stats WHERE code=?1", [code],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .map_err(|_| format!("{}: {}", hero_names(lang, hero).0, bonus_text(lang,
                "缺少基礎屬性", "Base attributes unavailable", "기본 능력치 없음")))?;
        let base_stats = [stats.0, stats.1, stats.2, stats.3]
            .into_iter()
            .try_fold(0_i64, |sum, stat| {
                stat.filter(|v| *v >= 0).and_then(|v| sum.checked_add(v))
            })
            .ok_or_else(|| format!("Invalid base attributes: {code}"))?;
        let mut node = BonusNode {
            key,
            code: code.into(),
            title: hero_names(lang, hero).0,
            base_stats,
            rank: hero.rank,
            nation: hero.nation,
            children: Vec::new(),
            predecessor_options: Vec::new(),
            divisor: 0,
            white_divisor: 0,
            rule: bonus_text(
                lang,
                "起始兵種；未有轉職紅利",
                "Starting unit; no formation bonus",
                "시작 용병 · 전직 보너스 없음",
            )
            .into(),
            source: GROWTH_SOURCE.into(),
            white_enabled,
            manual_required: false,
            warning: String::new(),
            uses_levels: false,
            min_level: 1,
            max_level: if matches!(hero.rank, 9 | 11 | 12) {
                260
            } else {
                250
            },
            starting_bonus: if code == "F40" { 500 } else { 0 },
        };
        let mut parents: Vec<String> = Vec::new();
        if let Some(combos) = self.index.combos.get(code) {
            let mut matches: Vec<_> = combos.iter().filter(|combo| combo.result == code).collect();
            matches.sort_by(|a, b| a.parts.cmp(&b.parts));
            matches.dedup_by(|a, b| a.parts == b.parts);
            if matches.len() > 1 {
                return Err(format!("Ambiguous bonus routes: {code}"));
            }
            if let Some(combo) = matches.first() {
                parents = combo.parts.clone();
            }
        }
        if parents.is_empty() {
            if let Some(chain) = self.index.chains.get(code) {
                if let Some(index) = chain
                    .iter()
                    .position(|step| step.code == code)
                    .filter(|index| *index > 0)
                {
                    parents.push(chain[index - 1].code.clone());
                }
            }
        }
        // These source-defined recipes exist in the client recruitment text,
        // but the catalog's job_steps import currently omits their edges.
        // Prefer structured catalog edges whenever a future import adds them.
        if parents.is_empty() {
            let fallback: &[&str] = match code {
                "T116" => &["E36", "E37", "E38"],
                "T117" => &["E42", "E43", "E44"],
                "T121" => &["E39", "E40", "E41"],
                "T122" => &["E33", "E34", "E35"],
                _ => &[],
            };
            parents.extend(fallback.iter().map(|code| (*code).to_string()));
        }
        path.push(code.into());
        for (index, parent) in parents.iter().enumerate() {
            let mut child = self.bonus_node(
                parent,
                format!("{}/{index}:{parent}", node.key),
                lang,
                hero.rank == 11,
                path,
                count,
            )?;
            if hero.rank == 11 {
                // QuestText.dat 00020544 applies to each legend material.
                child.min_level = 100;
            } else if let Some(minimum) = promotion_minimum(code, parent) {
                child.min_level = minimum;
            }
            node.children.push(child);
        }
        if parents.is_empty() && hero.rank == 0 && hero.nation == 10000 {
            for parent in indian_predecessors(code) {
                let mut option = self.bonus_node(
                    parent,
                    format!("{}/choice:{parent}", node.key),
                    lang,
                    false,
                    path,
                    count,
                )?;
                option.min_level = 100;
                node.predecessor_options.push(option);
            }
        }
        path.pop();
        if matches!(code, "E74" | "E75" | "E76" | "E77" | "E85") {
            let statues = if code == "E85" { 4 } else { 3 };
            for index in 0..statues {
                node.children.push(BonusNode {
                    key: format!("{}/statue:{index}", node.key),
                    code: format!("statue:{index}"),
                    title: format!(
                        "{} {}",
                        bonus_text(
                            lang,
                            if statues == 4 {
                                "神獸石像"
                            } else {
                                "幻獸石像"
                            },
                            if statues == 4 {
                                "Divine beast statue"
                            } else {
                                "Summon statue"
                            },
                            if statues == 4 {
                                "신수 석상"
                            } else {
                                "환수 석상"
                            }
                        ),
                        index + 1
                    ),
                    base_stats: 0,
                    rank: -1,
                    nation: 0,
                    children: Vec::new(),
                    predecessor_options: Vec::new(),
                    divisor: 0,
                    white_divisor: 0,
                    rule: bonus_text(
                        lang,
                        "填入石像記錄的等級",
                        "Enter the level recorded on the statue",
                        "석상에 기록된 레벨 입력",
                    )
                    .into(),
                    source: GENERAL_SOURCE.into(),
                    white_enabled: false,
                    manual_required: false,
                    warning: String::new(),
                    uses_levels: false,
                    min_level: 100,
                    max_level: 200,
                    starting_bonus: 0,
                });
            }
            node.divisor = statues;
            node.uses_levels = true;
            node.source = GENERAL_SOURCE.into();
        } else if !node.predecessor_options.is_empty() {
            node.divisor = 1;
            node.uses_levels = true;
            node.source = INDIA_CONVERSION_SOURCE.into();
        } else if node.children.is_empty() {
            if hero.rank == 0 && hero.nation == 10000 {
                node.manual_required = true;
                node.warning = bonus_text(
                    lang,
                    "請填入四國傭兵轉換時的等級紅利；印度兵使用白衣後填 0",
                    "Enter the former mercenary's level bonus; enter 0 if white-robe use reset it",
                    "이전 용병의 전환 레벨 보너스를 입력하세요. 백의로 초기화되었다면 0",
                )
                .into();
                node.rule = bonus_text(
                    lang,
                    "四國傭兵轉換等級的 100%",
                    "100% of the former mercenary's conversion level",
                    "이전 용병 전환 레벨의 100%",
                )
                .into();
                node.source = "https://www.52jushang.com/yongbing/india/sukgong.asp".into();
            } else if matches!(hero.rank, 5..=12) {
                node.manual_required = true;
                node.warning = bonus_text(
                    lang,
                    "缺少前置材料資料，請填入實際繼承紅利",
                    "Prerequisite data is unavailable; enter the actual inherited bonus",
                    "선행 재료 정보 없음: 실제 상속 보너스를 입력하세요",
                )
                .into();
                node.rule = node.warning.clone();
            }
        } else {
            let n = node.children.len();
            let parent_rank = node.children[0].rank;
            let (divisor, levels, source) = match (hero.rank, n) {
                (1 | 2 | 3 | 10, 1) => (5, false, GENERAL_SOURCE),
                (4, 1) if hero.nation == 10000 => (
                    1,
                    true,
                    "https://www.gersangjjang.com/yongbing/india/yukji.asp",
                ),
                (5, 3) if hero.nation == 10000 => (10, false, INDIA_SOURCE),
                (5, 3) => (3, true, GENERAL_SOURCE),
                (6, 3) => (30, false, "Client OnlineText.dat:00032396"),
                (6, 1) if parent_rank == 6 => (
                    10,
                    false,
                    "https://www.52jushang.com/yongbing/5monster2.asp",
                ),
                (7, 3) => (30, false, MONSTER_SOURCE),
                (7, 1) if parent_rank == 7 => (10, false, "Client QuestText.dat:00009795"),
                (8, 3) if code == "T79" => (30, false, "Client OnlineText.dat:00017183"),
                (9, 2) => (10, false, "Client QuestText.dat:00009489"),
                (9, 1) if parent_rank == 9 => (5, false, "Client QuestText.dat:00023343"),
                (11, 4) => (20, false, "Client QuestText.dat:00020544"),
                (12, 2) => (10, false, "https://www.52jushang.com/yongbing/mingwang.asp"),
                (12, 1) if parent_rank == 12 => (5, false, "Client QuestText.dat:00025408"),
                _ => (0, false, ""),
            };
            node.divisor = divisor;
            node.uses_levels = levels;
            node.source = source.into();
            if hero.rank == 11 {
                node.white_divisor = 4;
            }
            if divisor == 0 {
                node.manual_required = true;
                node.warning = bonus_text(
                    lang,
                    "此路線尚無已確認公式，請填入實際繼承紅利",
                    "This route has no verified formula; enter the actual inherited bonus",
                    "이 경로의 공식 미확인: 실제 상속 보너스를 입력하세요",
                )
                .into();
            }
        }
        if node.divisor > 0 {
            node.rule = if !node.predecessor_options.is_empty() {
                bonus_text(
                    lang,
                    "四選一；繼承所選四國傭兵轉換等級的 100%",
                    "Choose one of four; inherit 100% of its conversion level",
                    "4종 중 하나 선택 · 선택한 용병의 전환 레벨 100% 상속",
                )
                .into()
            } else if node.white_divisor > 0 {
                bonus_text(
                    lang,
                    "⌊材料純屬性總和 ÷ 20 ＋ 白衣總和 ÷ 4⌋",
                    "⌊Sum of pure material stats ÷ 20 + white-robe points ÷ 4⌋",
                    "⌊재료 순수 능력치 합 ÷ 20 + 백의 합 ÷ 4⌋",
                )
                .into()
            } else if node.uses_levels {
                format!(
                    "{} ÷ {}⌋",
                    bonus_text(
                        lang,
                        "⌊材料等級總和",
                        "⌊Sum of material levels",
                        "⌊재료 레벨 합"
                    ),
                    node.divisor
                )
            } else {
                format!(
                    "{} ÷ {}⌋",
                    bonus_text(
                        lang,
                        "⌊材料純屬性總和",
                        "⌊Sum of pure material stats",
                        "⌊재료 순수 능력치 합"
                    ),
                    node.divisor
                )
            };
        }
        if node.white_enabled && node.nation == 10000 {
            node.warning = bonus_text(lang,
                "印度白衣會影響早期轉職；確認白衣在四國兵時完成，或填實際非白衣紅利",
                "White-robe history affects Indian promotions; confirm training before India or enter actual non-white bonus",
                "인도 전직은 백의 이력에 영향을 받습니다. 인도 전환 전 완료를 확인하거나 실제 비백의 보너스를 입력하세요").into();
        }
        if code == "F40" {
            node.rule = bonus_text(
                lang,
                "招募固定紅利 500 點",
                "Fixed recruitment bonus: 500 points",
                "고용 기본 보너스: 500포인트",
            )
            .into();
            node.source = "https://www.52jushang.com/yongbing/guaiwu_zhengshu.asp".into();
        }
        Ok(node)
    }

    pub fn bonus_candidates(
        &self,
        query: &str,
        group: &str,
        lang: &str,
    ) -> Vec<CharacterCalcCandidate> {
        let folded = query.trim().to_lowercase();
        self.index.prepare_aliases(&folded);
        let rank = if group.is_empty() {
            None
        } else {
            let Ok(rank) = group.parse::<i64>() else {
                return Vec::new();
            };
            Some(rank)
        };
        let mut result: Vec<_> = self
            .index
            .heroes
            .iter()
            .filter(|hero| hero.rank < 13 && rank.is_none_or(|rank| rank == hero.rank))
            .filter(|hero| {
                folded.is_empty()
                    || self
                        .index
                        .name_hit(query.trim(), &folded, &hero.code, &hero.names)
            })
            .filter_map(|hero| {
                let tree = self.bonus_tree(&hero.code, lang).ok()?;
                if (tree.children.is_empty() && tree.predecessor_options.is_empty())
                    || tree.manual_required
                {
                    return None;
                }
                Some(CharacterCalcCandidate {
                    code: hero.code.clone(),
                    title: hero_names(lang, hero).0,
                    subtitle: format!(
                        "{} · {}",
                        hero_rank_label(&hero.rank.to_string(), lang),
                        hero.code
                    ),
                    route_count: 1,
                    icon: hero.icon.clone(),
                })
            })
            .collect();
        result.sort_by_key(|row| {
            let hero = self
                .index
                .heroes
                .iter()
                .find(|hero| hero.code == row.code)
                .unwrap();
            (hero_rank_order(hero.rank), hero.code.clone())
        });
        result
    }

    pub fn bonus_groups(&self, lang: &str) -> Vec<CharacterCalcGroup> {
        let mut counts = BTreeMap::<i64, usize>::new();
        for candidate in self.bonus_candidates("", "", lang) {
            if let Some(hero) = self
                .index
                .heroes
                .iter()
                .find(|hero| hero.code == candidate.code)
            {
                *counts.entry(hero.rank).or_default() += 1;
            }
        }
        let mut ranks: Vec<_> = counts.keys().copied().collect();
        ranks.sort_by_key(|rank| hero_rank_order(*rank));
        let mut groups = vec![CharacterCalcGroup {
            id: String::new(),
            title: bonus_text(lang, "全部", "All", "전체").into(),
            count: counts.values().sum(),
        }];
        groups.extend(ranks.into_iter().map(|rank| CharacterCalcGroup {
            id: rank.to_string(),
            title: hero_rank_label(&rank.to_string(), lang),
            count: counts[&rank],
        }));
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill_required(node: &BonusNode, inputs: &mut BTreeMap<String, BonusInput>) {
        if node.manual_required {
            inputs.entry(node.key.clone()).or_default().inherited = "100".into();
        }
        for child in &node.children {
            fill_required(child, inputs);
        }
    }

    #[test]
    fn cumulative_growth_matches_verified_milestones_and_rejects_unknown_levels() {
        for (level, expected) in [
            (1, 0),
            (100, 396),
            (120, 477),
            (130, 528),
            (150, 682),
            (200, 1306),
            (212, 1507),
            (245, 2156),
            (250, 2256),
            (260, 2466),
        ] {
            assert_eq!(bonus_level_points(level).unwrap(), expected, "{level}");
        }
        assert!(bonus_level_points(0).is_err());
        assert!(bonus_level_points(261).is_err());
    }

    #[test]
    fn all_fifteen_legends_have_four_complete_independent_paths() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let legends = catalog.bonus_candidates("", "11", "zh");
        assert_eq!(legends.len(), 15);
        for hero in legends {
            let tree = catalog.bonus_tree(&hero.code, "zh").unwrap();
            assert_eq!(tree.children.len(), 4);
            assert_eq!((tree.divisor, tree.white_divisor), (20, 4));
            let mut inputs = BTreeMap::new();
            fill_required(&tree, &mut inputs);
            let result = calculate_bonus(&tree, &inputs).unwrap();
            assert!(result.bonus > 0);
            let keys: BTreeSet<_> = result.lines.iter().map(|line| &line.key).collect();
            assert_eq!(keys.len(), result.lines.len());
            assert!(tree.children.iter().all(|child| child.white_enabled));
        }
    }

    #[test]
    fn stages_floor_individually_but_legend_material_fractions_are_combined() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let tree = catalog.bonus_tree("T80", "zh").unwrap();
        let mut inputs = BTreeMap::new();
        for (index, child) in tree.children.iter().enumerate() {
            inputs.insert(
                child.key.clone(),
                BonusInput {
                    level: child.min_level.to_string(),
                    inherited: (index + 1).to_string(),
                    white: "1".into(),
                    ..BonusInput::default()
                },
            );
        }
        let result = calculate_bonus(&tree, &inputs).unwrap();
        let pure: i64 = tree
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                child.base_stats + bonus_level_points(child.min_level).unwrap() + index as i64 + 1
            })
            .sum();
        assert_eq!(result.bonus, (pure + 20) / 20);
        assert_eq!(
            result.lines.len(),
            5,
            "manual incoming values replace earlier history"
        );
    }

    #[test]
    fn published_level_250_legend_examples_match_with_and_without_white_robe() {
        // Independent fixtures: https://gilboung.tistory.com/90 and /6.
        let catalog = Catalog::open_for_mcp().unwrap();
        for (code, plain, white) in [("T80", 719, 1019), ("T81", 723, 1023)] {
            let tree = catalog.bonus_tree(code, "en").unwrap();
            assert_eq!(
                calculate_bonus(&tree, &BTreeMap::new()).unwrap().bonus,
                plain
            );
            let mut inputs = BTreeMap::new();
            for child in &tree.children {
                inputs.insert(
                    child.key.clone(),
                    BonusInput {
                        white: "300".into(),
                        ..child.default_input()
                    },
                );
            }
            assert_eq!(calculate_bonus(&tree, &inputs).unwrap().bonus, white);
        }
    }

    #[test]
    fn statue_limits_and_fixed_hire_bonus_are_not_treated_as_ordinary_levels() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let beast = catalog.bonus_tree("E85", "zh").unwrap();
        assert_eq!(
            calculate_bonus(&beast, &BTreeMap::new()).unwrap().bonus,
            200
        );
        assert!(beast
            .children
            .iter()
            .all(|statue| statue.max_level == 200 && statue.default_input().level == "200"));
        let mut inputs = BTreeMap::new();
        inputs.insert(beast.children[0].key.clone(), BonusInput::default());
        assert!(calculate_bonus(&beast, &inputs).is_err());
        let bear = catalog.bonus_tree("F40", "zh").unwrap();
        assert_eq!(calculate_bonus(&bear, &BTreeMap::new()).unwrap().bonus, 500);
    }

    #[test]
    fn level_only_recipes_ignore_unused_stat_history_and_manual_override_ignores_old_drafts() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let tree = catalog.bonus_tree("E33", "en").unwrap();
        let mut inputs = BTreeMap::new();
        for child in &tree.children {
            inputs.insert(
                child.key.clone(),
                BonusInput {
                    inherited: "invalid but unused".into(),
                    ..child.default_input()
                },
            );
        }
        assert_eq!(calculate_bonus(&tree, &inputs).unwrap().bonus, 250);
        let legend = catalog.bonus_tree("T112", "en").unwrap();
        for child in &legend.children {
            inputs.insert(
                child.key.clone(),
                BonusInput {
                    inherited: "700".into(),
                    ..child.default_input()
                },
            );
            inputs.insert(
                child.children[0].key.clone(),
                BonusInput {
                    level: "bad old draft".into(),
                    ..BonusInput::default()
                },
            );
        }
        calculate_bonus(&legend, &inputs).unwrap();
    }

    #[test]
    fn repeated_monster_materials_keep_separate_inputs() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let tree = catalog.bonus_tree("D58", "zh").unwrap();
        let mut inputs = BTreeMap::new();
        let first = &tree.children[0].children[0];
        let repeated = &tree.children[1].children[0];
        assert_eq!(first.code, repeated.code);
        assert_ne!(first.key, repeated.key);
        inputs
            .entry(first.key.clone())
            .or_insert_with(BonusInput::default)
            .level = "100".into();
        inputs
            .entry(repeated.key.clone())
            .or_insert_with(BonusInput::default)
            .level = "200".into();
        let result = calculate_bonus(&tree, &inputs).unwrap();
        assert_eq!(
            result
                .lines
                .iter()
                .find(|row| row.key == first.key)
                .unwrap()
                .level,
            100
        );
        assert_eq!(
            result
                .lines
                .iter()
                .find(|row| row.key == repeated.key)
                .unwrap()
                .level,
            200
        );
    }

    #[test]
    fn source_rules_distinguish_level_averages_and_attribute_divisors() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for (code, divisor, levels) in [
            ("T33", 5, false),
            ("E33", 3, true),
            ("D58", 30, false),
            ("T62", 10, false),
            ("D78", 1, true),
            ("F31", 10, false),
            ("T79", 30, false),
            ("E74", 3, true),
            ("E85", 4, true),
            ("T49", 10, false),
            ("V06", 5, false),
            ("T86", 10, false),
            ("V29", 5, false),
            ("T116", 30, false),
            ("T117", 30, false),
            ("T121", 30, false),
            ("T122", 30, false),
        ] {
            let tree = catalog.bonus_tree(code, "en").unwrap();
            assert_eq!(
                (tree.divisor, tree.uses_levels),
                (divisor, levels),
                "{code}"
            );
            let mut inputs = BTreeMap::new();
            fill_required(&tree, &mut inputs);
            calculate_bonus(&tree, &inputs).unwrap();
        }
    }

    #[test]
    fn invalid_drafts_and_indian_white_robe_history_never_produce_plausible_results() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let tree = catalog.bonus_tree("T112", "en").unwrap();
        calculate_bonus(&tree, &BTreeMap::new()).unwrap();
        let mut inputs = BTreeMap::new();
        fill_required(&tree, &mut inputs);
        calculate_bonus(&tree, &inputs).unwrap();
        let child = &tree.children[0];
        inputs.entry(child.key.clone()).or_default().level = "250.5".into();
        assert!(calculate_bonus(&tree, &inputs).is_err());
        inputs.get_mut(&child.key).unwrap().level = "250".into();
        inputs.get_mut(&child.key).unwrap().white = "300".into();
        assert!(calculate_bonus(&tree, &inputs).is_err());
        inputs.get_mut(&child.key).unwrap().inherited = "600".into();
        calculate_bonus(&tree, &inputs).unwrap();
        inputs.get_mut(&child.key).unwrap().white = "301".into();
        assert!(calculate_bonus(&tree, &inputs).is_err());
        inputs.get_mut(&child.key).unwrap().white = "300".into();
        inputs.get_mut(&child.key).unwrap().inherited = "-1".into();
        assert!(calculate_bonus(&tree, &inputs).is_err());
    }

    #[test]
    fn all_nine_indian_mercenaries_offer_four_alternative_conversion_routes() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let candidates = catalog.bonus_candidates("", "0", "zh");
        assert_eq!(candidates.len(), 9);
        for candidate in candidates {
            let tree = catalog.bonus_tree(&candidate.code, "zh").unwrap();
            assert!(
                tree.children.is_empty(),
                "OR options must not become AND ingredients"
            );
            assert_eq!(tree.predecessor_options.len(), 4, "{}", tree.code);
            assert!(!tree.manual_required);
            assert_eq!((tree.divisor, tree.uses_levels), (1, true));
            let result = calculate_bonus(&tree, &BTreeMap::new()).unwrap();
            assert_eq!(result.bonus, 250);
            assert_eq!(
                result.lines.len(),
                2,
                "Only one ordinary source is consumed"
            );
            for option in &tree.predecessor_options {
                assert_eq!(
                    (option.rank, option.min_level, option.max_level),
                    (0, 100, 250)
                );
                assert_ne!(option.nation, 10000);
                assert!(option.key.ends_with(&format!("choice:{}", option.code)));
            }
        }
        let longsword = catalog.bonus_tree("D117", "zh").unwrap();
        assert_eq!(
            longsword
                .predecessor_options
                .iter()
                .map(|option| (option.code.as_str(), option.title.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("U86", "刀手"),
                ("U09", "劍術浪人"),
                ("U65", "大刀武士"),
                ("U55", "斧頭巨漢")
            ]
        );
    }

    #[test]
    fn indian_choices_use_only_selected_level_and_keep_branch_drafts_independent() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let tree = catalog.bonus_tree("D117", "en").unwrap();
        let mut inputs = BTreeMap::new();
        for (option, level) in tree.predecessor_options.iter().zip([100, 150, 200, 250]) {
            inputs.insert(
                option.key.clone(),
                BonusInput {
                    level: level.to_string(),
                    inherited: "unused stat history".into(),
                    ..option.default_input()
                },
            );
        }
        for (option, level) in tree.predecessor_options.iter().zip([100, 150, 200, 250]) {
            inputs.entry(tree.key.clone()).or_default().predecessor = option.code.clone();
            let result = calculate_bonus(&tree, &inputs).unwrap();
            assert_eq!(result.bonus, level);
            assert_eq!(result.lines.len(), 2);
            assert_eq!(result.lines[0].key, option.key);
            assert_eq!(tree.effective_children(&inputs)[0].key, option.key);
        }
        // Switching back uses this branch's own level, not the last option's.
        inputs.get_mut(&tree.key).unwrap().predecessor.clear();
        assert_eq!(calculate_bonus(&tree, &inputs).unwrap().bonus, 100);
        inputs
            .get_mut(&tree.predecessor_options[3].key)
            .unwrap()
            .level = "bad hidden draft".into();
        assert_eq!(calculate_bonus(&tree, &inputs).unwrap().bonus, 100);
        inputs
            .get_mut(&tree.predecessor_options[0].key)
            .unwrap()
            .level = "99".into();
        assert!(calculate_bonus(&tree, &inputs)
            .unwrap_err()
            .contains("100–250"));
        inputs
            .get_mut(&tree.predecessor_options[0].key)
            .unwrap()
            .level = "100".into();
        inputs.get_mut(&tree.key).unwrap().predecessor = "not-a-route".into();
        assert!(calculate_bonus(&tree, &inputs)
            .unwrap_err()
            .contains("Choose a valid conversion source"));
    }

    #[test]
    fn indian_conversion_bonus_flows_into_promotions_and_manual_history_can_replace_it() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let tree = catalog.bonus_tree("D118", "en").unwrap();
        let indian = &tree.children[0];
        let source = &indian.predecessor_options[0];
        let mut inputs = BTreeMap::new();
        inputs.insert(
            source.key.clone(),
            BonusInput {
                level: "100".into(),
                ..source.default_input()
            },
        );
        let expected = (indian.base_stats + bonus_level_points(250).unwrap() + 100) / 5;
        assert_eq!(calculate_bonus(&tree, &inputs).unwrap().bonus, expected);
        inputs.get_mut(&source.key).unwrap().level = "250".into();
        let higher = calculate_bonus(&tree, &inputs).unwrap().bonus;
        assert_eq!(higher - expected, 30);
        inputs.insert(
            indian.key.clone(),
            BonusInput {
                inherited: "0".into(), // White-robe reset discards the conversion bonus.
                predecessor: "stale replaced history".into(),
                ..indian.default_input()
            },
        );
        let result = calculate_bonus(&tree, &inputs).unwrap();
        assert_eq!(
            result.bonus,
            (indian.base_stats + bonus_level_points(250).unwrap()) / 5
        );
        assert_eq!(result.lines.len(), 2);
    }

    #[test]
    fn no_rebirth_maxima_match_independent_wisdom_king_and_legend_examples() {
        // Bahamut snA=12582 (2025 update) gives the awakened results. Its images
        // show deliberately compressed predecessors, not unawakened maxima.
        let catalog = Catalog::open_for_mcp().unwrap();
        for (code, expected) in [
            ("T86", 666),
            ("T87", 665),
            ("T88", 660),
            ("T89", 677),
            ("T113", 726),
            ("V29", 976),
            ("V28", 956),
            ("V27", 955),
            ("V26", 968),
            ("T80", 1019),
            ("T81", 1023),
            ("T92", 1013),
            ("V16", 1080),
        ] {
            let tree = catalog.bonus_tree(code, "en").unwrap();
            let maximum = calculate_max_bonus(&tree, &BTreeMap::new()).unwrap();
            assert_eq!(maximum.bonus, expected, "{code}");
            assert!(maximum.supports_perfect(), "{code}");
        }
        // The database has no awakened Budong entry. Verify the published
        // arithmetic without inventing an entity or a selectable target.
        let budong = catalog.bonus_tree("T113", "en").unwrap();
        assert_eq!(
            (budong.base_stats + bonus_level_points(260).unwrap() + 726) / 5,
            988
        );
    }

    #[test]
    fn compressed_wisdom_king_routes_still_reach_the_maximum_after_each_floor() {
        // Exact level combinations transcribed from the five snA=12582 images.
        let catalog = Catalog::open_for_mcp().unwrap();
        for (code, ordinary_level, combined_level, fiend, fiend_level, preceding_bonus, expected) in [
            ("V29", 104, 196, "D58", 245, 664, 976),
            ("V28", 176, 197, "D57", 247, 664, 956),
            ("V27", 174, 193, "D55", 250, 659, 955),
            ("V26", 141, 188, "D56", 240, 674, 968),
        ] {
            let tree = catalog.bonus_tree(code, "en").unwrap();
            let mut inputs = BTreeMap::new();
            fn levels(
                node: &BonusNode,
                inputs: &mut BTreeMap<String, BonusInput>,
                ordinary: i64,
                combined: i64,
                fiend: &str,
                fiend_level: i64,
            ) {
                let level = if node.code == fiend {
                    fiend_level
                } else if node.rank == 4 && node.nation != 0 {
                    ordinary
                } else if node.rank == 5 {
                    combined
                } else {
                    node.max_level
                };
                inputs.insert(
                    node.key.clone(),
                    BonusInput {
                        level: level.to_string(),
                        ..node.default_input()
                    },
                );
                for child in &node.children {
                    levels(child, inputs, ordinary, combined, fiend, fiend_level);
                }
            }
            levels(
                &tree,
                &mut inputs,
                ordinary_level,
                combined_level,
                fiend,
                fiend_level,
            );
            let result = calculate_bonus(&tree, &inputs).unwrap();
            assert_eq!(result.bonus, expected, "{code}");
            assert_eq!(
                result
                    .lines
                    .iter()
                    .find(|line| line.key == tree.children[0].key)
                    .unwrap()
                    .bonus,
                preceding_bonus,
                "{code}"
            );
            assert_eq!(
                result.bonus,
                calculate_max_bonus(&tree, &inputs).unwrap().bonus,
                "Lower levels can still be perfect: {code}"
            );
        }
    }

    #[test]
    fn compressed_four_country_legend_is_perfect_without_every_material_at_its_cap() {
        // Bahamut snA=12729, Meng Huo screenshot: U03=237, U96=234,
        // U90=234; the remaining stages are 250 and final materials have W300.
        let catalog = Catalog::open_for_mcp().unwrap();
        let tree = catalog.bonus_tree("T92", "zh").unwrap();
        let mut inputs = BTreeMap::new();
        fn levels(node: &BonusNode, inputs: &mut BTreeMap<String, BonusInput>) {
            let level = match node.code.as_str() {
                "U03" => 237,
                "U96" | "U90" => 234,
                _ => 250,
            };
            inputs.insert(
                node.key.clone(),
                BonusInput {
                    level: level.to_string(),
                    white: if node.white_enabled { "300" } else { "0" }.into(),
                    ..node.default_input()
                },
            );
            for child in &node.children {
                levels(child, inputs);
            }
        }
        levels(&tree, &mut inputs);
        let result = calculate_bonus(&tree, &inputs).unwrap();
        assert_eq!(result.bonus, 1013);
        assert_eq!(
            result.bonus,
            calculate_max_bonus(&tree, &inputs).unwrap().bonus
        );
    }

    #[test]
    fn indian_white_before_conversion_matches_razia_image_and_differs_from_reset_history() {
        // Bahamut snA=12729, Razia image 582d6a9d65d88d03e6f78c11611624a6.JPG.
        // All white training is completed BEFORE the four-country conversion.
        let catalog = Catalog::open_for_mcp().unwrap();
        let tree = catalog.bonus_tree("V16", "en").unwrap();
        let mut inputs = BTreeMap::new();
        fn fill(node: &BonusNode, inputs: &mut BTreeMap<String, BonusInput>) {
            inputs.insert(
                node.key.clone(),
                BonusInput {
                    white: if node.white_enabled { "300" } else { "0" }.into(),
                    white_before_india: node.white_enabled && node.nation == 10000,
                    ..node.default_input()
                },
            );
            for child in node.effective_children(&BTreeMap::new()) {
                fill(child, inputs);
                if !node.predecessor_options.is_empty() {
                    let level = match node.code.as_str() {
                        "D115" | "F7" => "174",
                        "F13" => "119",
                        "F11" => "219",
                        other => panic!("Unexpected Razia source: {other}"),
                    };
                    inputs.get_mut(&child.key).unwrap().level = level.into();
                }
            }
        }
        fill(&tree, &mut inputs);
        let result = calculate_bonus(&tree, &inputs).unwrap();
        assert_eq!(result.bonus, 1080);
        for (code, expected) in [("T61", 716), ("T82", 721), ("T76", 722), ("T98", 717)] {
            let node = tree.children.iter().find(|node| node.code == code).unwrap();
            assert_eq!(
                result
                    .lines
                    .iter()
                    .find(|line| line.key == node.key)
                    .unwrap()
                    .bonus,
                expected,
                "{code}"
            );
        }
        let maximum = calculate_max_bonus(&tree, &inputs).unwrap();
        assert_eq!(maximum.bonus, 1080);
        assert!(maximum.supports_perfect());
        let first = &tree.children[0];
        inputs.get_mut(&first.key).unwrap().white_before_india = false;
        assert!(
            calculate_bonus(&tree, &inputs).is_err(),
            "The history must be explicitly confirmed"
        );
        inputs.get_mut(&first.key).unwrap().white_before_india = true;
        // Zero overrides replace only the original conversion bonus, leaving
        // this fixture's W300 in the first two Indian promotion formulas.
        fn reset_conversion(node: &BonusNode, inputs: &mut BTreeMap<String, BonusInput>) {
            if !node.predecessor_options.is_empty() {
                inputs.get_mut(&node.key).unwrap().inherited = "0".into();
            }
            for child in &node.children {
                reset_conversion(child, inputs);
            }
        }
        reset_conversion(&tree, &mut inputs);
        assert_eq!(calculate_bonus(&tree, &inputs).unwrap().bonus, 1079);
    }

    #[test]
    fn maximum_uses_own_caps_and_selected_routes_without_trusting_manual_history() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let tree = catalog.bonus_tree("D117", "en").unwrap();
        let mut inputs = BTreeMap::new();
        inputs.insert(
            tree.key.clone(),
            BonusInput {
                predecessor: "U55".into(),
                ..tree.default_input()
            },
        );
        for option in &tree.predecessor_options {
            inputs.insert(
                option.key.clone(),
                BonusInput {
                    level: "incomplete".into(),
                    inherited: "999999".into(),
                    ..option.default_input()
                },
            );
        }
        let maximum = calculate_max_bonus(&tree, &inputs).unwrap();
        assert_eq!(maximum.bonus, 250);
        assert!(!maximum.supports_perfect());
        inputs.get_mut(&tree.key).unwrap().predecessor = "invalid".into();
        assert!(calculate_max_bonus(&tree, &inputs).is_err());
        let statue = catalog.bonus_tree("E85", "en").unwrap();
        assert_eq!(
            calculate_max_bonus(&statue, &BTreeMap::new())
                .unwrap()
                .bonus,
            200
        );
        let ordinary = catalog.bonus_tree("D118", "en").unwrap();
        assert_eq!(
            calculate_max_bonus(&ordinary, &BTreeMap::new())
                .unwrap()
                .scope,
            BonusMaximumScope::NoWhiteRobeOrRebirth
        );
        let mut unknown = catalog.bonus_tree("T80", "en").unwrap();
        unknown.children[0].manual_required = true;
        inputs.insert(
            unknown.children[0].key.clone(),
            BonusInput {
                inherited: "0".into(),
                ..unknown.children[0].default_input()
            },
        );
        assert!(
            calculate_max_bonus(&unknown, &inputs).is_err(),
            "Unknown history must not become an asserted maximum of zero"
        );
    }

    #[test]
    fn client_promotion_requirements_reject_impossible_material_levels() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let sword_general = catalog.bonus_tree("U87", "en").unwrap();
        let source = &sword_general.children[0];
        assert_eq!(source.code, "U86");
        let mut inputs = BTreeMap::new();
        inputs.insert(
            source.key.clone(),
            BonusInput {
                level: "49".into(),
                ..source.default_input()
            },
        );
        assert!(calculate_bonus(&sword_general, &inputs).is_err());
        inputs.get_mut(&source.key).unwrap().level = "50".into();
        assert!(calculate_bonus(&sword_general, &inputs).is_ok());

        let indian_second = catalog.bonus_tree("T44", "en").unwrap();
        assert_eq!(
            indian_second.children[0].min_level, 80,
            "India's training table and MI_GetJob agree on 80, despite the generic table's 100"
        );
        let awake = catalog.bonus_tree("V27", "en").unwrap();
        assert_eq!(awake.children[0].min_level, 260);
        assert_eq!(awake.children[0].default_input().level, "260");
        inputs.clear();
        inputs.insert(
            awake.children[0].key.clone(),
            BonusInput {
                level: "259".into(),
                ..awake.children[0].default_input()
            },
        );
        assert!(calculate_bonus(&awake, &inputs).is_err());
    }

    #[test]
    fn recorded_statue_levels_follow_client_ten_level_steps() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let divine = catalog.bonus_tree("E74", "en").unwrap();
        let statue = &divine.children[0];
        let mut inputs = BTreeMap::new();
        for level in [99, 105, 199, 201] {
            inputs.insert(
                statue.key.clone(),
                BonusInput {
                    level: level.to_string(),
                    ..statue.default_input()
                },
            );
            assert!(
                calculate_bonus(&divine, &inputs).is_err(),
                "invalid statue {level}"
            );
        }
        for level in [100, 150, 200] {
            inputs.insert(
                statue.key.clone(),
                BonusInput {
                    level: level.to_string(),
                    ..statue.default_input()
                },
            );
            assert!(
                calculate_bonus(&divine, &inputs).is_ok(),
                "valid statue {level}"
            );
        }
    }
}
