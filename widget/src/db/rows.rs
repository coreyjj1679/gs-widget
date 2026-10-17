use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub(in crate::db) struct Index {
    pub(in crate::db) monsters: Vec<MonsterRow>,
    pub(in crate::db) nations: Vec<(i64, i64)>,
    pub(in crate::db) places: Vec<PlaceRow>,
    pub(in crate::db) items: Vec<ItemRow>,
    pub(in crate::db) item_icons: HashMap<i64, (PathBuf, i32)>,
    pub(in crate::db) recipes: Vec<RecipeRow>,
    pub(in crate::db) quests: Vec<QuestRow>,
    pub(in crate::db) npcs: Vec<NpcRow>,
    pub(in crate::db) heroes: Vec<HeroRow>,
    pub(in crate::db) chains: HashMap<String, Vec<ChainStep>>,
    pub(in crate::db) combos: HashMap<String, Vec<Combo>>,
    pub(in crate::db) roster_uses: HashMap<i64, Vec<RosterUse>>,
    pub(in crate::db) works: Vec<WorkBuilding>,
    pub(in crate::db) yards: HashMap<String, WorkFacility>,
    pub(in crate::db) formulas: HashMap<i64, ItemFormula>,
    pub(in crate::db) sets: Vec<SetRow>,
    pub(in crate::db) set_magic: HashMap<i64, SetMagic>,
    pub(in crate::db) towns: Vec<TownRow>,
    pub(in crate::db) charts: Vec<ChartRow>,
    pub(in crate::db) aliases: RefCell<HashMap<String, HashSet<String>>>,
    pub(in crate::db) active_alias: Cell<bool>,
    pub(in crate::db) active_codes: RefCell<HashSet<String>>,
}

pub(in crate::db) struct MonsterRow {
    pub(in crate::db) code: String,
    pub(in crate::db) nation: i64,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) place_en: String,
    pub(in crate::db) place_zh: String,
    pub(in crate::db) places: Vec<i64>,
    pub(in crate::db) names: String,
    pub(in crate::db) icon: Option<(PathBuf, i32)>,
    pub(in crate::db) badge: Option<PathBuf>,
}

pub(in crate::db) struct PlaceRow {
    pub(in crate::db) nation: i64,
    pub(in crate::db) gts: i64,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) count: i64,
    pub(in crate::db) names: String,
    pub(in crate::db) sort: String,
}

pub(in crate::db) struct ItemRow {
    pub(in crate::db) code_str: String,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) category: String,
    pub(in crate::db) subtype: String,
    pub(in crate::db) names: String,
}

pub(in crate::db) struct SetRow {
    pub(in crate::db) code: i64,
    pub(in crate::db) kind: i64,
    pub(in crate::db) pieces: i64,
    pub(in crate::db) ko: String,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) names: String,
    pub(in crate::db) icon: Option<(PathBuf, i32)>,
}

pub(in crate::db) struct SetMagic {
    pub(in crate::db) value1: i64,
    pub(in crate::db) value2: i64,
    pub(in crate::db) percent: bool,
    pub(in crate::db) note: String,
}

pub(in crate::db) struct RecipeRow {
    pub(in crate::db) id: i64,
    pub(in crate::db) source: String,
    pub(in crate::db) result_str: String,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) names: String,
    pub(in crate::db) level_min: Option<i64>,
    pub(in crate::db) icon: Option<(PathBuf, i32)>,
}

pub(in crate::db) struct QuestRow {
    pub(in crate::db) id: i64,
    pub(in crate::db) source: String,
    pub(in crate::db) code: String,
    pub(in crate::db) kind: String,
    pub(in crate::db) npc: i64,
    pub(in crate::db) title_en: String,
    pub(in crate::db) title_zh: String,
    pub(in crate::db) names: String,
}

#[derive(Clone)]
pub(in crate::db) struct ChainStep {
    pub(in crate::db) slot: i64,
    pub(in crate::db) code: String,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) materials: Vec<(i64, i64)>,
}

#[derive(Clone)]
pub(in crate::db) struct Combo {
    pub(in crate::db) result: String,
    pub(in crate::db) parts: Vec<String>,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) materials: Vec<(i64, i64)>,
}

#[derive(Clone)]
pub(in crate::db) struct RosterUse {
    pub(in crate::db) qty: i64,
    pub(in crate::db) target: String,
    pub(in crate::db) source: String,
    pub(in crate::db) combo: bool,
    pub(in crate::db) note: String,
}

pub(in crate::db) struct ChainBook {
    pub(in crate::db) lines: HashMap<String, Vec<ChainStep>>,
    pub(in crate::db) combos: HashMap<String, Vec<Combo>>,
    pub(in crate::db) uses: HashMap<i64, Vec<RosterUse>>,
}

#[derive(Clone)]
pub(in crate::db) struct HeroSkill {
    pub(in crate::db) name: String,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
}

pub(in crate::db) struct HeroRow {
    pub(in crate::db) code: String,
    pub(in crate::db) nation: i64,
    pub(in crate::db) rank: i64,
    pub(in crate::db) ko: String,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) nation_en: String,
    pub(in crate::db) nation_zh: String,
    pub(in crate::db) desc_en: String,
    pub(in crate::db) desc_zh: String,
    pub(in crate::db) hire_en: String,
    pub(in crate::db) hire_zh: String,
    pub(in crate::db) skills: Vec<HeroSkill>,
    pub(in crate::db) names: String,
    pub(in crate::db) icon: Option<(PathBuf, i32)>,
}

pub(in crate::db) struct NpcRow {
    pub(in crate::db) code: i64,
    pub(in crate::db) ko: String,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) nation_en: String,
    pub(in crate::db) nation_zh: String,
    pub(in crate::db) map_en: String,
    pub(in crate::db) map_zh: String,
    pub(in crate::db) map_index: i64,
    pub(in crate::db) x: i64,
    pub(in crate::db) y: i64,
    pub(in crate::db) place_en: String,
    pub(in crate::db) place_zh: String,
    pub(in crate::db) names: String,
    pub(in crate::db) icon: Option<(PathBuf, i32)>,
}

pub(in crate::db) struct QuestLine {
    pub(in crate::db) role: String,
    pub(in crate::db) slot: i64,
    pub(in crate::db) texts: HashMap<String, String>,
}

pub(in crate::db) struct QuestStep {
    pub(in crate::db) role: String,
    pub(in crate::db) kind: String,
    pub(in crate::db) code: String,
    pub(in crate::db) qty: String,
    pub(in crate::db) label: String,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(in crate::db) struct WorkUpgradeName {
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
}

pub(in crate::db) struct WorkProduct {
    pub(in crate::db) item: i64,
    pub(in crate::db) nations: String,
}

pub(in crate::db) struct FormulaPart {
    pub(in crate::db) kind: String,
    pub(in crate::db) item: i64,
    pub(in crate::db) qty: i64,
}

pub(in crate::db) struct ItemFormula {
    pub(in crate::db) country: i64,
    pub(in crate::db) labor: i64,
    pub(in crate::db) yield_n: i64,
    pub(in crate::db) yield_max: i64,
    pub(in crate::db) plant: String,
    pub(in crate::db) parts: Vec<FormulaPart>,
}

pub(in crate::db) struct WorkFacility {
    pub(in crate::db) code: String,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) names: String,
    #[allow(dead_code)]
    pub(in crate::db) upgrades: Vec<WorkUpgradeName>,
    pub(in crate::db) products: Vec<WorkProduct>,
    pub(in crate::db) icon: Option<(PathBuf, i32)>,
}

#[derive(Clone)]
pub(in crate::db) struct TownGood {
    pub(in crate::db) item: i64,
    pub(in crate::db) specialty: bool,
}

pub(in crate::db) struct TownRow {
    pub(in crate::db) code: String,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) region: String,
    pub(in crate::db) x: i64,
    pub(in crate::db) y: i64,
    pub(in crate::db) names: String,
    pub(in crate::db) goods: Vec<TownGood>,
}

pub(in crate::db) struct ChartRow {
    pub(in crate::db) code: i64,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
}

pub(in crate::db) struct WorkBuilding {
    pub(in crate::db) code: String,
    pub(in crate::db) en: String,
    pub(in crate::db) zh: String,
    pub(in crate::db) cost: i64,
    pub(in crate::db) credit: i64,
    pub(in crate::db) names: String,
    pub(in crate::db) facilities: Vec<String>,
    pub(in crate::db) icon: Option<(PathBuf, i32)>,
}

pub(in crate::db) struct MakePlace {
    pub(in crate::db) region: String,
    pub(in crate::db) building: String,
    pub(in crate::db) facility: String,
    pub(in crate::db) key: String,
    pub(in crate::db) icon: Option<(PathBuf, i32)>,
}
