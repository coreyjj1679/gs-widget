//! Browse changes between archived game catalogs offline.
mod artwork;
mod drops;
mod feed;
#[cfg(test)]
mod feed_smoke;
mod history;
mod names;

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;
#[cfg(test)]
use slint::Model;
use slint::{ComponentHandle, Image, ModelRc, SharedString, VecModel};

use crate::{db, App, GameUpdateField};
use artwork::Artwork;
use feed::{Feed, Layout, Published};
use history::{History, Report};

const REPORT: &str = include_str!("../../data/catalog-diff.json");
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
struct Name {
    en: String,
    zh: String,
    ko: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct Region {
    id: String,
    name: Name,
}

impl Name {
    fn display(&self, lang: &str) -> String {
        db::show3(lang, &self.en, &self.zh, &self.ko)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Added,
    Modified,
    Removed,
    Renamed,
}

impl Kind {
    fn labels(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Added => ("added", "新增", "Added"),
            Self::Modified => ("changed", "修改", "Changed"),
            Self::Removed => ("removed", "移除", "Removed"),
            Self::Renamed => ("renamed", "改名", "Renamed"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Entry {
    category: String,
    table: String,
    key: String,
    name: Name,
    #[serde(default)]
    previous_name: Option<Name>,
    #[serde(default)]
    drop_before: Option<drops::DropSnapshot>,
    #[serde(default)]
    drop_after: Option<drops::DropSnapshot>,
    #[serde(default)]
    loot: Vec<drops::LootSlot>,
    #[serde(default)]
    regions: Vec<Region>,
    #[serde(default)]
    previous_regions: Option<Vec<Region>>,
    kind: Kind,
    before: BTreeMap<String, Value>,
    after: BTreeMap<String, Value>,
}

fn table_label<'a>(table: &'a str, lang: &str) -> &'a str {
    let (zh, en, ko) = match table {
        "items" => ("道具資料", "Item details", "아이템 정보"),
        "item_sets" => ("套裝", "Equipment sets", "장비 세트"),
        "item_set_effects" => ("套裝效果", "Set effects", "세트 효과"),
        "item_set_members" => ("套裝部件", "Set pieces", "세트 구성품"),
        "item_set_magic" => ("套裝法術", "Set magic", "세트 마법"),
        "item_bases" => ("基礎道具", "Base items", "기본 아이템"),
        "item_gear" | "item_affix" => ("道具屬性", "Item stats", "아이템 능력치"),
        "enhance" => ("強化屬性", "Enhancement stats", "강화 능력치"),
        "monsters" => ("怪物資料", "Monster details", "몬스터 정보"),
        "monster_drops" => ("怪物掉落", "Monster drops", "몬스터 드롭"),
        "monster_locations" => ("怪物出沒地點", "Monster locations", "몬스터 출현 지역"),
        "monster_categories" => ("怪物分類", "Monster categories", "몬스터 분류"),
        "monster_sprites" => ("怪物圖像", "Monster sprites", "몬스터 이미지"),
        "monster_portraits" => ("怪物頭像", "Monster portraits", "몬스터 초상화"),
        "recipes" => ("配方資料", "Recipe details", "제작법 정보"),
        "recipe_materials" | "recipe_material_choices" => {
            ("配方材料", "Recipe materials", "제작 재료")
        }
        "item_formulas" => ("生產配方", "Production formulas", "생산 공식"),
        "item_formula_parts" => ("生產配方材料", "Formula materials", "생산 공식 재료"),
        "quests" => ("任務資料", "Quest details", "퀘스트 정보"),
        "quest_steps" => ("任務步驟", "Quest steps", "퀘스트 단계"),
        "quest_lines" => ("任務對話", "Quest dialogue", "퀘스트 대화"),
        "maps" => ("地圖", "Maps", "지도"),
        "npcs" => ("NPC 資料", "NPC details", "NPC 정보"),
        "work_products" => ("生產品項", "Workshop products", "생산 품목"),
        "enchant_equipment" => ("鬼道裝備", "Enchant equipment", "주술 장비"),
        "enchant_effects" => ("鬼道效果", "Enchant effects", "주술 효과"),
        "enchant_recipes" => ("鬼道配方", "Enchant recipes", "주술 제작법"),
        "enchant_materials" => ("鬼道材料", "Enchant materials", "주술 재료"),
        "enchant_recipe_targets" => ("鬼道適用裝備", "Enchant targets", "주술 대상"),
        "strings" => ("遊戲文字", "Game text", "게임 텍스트"),
        _ => return table,
    };
    match db::primary_lang(lang) {
        "zh" => zh,
        "ko" => ko,
        _ => en,
    }
}

fn field_label<'a>(field: &'a str, lang: &str) -> &'a str {
    let (zh, en) = match field {
        "text" => ("文字", "Text"),
        "name_en" => ("英文名稱", "English name"),
        "name_zh" => ("中文名稱", "Chinese name"),
        "name_ko" => ("韓文名稱", "Korean name"),
        "name_gts" => ("名稱文字編號", "Name text ID"),
        "desc_gts" => ("說明文字編號", "Description text ID"),
        "large_image" => ("背包圖集", "Inventory icon sheet"),
        "large_frame" => ("背包圖格", "Inventory icon frame"),
        "small_image" => ("地面圖集", "Field icon sheet"),
        "small_frame" => ("地面圖格", "Field icon frame"),
        "qty" | "result_qty" => ("數量", "Quantity"),
        "price" => ("售價", "Vendor price"),
        "weight" => ("重量", "Weight"),
        "description" => ("說明", "Description"),
        "category" => ("分類", "Category"),
        "hp" => ("生命力", "HP"),
        "mp" => ("法力", "MP"),
        "exp" => ("經驗值", "Experience"),
        "element" => ("屬性", "Element"),
        "location_gts" => ("出沒地點", "Location"),
        "nation" => ("國籍", "Nation"),
        "fee" => ("製作費用", "Craft fee"),
        "kind" | "subtype" => ("類型", "Type"),
        "code" => ("代碼", "Code"),
        "item_code" => ("道具代碼", "Item code"),
        "slot" => ("欄位", "Slot"),
        "drop_rate_per_100000" => ("掉落率", "Drop rate"),
        "attack" | "attack_damage" => ("攻擊力", "Attack"),
        "atk_min" | "dmin" | "damage_min" => ("最低攻擊力", "Minimum attack"),
        "atk_max" | "dmax" | "damage_max" => ("最高攻擊力", "Maximum attack"),
        "attack_range" => ("攻擊距離", "Range"),
        "atk_speed" | "attack_speed" => ("攻擊速度", "Atk speed"),
        "hit_resist" => ("打擊抵抗", "Hit resist"),
        "magic_resist" => ("法術抵抗", "Magic resist"),
        "strength" => ("力量", "STR"),
        "dexterity" => ("敏捷", "DEX"),
        "vitality" => ("體質", "VIT"),
        "intelligence" => ("智力", "INT"),
        "hp_max" => ("生命力上限", "Max HP"),
        "mp_max" => ("法力上限", "Max MP"),
        "armor" => ("防禦力", "Armor"),
        "is_boss" => ("首領", "Boss"),
        "level" => ("等級", "Level"),
        "level_min" => ("最低等級", "Min level"),
        "level_max" => ("最高等級", "Max level"),
        "learn_fee" => ("學習費", "Learn fee"),
        "success_rate" => ("成功率", "Success"),
        "enhance" => ("強化等級", "Enhancement"),
        "pieces" => ("部件數", "Pieces"),
        "title" => ("名稱", "Name"),
        "detail" => ("說明", "Description"),
        "prev_code" => ("前置任務代碼", "Previous quest code"),
        "next_code" => ("後續任務代碼", "Next quest code"),
        "repeat_count" => ("重複次數", "Repeat count"),
        "npc_code" => ("NPC 代碼", "NPC code"),
        _ => return field,
    };
    db::ui_text_ref(lang, zh, en)
}

fn value_text(field: &str, value: Option<&Value>, lang: &str) -> String {
    if field == "drop_rate_per_100000" && value.is_some() {
        return db::drop_rate_text(value.and_then(Value::as_i64), lang);
    }
    match value {
        None => "—".to_owned(),
        Some(Value::String(s)) if s.is_empty() => "\"\"".to_owned(),
        Some(Value::String(s)) => s.clone(),
        Some(value) => value.to_string(),
    }
}

impl Entry {
    fn fields(&self, lang: &str) -> ModelRc<GameUpdateField> {
        let keys: BTreeSet<_> = self.before.keys().chain(self.after.keys()).collect();
        let mut keys: Vec<_> = keys.into_iter().collect();
        if self.kind == Kind::Renamed {
            keys.sort_by_key(|key| !self.name_field(key));
        }
        let mut fields = Vec::with_capacity(keys.len());
        for key in keys {
            let label = field_label(key, lang);
            let display = |value: Option<&Value>, before: bool| {
                if key == "location_gts" {
                    if let Some(place) =
                        value.and_then(|v| drops::location_value(self, v, lang, before))
                    {
                        return place;
                    }
                }
                value_text(key, value, lang)
            };
            let before = display(self.before.get(key), true);
            let after = display(self.after.get(key), false);
            fields.push(GameUpdateField {
                label: label.into(),
                before: before.into(),
                after: after.into(),
            });
        }
        ModelRc::new(VecModel::from(fields))
    }
}

pub(crate) fn detail_name_history(
    catalog: &db::Catalog,
    key: &str,
    lang: &str,
) -> ModelRc<crate::CatalogNameChange> {
    let Some(version) = catalog.game_version() else {
        return ModelRc::default();
    };
    let key = catalog.stable_history_key(key);
    let mut rows = Vec::new();
    for event in db::name_history::events(version, &key) {
        let versions: SharedString = format!("{} → {}", event.from, event.to).into();
        for change in event.changes {
            let display = |value: &str| -> SharedString {
                if value.is_empty() {
                    db::ui_text_ref(lang, "未提供譯名", "Translation unavailable").into()
                } else {
                    value.into()
                }
            };
            rows.push(crate::CatalogNameChange {
                versions: versions.clone(),
                language: names::language_label(&change.lang, db::primary_lang(lang)).into(),
                before: display(&change.before),
                after: display(&change.after),
            });
        }
    }
    ModelRc::new(VecModel::from(rows))
}

fn bundled_history() -> Option<&'static History> {
    static HISTORY: OnceLock<Result<History, String>> = OnceLock::new();
    HISTORY
        .get_or_init(|| {
            History::parse(REPORT)
                .inspect_err(|error| eprintln!("Could not read catalog history: {error}"))
        })
        .as_ref()
        .ok()
}

#[derive(Default)]
struct State {
    selected: Option<(usize, usize)>,
    cache: std::collections::VecDeque<((usize, usize), String, Rc<Feed>)>,
    expanded: std::collections::HashSet<String>,
    search_closed: std::collections::HashSet<String>,
    last_query: String,
    published: Option<Rc<Published>>,
    layout: Layout,
}

struct Controller {
    catalog: Rc<db::Catalog>,
    artwork: Rc<Artwork>,
    installed: OnceCell<Option<usize>>,
    state: RefCell<State>,
}
enum Action {
    Refresh,
    Pair(i32, i32),
    Search(SharedString),
    Extras(bool),
    Toggle(SharedString),
}

impl Controller {
    fn update(&self, app: &App, action: Action) {
        let _timing = crate::perf::Span::new("game_updates.update", 1);
        let history = bundled_history();
        let installed = *self.installed.get_or_init(|| {
            let installed = self.catalog.game_version()?;
            history?.versions.iter().position(|v| v == installed.trim())
        });
        let lang = app.get_lang().to_string();
        let Some(history) = history.filter(|h| h.versions.len() >= 2) else {
            app.set_game_update_summary(
                db::ui_text(
                    &lang,
                    "這個版本的資料庫沒有比對記錄。",
                    "No comparison is available for this catalog version.",
                )
                .into(),
            );
            return;
        };
        let mut state = self.state.borrow_mut();
        let previous_pair = state.selected;
        let latest = history.versions.len() - 1;
        let endpoint = installed.filter(|&i| i > 0).unwrap_or(latest);
        let mut pair = previous_pair.unwrap_or((endpoint.saturating_sub(1), endpoint));
        let mut query = app.get_game_update_query();
        let mut extras = app.get_game_update_show_extra();
        match &action {
            Action::Pair(from, to) => {
                let (Ok(from), Ok(to)) = (usize::try_from(*from), usize::try_from(*to)) else {
                    return;
                };
                if from >= to || to > latest {
                    return;
                }
                pair = (from, to);
            }
            Action::Search(text) => query = text.clone(),
            Action::Extras(show) => extras = *show,
            _ => {}
        }
        let selection_changed = previous_pair != Some(pair);
        let feed = if let Some(at) = state
            .cache
            .iter()
            .position(|(p, l, _)| *p == pair && l == &lang)
        {
            let entry = state.cache.remove(at).expect("selected cache entry");
            let feed = entry.2.clone();
            state.cache.push_front(entry);
            feed
        } else {
            let Some(report) = history.report(pair.0, pair.1) else {
                drop(state);
                app.set_game_update_summary(
                    db::ui_text(
                        &lang,
                        "這個版本的資料庫沒有比對記錄。",
                        "No comparison is available for this catalog version.",
                    )
                    .into(),
                );
                app.set_game_update_feed(ModelRc::default());
                app.set_game_update_lines(ModelRc::default());
                return;
            };
            let feed = Rc::new(Feed::new(report, &lang));
            state.cache.push_front((pair, lang.clone(), feed.clone()));
            state.cache.truncate(3);
            feed
        };
        if selection_changed {
            state.expanded = feed.default_expanded();
            state.search_closed.clear();
        }
        if state.last_query != query.as_str() {
            state.last_query = query.to_string();
            state.search_closed.clear();
        }
        if let Action::Extras(true) = action {
            state.expanded.insert("section:extra".into());
            state.search_closed.remove("section:extra");
        }
        state.selected = Some(pair);
        let toggles = if query.trim().is_empty() {
            &mut state.expanded
        } else {
            &mut state.search_closed
        };
        if let Action::Toggle(id) = action {
            if !toggles.remove(id.as_str()) {
                toggles.insert(id.to_string());
            }
        }
        let published = Rc::new(feed.publish(extras, &query, toggles));
        state.layout = published.layout();
        let lines = ModelRc::new(VecModel::from(state.layout.lines.clone()));
        state.published = Some(published.clone());
        drop(state);

        self.artwork.reset_targets();
        if selection_changed {
            let options = |versions: &[String]| {
                ModelRc::new(VecModel::from(
                    versions
                        .iter()
                        .map(|v| SharedString::from(v.as_str()))
                        .collect::<Vec<_>>(),
                ))
            };
            app.set_game_update_from_versions(options(&history.versions[..pair.1]));
            app.set_game_update_to_versions(options(&history.versions[pair.0 + 1..]));
        }
        app.set_game_update_versions(feed.versions.clone());
        app.set_game_update_from(pair.0 as i32);
        app.set_game_update_to(pair.1 as i32);
        app.set_game_update_query(query);
        app.set_game_update_show_extra(extras);
        app.set_game_update_summary(published.summary.as_str().into());
        app.set_game_update_feed(published.model(feed, installed.map(|_| self.artwork.clone())));
        app.set_game_update_lines(lines);
    }
}

pub fn install(app: &App, catalog: Rc<db::Catalog>, images: Rc<RefCell<HashMap<String, Image>>>) {
    let controller = Rc::new(Controller {
        artwork: Rc::new(Artwork::new(app, catalog.clone(), images)),
        catalog,
        installed: OnceCell::new(),
        state: RefCell::default(),
    });
    let binding = || (app.as_weak(), controller.clone());
    let (handle, state) = binding();
    app.on_refresh_game_update(move || {
        if let Some(app) = handle.upgrade() {
            state.update(&app, Action::Refresh);
        }
    });
    let (handle, state) = binding();
    app.on_select_game_update_versions(move |from, to| {
        if let Some(app) = handle.upgrade() {
            state.update(&app, Action::Pair(from, to));
        }
    });
    let (handle, state) = binding();
    app.on_search_game_updates(move |query| {
        if let Some(app) = handle.upgrade() {
            state.update(&app, Action::Search(query));
        }
    });
    let (handle, state) = binding();
    app.on_toggle_game_update_section(move |id| {
        if let Some(app) = handle.upgrade() {
            state.update(&app, Action::Toggle(id));
        }
    });
    let (handle, state) = binding();
    app.on_set_game_update_extras(move |show| {
        if let Some(app) = handle.upgrade() {
            state.update(&app, Action::Extras(show));
        }
    });
    let (_, state) = binding();
    app.on_game_update_line_at_y(move |y| state.state.borrow().layout.at_y(y));
    app.on_finish_game_update_icons(move || {
        let updated = controller.artwork.finish();
        let published = controller.state.borrow().published.clone();
        if let Some(model) = published {
            model.refresh_icons(&updated);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drop_rates_and_one_sided_changes_are_kept_in_field_detail() {
        let entry:Entry=serde_json::from_str(r#"{"category":"monsters","kind":"modified","table":"monster_drops","key":"M06 / 2","name":{},"before":{"drop_rate_per_100000":15000,"old_field":null},"after":{"drop_rate_per_100000":12000}}"#).unwrap();
        let fields = entry.fields("zh");
        let rate = fields.row_data(0).unwrap();
        assert_eq!(rate.label.as_str(), "掉落率");
        assert_eq!(rate.before.as_str(), "15%");
        assert_eq!(rate.after.as_str(), "12%");
        let old = fields.row_data(1).unwrap();
        assert_eq!(
            (old.label.as_str(), old.before.as_str(), old.after.as_str()),
            ("old_field", "null", "—")
        );
    }

    #[test]
    fn transparent_inventory_frames_are_distinguished_from_real_art() {
        let catalog = db::Catalog::open().unwrap();
        for code in [56, 13335, 13336, 13337] {
            if let Some((path, frame)) = catalog.calculator_icon(code) {
                let expected = crate::agf::frame_rgba(&path, frame)
                    .ok()
                    .is_some_and(|icon| icon.rgba.chunks_exact(4).any(|p| p[3] != 0));
                assert_eq!(
                    artwork::visible_frame(&path, frame).is_some(),
                    expected,
                    "{code}"
                );
            }
        }
    }
}
