//! A retained, offline promotion-bonus worksheet, separate from acquisition costs.
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use slint::{ComponentHandle, Image, Model, ModelRc, VecModel};

use crate::db::bonus::{
    bonus_text as tr, calculate_bonus, calculate_max_bonus, calculate_minimum_perfect_plan,
    BonusInput, BonusMinimumPlan, BonusNode,
};
use crate::{db, icons, App, BonusEdge, BonusRow, CharacterCalcChoice};

mod evidence;
use evidence::source_text;

type IconCache = Rc<RefCell<HashMap<String, Image>>>;

fn find_node<'a>(node: &'a BonusNode, key: &str) -> Option<&'a BonusNode> {
    if node.key == key {
        Some(node)
    } else {
        node.children
            .iter()
            .chain(&node.predecessor_options)
            .find_map(|child| find_node(child, key))
    }
}

struct TreeEntry<'a> {
    node: &'a BonusNode,
    depth: i32,
    active: bool,
    level_only: bool,
    source_owner: Option<&'a BonusNode>,
    x: f32,
}

/// Each terminal branch owns a column. A parent is centered above its children,
/// so a four-material legend reads as four parallel promotion histories.
fn tree_layout<'a>(
    tree: &'a BonusNode,
    inputs: &BTreeMap<String, BonusInput>,
) -> (Vec<TreeEntry<'a>>, Vec<BonusEdge>, i32, i32) {
    fn place<'a>(
        node: &'a BonusNode,
        depth: i32,
        active: bool,
        level_only: bool,
        source_owner: Option<&'a BonusNode>,
        inputs: &BTreeMap<String, BonusInput>,
        entries: &mut Vec<TreeEntry<'a>>,
        edges: &mut Vec<BonusEdge>,
        columns: &mut i32,
    ) -> f32 {
        let index = entries.len();
        entries.push(TreeEntry {
            node,
            depth,
            active,
            level_only,
            source_owner,
            x: 0.0,
        });
        let children_active = active
            && (depth == 0
                || inputs
                    .get(&node.key)
                    .is_none_or(|input| input.inherited.trim().is_empty()));
        let children = if level_only {
            Vec::new()
        } else {
            node.effective_children(inputs)
        };
        let positions: Vec<f32> = children
            .into_iter()
            .map(|child| {
                place(
                    child,
                    depth + 1,
                    children_active,
                    node.uses_levels,
                    (!node.predecessor_options.is_empty()).then_some(node),
                    inputs,
                    entries,
                    edges,
                    columns,
                )
            })
            .collect();
        let x = if positions.is_empty() {
            let x = *columns as f32 + 0.5;
            *columns += 1;
            x
        } else {
            (positions[0] + positions[positions.len() - 1]) / 2.0
        };
        entries[index].x = x;
        for child_x in positions {
            edges.push(BonusEdge {
                from_x: x,
                from_y: depth as f32 - 1.0,
                to_x: child_x,
                to_y: depth as f32,
            });
        }
        x
    }
    let mut entries = Vec::new();
    let mut edges = Vec::new();
    let mut columns = 0;
    place(
        tree,
        0,
        true,
        false,
        None,
        inputs,
        &mut entries,
        &mut edges,
        &mut columns,
    );
    let depth = entries.iter().map(|entry| entry.depth).max().unwrap_or(0);
    (entries, edges, columns, depth)
}

fn localized_error(lang: &str, error: &str) -> String {
    let (name, message) = error.split_once(" · ").unwrap_or(("", error));
    let parts: Vec<_> = message.split(" / ").collect();
    let message = if parts.len() == 3 {
        tr(lang, parts[0], parts[1], parts[2]).to_string()
    } else {
        db::localized_error(lang, message)
    };
    if name.is_empty() {
        message
    } else {
        format!("{name} · {message}")
    }
}

struct Controller {
    catalog: Rc<db::Catalog>,
    cache: IconCache,
    tree: RefCell<Option<BonusNode>>,
    inputs: RefCell<BTreeMap<String, BonusInput>>,
    groups: RefCell<Vec<String>>,
    group: RefCell<String>,
    query: RefCell<String>,
    selected_key: RefCell<String>,
    rows: Rc<VecModel<BonusRow>>,
    plan_receiver: RefCell<Option<mpsc::Receiver<Result<BonusMinimumPlan, String>>>>,
    plan_timer: slint::Timer,
}

impl Controller {
    fn invalidate_plan(&self, app: &App) {
        self.plan_timer.stop();
        self.plan_receiver.borrow_mut().take();
        app.set_bonus_optimizing(false);
        app.set_bonus_plan_status("".into());
    }
    fn image(&self, code: &str) -> (Image, bool) {
        let image = self
            .catalog
            .character_calculator_icon(&format!("hero:{code}"))
            .and_then(|(path, frame)| icons::cached_icon(&self.cache, &path, frame));
        let present = image.is_some();
        (image.unwrap_or_default(), present)
    }

    fn search(&self, app: &App, query: &str) {
        *self.query.borrow_mut() = query.into();
        let candidates = self
            .catalog
            .bonus_candidates(query, &self.group.borrow(), &app.get_lang())
            .into_iter()
            .map(|candidate| {
                let (icon, has_icon) = self.image(&candidate.code);
                CharacterCalcChoice {
                    alternate_name: self.catalog.record_alternate_name(&format!("hero:{}", candidate.code), &app.get_lang()).into(),
                    id: candidate.code.into(),
                    title: candidate.title.into(),
                    subtitle: candidate.subtitle.into(),
                    icon,
                    has_icon,
                }
            })
            .collect::<Vec<_>>();
        app.set_bonus_candidates(ModelRc::new(VecModel::from(candidates)));
    }

    fn refresh(&self, app: &App) {
        self.invalidate_plan(app);
        app.set_bonus_source_text(source_text(&app.get_lang()).into());
        let groups = self.catalog.bonus_groups(&app.get_lang());
        let selected = groups
            .iter()
            .position(|group| group.id == *self.group.borrow())
            .unwrap_or(0);
        *self.groups.borrow_mut() = groups.iter().map(|group| group.id.clone()).collect();
        app.set_bonus_groups(ModelRc::new(VecModel::from(
            groups
                .into_iter()
                .map(|group| format!("{} ({})", group.title, group.count).into())
                .collect::<Vec<_>>(),
        )));
        app.set_bonus_group_index(selected as i32);
        let query = self.query.borrow().clone();
        self.search(app, &query);
        let code = self.tree.borrow().as_ref().map(|tree| tree.code.clone());
        if let Some(code) = code {
            match self.catalog.bonus_tree(&code, &app.get_lang()) {
                Ok(tree) => *self.tree.borrow_mut() = Some(tree),
                Err(error) => {
                    app.set_bonus_error(error.into());
                    return;
                }
            }
        }
        self.render(app);
    }

    fn select(&self, app: &App, code: &str) {
        self.invalidate_plan(app);
        match self.catalog.bonus_tree(code, &app.get_lang()) {
            Ok(tree) => {
                self.inputs.borrow_mut().clear();
                *self.tree.borrow_mut() = Some(tree);
                self.render(app);
            }
            Err(error) => {
                // A failed selection must never leave a previous target's result displayed.
                self.tree.borrow_mut().take();
                self.rows.set_vec(Vec::new());
                app.set_bonus_edges(ModelRc::default());
                app.set_bonus_selected_row(BonusRow::default());
                app.set_bonus_selected_key("".into());
                app.set_bonus_maximum("".into());
                app.set_bonus_maximum_label("".into());
                app.set_bonus_verification("".into());
                app.set_bonus_perfect(false);
                app.set_bonus_maximum_note("".into());
                app.set_bonus_selected_name("".into());
                app.set_bonus_selected_alternate_name("".into());
                app.set_bonus_selected_icon(Image::default());
                app.set_bonus_selected_has_icon(false);
                app.set_bonus_note("".into());
                app.set_bonus_total("".into());
                app.set_bonus_formula("".into());
                app.set_bonus_error(error.into());
                app.set_bonus_has_white(false);
                app.set_bonus_can_optimize(false);
            }
        }
    }

    fn render(&self, app: &App) {
        let lang = app.get_lang();
        let borrowed = self.tree.borrow();
        let Some(tree) = borrowed.as_ref() else {
            return;
        };
        let inputs = self.inputs.borrow();
        let result = calculate_bonus(tree, &inputs);
        let (icon, has_icon) = self.image(&tree.code);
        app.set_bonus_selected_name(tree.title.clone().into());
        app.set_bonus_selected_alternate_name(tree.alternate_name.clone().into());
        app.set_bonus_selected_icon(icon);
        app.set_bonus_selected_has_icon(has_icon);
        app.set_bonus_error(
            result
                .as_ref()
                .err()
                .map(|error| localized_error(&lang, error))
                .unwrap_or_default()
                .into(),
        );
        app.set_bonus_total(
            result
                .as_ref()
                .map(|result| result.bonus.to_string())
                .unwrap_or_default()
                .into(),
        );
        let maximum = calculate_max_bonus(tree, &inputs);
        app.set_bonus_can_optimize(maximum.as_ref().is_ok_and(|value| value.supports_perfect()));
        let has_manual_history = result.as_ref().is_ok_and(|value| {
            value.lines.iter().any(|line| {
                line.key != tree.key
                    && inputs
                        .get(&line.key)
                        .is_some_and(|input| !input.inherited.trim().is_empty())
            })
        });
        app.set_bonus_maximum(
            maximum
                .as_ref()
                .map(|value| value.bonus.to_string())
                .unwrap_or_default()
                .into(),
        );
        app.set_bonus_perfect(
            !has_manual_history
                && maximum.as_ref().is_ok_and(|maximum| {
                    maximum.supports_perfect()
                        && result
                            .as_ref()
                            .is_ok_and(|value| value.bonus == maximum.bonus)
                }),
        );
        app.set_bonus_maximum_label(
            if maximum.as_ref().is_ok_and(|value| value.supports_perfect()) {
                tr(&lang, "無重生上限", "No-rebirth max", "환생 제외 최대")
            } else {
                tr(
                    &lang,
                    "無白衣／無重生上限",
                    "No-white / no-rebirth max",
                    "백의·환생 제외 최대",
                )
            }
            .into(),
        );
        app.set_bonus_maximum_note(match &maximum {
            Err(error) => localized_error(&lang, error),
            Ok(_) if has_manual_history => tr(&lang,
                "已手填歷史紅利：可對照上限，但不判定完美。",
                "Historical points were entered manually: compare the maximum, but perfection cannot be verified.",
                "과거 보너스를 직접 입력했으므로 최대값만 비교하고 완벽 여부는 판정하지 않습니다.").into(),
            Ok(value) if value.supports_perfect() => tr(&lang,
                "按各素材等級上限逐階取整；等於上限即為無重生完美，不必每階都滿級。傳說上限含各素材白衣 300；印度路線按四國兵先完成白衣再轉印度計算。",
                "Each material uses its own cap, rounded down at every promotion. Matching this maximum is perfect without rebirth; every stage need not be max level. Legends include 300 white-robe points per material, completed before converting to India.",
                "재료별 상한과 단계별 버림으로 계산합니다. 최대값과 같으면 모든 단계가 만렙이 아니어도 환생 제외 완벽입니다. 전설은 재료별 백의 300을 포함하며 인도 전환 전에 완료한 이력입니다.").into(),
            Ok(_) => tr(&lang,
                "這是無白衣、無重生的滿級參考值；尚未涵蓋所有白衣歷程，不標示完美。",
                "This reference excludes white-robe and rebirth history. It is not a verified full-history maximum and is not marked perfect.",
                "백의·환생을 제외한 만렙 참고값입니다. 전체 이력의 최대값은 아니므로 완벽으로 표시하지 않습니다.").into(),
        }.into());
        let benchmark = match tree.code.as_str() {
            "T92" => Some(1013),
            "V16" => Some(1080),
            "V27" => Some(955),
            "V26" => Some(968),
            "V28" => Some(956),
            "V29" => Some(976),
            _ => None,
        };
        app.set_bonus_verification(
            match (benchmark, &maximum) {
                (Some(expected), Ok(maximum)) if expected == maximum.bonus => format!(
                    "{} {} · {}",
                    tr(
                        &lang,
                        "Geota 上限交叉核對",
                        "Geota maximum cross-check",
                        "거타 최대값 교차 확인"
                    ),
                    expected,
                    tr(&lang, "一致", "matched", "일치")
                ),
                (Some(expected), Ok(_)) => format!(
                    "{} {}",
                    tr(
                        &lang,
                        "獨立參考值（與目前模型不同）：",
                        "Independent reference (differs from current model): ",
                        "독립 참고값 (현재 모델과 다름): "
                    ),
                    expected
                ),
                _ => String::new(),
            }
            .into(),
        );
        app.set_bonus_formula(tree.rule.clone().into());
        app.set_bonus_note(tr(&lang,
            "點選節點查看計算明細。各階段捨去小數，不計裝備／皮膚。印度前身四選一；已有白衣或重生歷程可覆寫實際繼承紅利。",
            "Select a unit for details. Each stage rounds down; equipment and skins are excluded. Indian units use one of four former mercenaries. Override inherited points for existing white-robe or rebirth history.",
            "용병을 선택하면 계산 내역을 확인합니다. 단계마다 소수점을 버리며 장비·외형은 제외합니다. 인도 용병은 이전 용병 4종 중 1종을 선택합니다. 백의·환생 이력은 실제 보너스를 입력하세요.").into());
        let (nodes, edges, columns, depth) = tree_layout(tree, &inputs);
        app.set_bonus_edges(ModelRc::new(VecModel::from(edges)));
        app.set_bonus_tree_columns(columns);
        app.set_bonus_tree_center(nodes.first().map(|entry| entry.x).unwrap_or(0.5));
        app.set_bonus_tree_depth(depth);
        let lines = result.as_ref().ok();
        let rows = nodes
            .into_iter()
            .map(|entry| {
                let TreeEntry {
                    node,
                    depth,
                    active,
                    level_only,
                    source_owner,
                    x,
                } = entry;
                let input = inputs
                    .get(&node.key)
                    .cloned()
                    .unwrap_or_else(|| node.default_input());
                let line =
                    lines.and_then(|result| result.lines.iter().find(|line| line.key == node.key));
                let (icon, has_icon) = self.image(&node.code);
                let detail =
                    if !active {
                        tr(&lang, "前階段已由既有紅利或素材等級取代，不參與本次計算。",
                        "Earlier history is replaced by an existing bonus or the material's level.",
                        "이전 이력은 기존 보너스 또는 재료 레벨로 대체됩니다.").into()
                    } else if level_only {
                        tr(&lang, "此合成只取素材等級，不取屬性及繼承紅利。",
                        "This synthesis uses only the material's level, not its stats or bonus.",
                        "이 합성은 능력치나 보너스 없이 재료 레벨만 사용합니다.").into()
                    } else if depth == 0 {
                        String::new()
                    } else if let Some(line) = line {
                        format!(
                            "{} {} + {} {} + {} {} = {}{}",
                            tr(&lang, "基礎", "Base", "기본"),
                            line.base_stats,
                            tr(&lang, "等級", "Level", "레벨"),
                            line.level_points,
                            tr(&lang, "繼承", "Inherited", "보너스"),
                            line.bonus,
                            line.total_stats,
                            if input.inherited.is_empty() {
                                ""
                            } else {
                                tr(&lang, "（手動）", " (manual)", " (직접 입력)")
                            }
                        )
                    } else {
                        node.rule.clone()
                    };
                let detail = if node.warning.is_empty() || !active || level_only {
                    detail
                } else {
                    format!("{detail}\n{}", node.warning)
                };
                let detail = if active && depth > 0 {
                    format!(
                        "{detail} · {} {}–{}",
                        tr(&lang, "等級", "Level", "레벨"),
                        node.min_level,
                        node.max_level
                    )
                } else {
                    detail
                };
                BonusRow {
                    key: node.key.clone().into(),
                    title: node.title.clone().into(),
                    alternate_name: node.alternate_name.clone().into(),
                    detail: detail.into(),
                    depth,
                    x,
                    y: depth as f32 - 1.0,
                    active,
                    source_owner: source_owner
                        .map(|owner| owner.key.as_str())
                        .unwrap_or_default()
                        .into(),
                    source_options: ModelRc::new(VecModel::from(
                        source_owner
                            .map(|owner| {
                                owner
                                    .predecessor_options
                                    .iter()
                                    .map(|option| option.title.clone().into())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                    )),
                    source_choices: ModelRc::new(VecModel::from(
                        source_owner
                            .map(|owner| {
                                owner
                                    .predecessor_options
                                    .iter()
                                    .map(|option| {
                                        let (icon, has_icon) = self.image(&option.code);
                                        CharacterCalcChoice {
                                            id: option.code.clone().into(),
                                            title: option.title.clone().into(),
                                            alternate_name: option.alternate_name.clone().into(),
                                            subtitle: Default::default(),
                                            icon,
                                            has_icon,
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                    )),
                    source_codes: ModelRc::new(VecModel::from(
                        source_owner
                            .map(|owner| {
                                owner
                                    .predecessor_options
                                    .iter()
                                    .map(|option| option.code.clone().into())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                    )),
                    source_index: source_owner
                        .and_then(|owner| {
                            owner
                                .predecessor_options
                                .iter()
                                .position(|option| option.key == node.key)
                        })
                        .unwrap_or(0) as i32,
                    min_level: node.min_level as i32,
                    max_level: node.max_level as i32,
                    level: input.level.into(),
                    inherited: input.inherited.into(),
                    white: input.white.into(),
                    white_enabled: node.white_enabled && active && !level_only,
                    indian_white_history: node.nation == 10000 && node.white_enabled && !level_only,
                    white_before_india: input.white_before_india,
                    bonus: line
                        .filter(|_| !level_only)
                        .map(|line| line.bonus.to_string())
                        .unwrap_or_default()
                        .into(),
                    icon,
                    has_icon,
                    editable: depth > 0 && active,
                    inherited_editable: depth > 0 && active && !level_only,
                }
            })
            .collect::<Vec<_>>();
        let selected = rows
            .iter()
            .find(|row| row.key == self.selected_key.borrow().as_str())
            .or_else(|| rows.iter().find(|row| row.depth == 1))
            .or_else(|| rows.first());
        app.set_bonus_has_white(rows.iter().any(|row| row.white_enabled));
        if let Some(selected) = selected {
            *self.selected_key.borrow_mut() = selected.key.to_string();
            app.set_bonus_selected_key(selected.key.clone());
            app.set_bonus_selected_row(selected.clone());
        }
        // Keep component instances and input focus while editing a retained worksheet.
        let same_tree = self.rows.row_count() == rows.len()
            && rows.iter().enumerate().all(|(index, row)| {
                self.rows.row_data(index).is_some_and(|old| {
                    old.key == row.key
                        || (!row.source_owner.is_empty() && old.source_owner == row.source_owner)
                })
            });
        if same_tree {
            for (index, row) in rows.into_iter().enumerate() {
                self.rows.set_row_data(index, row);
            }
        } else {
            self.rows.set_vec(rows);
        }
    }

    fn edit(&self, app: &App, key: &str, field: &str, value: &str) {
        // Ignore stale callbacks after a different target was selected.
        let Some(row) = self.rows.iter().find(|row| row.key == key) else {
            return;
        };
        self.invalidate_plan(app);
        if field == "predecessor" {
            if !row.active {
                return;
            }
            let borrowed = self.tree.borrow();
            let Some(owner) = borrowed.as_ref().and_then(|tree| find_node(tree, key)) else {
                return;
            };
            let Some(choice) = owner
                .predecessor_options
                .iter()
                .find(|option| option.code == value)
            else {
                return;
            };
            self.inputs
                .borrow_mut()
                .entry(key.into())
                .or_insert_with(|| owner.default_input())
                .predecessor = value.into();
            *self.selected_key.borrow_mut() = choice.key.clone();
            drop(borrowed);
            self.render(app);
            return;
        }
        if !row.editable {
            return;
        }
        let borrowed = self.tree.borrow();
        let Some(node) = borrowed.as_ref().and_then(|tree| find_node(tree, key)) else {
            return;
        };
        let mut inputs = self.inputs.borrow_mut();
        let input = inputs
            .entry(key.into())
            .or_insert_with(|| node.default_input());
        match field {
            "level" => input.level = value.into(),
            "inherited" if row.inherited_editable => input.inherited = value.into(),
            "white" if row.white_enabled => input.white = value.into(),
            "white-before-india" if row.indian_white_history => {
                input.white_before_india = value == "true"
            }
            _ => return,
        }
        drop(inputs);
        drop(borrowed);
        self.render(app);
    }

    fn select_node(&self, app: &App, key: &str) {
        if let Some(row) = self.rows.iter().find(|row| row.key == key) {
            *self.selected_key.borrow_mut() = key.into();
            app.set_bonus_selected_key(row.key.clone());
            app.set_bonus_selected_row(row);
        }
    }

    fn preset(&self, app: &App, level: i32) {
        if !matches!(level, 0 | 200 | 250 | 260) {
            return;
        }
        self.invalidate_plan(app);
        let mut inputs = self.inputs.borrow_mut();
        if level == 0 {
            inputs.clear();
        } else {
            let borrowed = self.tree.borrow();
            if let Some(tree) = borrowed.as_ref() {
                for row in self.rows.iter().filter(|row| row.depth > 0) {
                    if let Some(node) = find_node(tree, &row.key) {
                        inputs
                            .entry(row.key.to_string())
                            .or_insert_with(|| node.default_input())
                            .level = (level as i64)
                            .clamp(node.min_level, node.max_level)
                            .to_string();
                    }
                }
            }
        }
        drop(inputs);
        self.render(app);
    }

    fn white_preset(&self, app: &App) {
        self.invalidate_plan(app);
        let rows: Vec<_> = self.rows.iter().filter(|row| row.white_enabled).collect();
        if rows.is_empty() {
            return;
        }
        let borrowed = self.tree.borrow();
        let Some(tree) = borrowed.as_ref() else {
            return;
        };
        let mut inputs = self.inputs.borrow_mut();
        let mut india = false;
        for row in rows {
            let Some(node) = find_node(tree, &row.key) else {
                continue;
            };
            let input = inputs
                .entry(row.key.to_string())
                .or_insert_with(|| node.default_input());
            input.white = "300".into();
            if row.indian_white_history && input.inherited.trim().is_empty() {
                input.white_before_india = true;
                india = true;
            }
        }
        drop(inputs);
        drop(borrowed);
        self.render(app);
        app.set_bonus_plan_status(
            if india {
                tr(
                    &app.get_lang(),
                    "已設白衣 300；印度素材按四國兵階段先完成白衣計算。",
                    "White robes set to 300; Indian routes assume completion before conversion.",
                    "백의 300 적용. 인도 재료는 전직 전에 완료한 이력입니다.",
                )
            } else {
                tr(
                    &app.get_lang(),
                    "可用白衣的素材已全部設為 300。",
                    "Every eligible material now has 300 white-robe points.",
                    "백의 가능한 재료를 모두 300으로 설정했습니다.",
                )
            }
            .into(),
        );
    }

    fn optimize(self: &Rc<Self>, app: &App) {
        let Some(tree) = self.tree.borrow().clone() else {
            return;
        };
        self.invalidate_plan(app);
        let inputs = self.inputs.borrow().clone();
        let (sender, receiver) = mpsc::channel();
        *self.plan_receiver.borrow_mut() = Some(receiver);
        app.set_bonus_optimizing(true);
        app.set_bonus_plan_status(
            tr(
                &app.get_lang(),
                "正在搜尋最低總經驗配置…",
                "Finding the lowest total-experience plan…",
                "최소 총 경험치 계획 계산 중…",
            )
            .into(),
        );
        std::thread::spawn(move || {
            let _ = sender.send(calculate_minimum_perfect_plan(&tree, &inputs));
        });
        let weak_app = app.as_weak();
        let weak_controller = Rc::downgrade(self);
        self.plan_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(50),
            move || {
                if let (Some(controller), Some(app)) =
                    (weak_controller.upgrade(), weak_app.upgrade())
                {
                    controller.receive_plan(&app);
                }
            },
        );
    }

    fn receive_plan(&self, app: &App) {
        let received = self
            .plan_receiver
            .borrow()
            .as_ref()
            .map(|receiver| receiver.try_recv());
        let result = match received {
            Some(Ok(result)) => result,
            Some(Err(mpsc::TryRecvError::Empty)) | None => return,
            Some(Err(mpsc::TryRecvError::Disconnected)) => Err("規劃未完成，請重試 / Planning did not finish; retry / 계산이 완료되지 않았습니다. 다시 시도하세요".into()),
        };
        self.plan_timer.stop();
        self.plan_receiver.borrow_mut().take();
        app.set_bonus_optimizing(false);
        match result {
            Ok(plan) => {
                *self.inputs.borrow_mut() = plan.inputs;
                self.render(app);
                app.set_bonus_plan_status(
                    format!(
                        "{} {} XP · {} {} XP · {}",
                        tr(
                            &app.get_lang(),
                            "已套用理論最低經驗",
                            "Least modeled XP applied:",
                            "이론상 최소 경험치 적용:"
                        ),
                        grouped(plan.total_experience),
                        tr(
                            &app.get_lang(),
                            "比全滿節省",
                            "saved vs. max levels:",
                            "최대레벨 대비 절약:"
                        ),
                        grouped(plan.saved_experience),
                        tr(
                            &app.get_lang(),
                            "不計白衣重練",
                            "white-robe retraining excluded",
                            "백의 재육성 제외"
                        )
                    )
                    .into(),
                );
            }
            Err(error) => {
                app.set_bonus_plan_status(localized_error(&app.get_lang(), &error).into())
            }
        }
    }
}

fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub(crate) fn install(app: &App, catalog: Rc<db::Catalog>, cache: IconCache) {
    let controller = Rc::new(Controller {
        catalog,
        cache,
        tree: RefCell::new(None),
        inputs: RefCell::new(BTreeMap::new()),
        groups: RefCell::new(Vec::new()),
        group: RefCell::new(String::new()),
        query: RefCell::new(String::new()),
        selected_key: RefCell::new(String::new()),
        rows: Rc::new(VecModel::default()),
        plan_receiver: RefCell::new(None),
        plan_timer: slint::Timer::default(),
    });
    app.set_bonus_rows(ModelRc::from(controller.rows.clone()));
    let weak = app.as_weak();
    let c = controller.clone();
    app.on_bonus_refresh(move || {
        if let Some(app) = weak.upgrade() {
            c.refresh(&app);
        }
    });
    let weak = app.as_weak();
    let c = controller.clone();
    app.on_bonus_search(move |query| {
        if let Some(app) = weak.upgrade() {
            c.search(&app, &query);
        }
    });
    let weak = app.as_weak();
    let c = controller.clone();
    app.on_bonus_select_character(move |code| {
        if let Some(app) = weak.upgrade() {
            c.select(&app, &code);
        }
    });
    let weak = app.as_weak();
    let c = controller.clone();
    app.on_bonus_select_group(move |index| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let Some(group) = c.groups.borrow().get(index as usize).cloned() else {
            return;
        };
        *c.group.borrow_mut() = group;
        app.set_bonus_group_index(index);
        c.search(&app, "");
    });
    let weak = app.as_weak();
    let c = controller.clone();
    app.on_bonus_input_changed(move |key, field, value| {
        if let Some(app) = weak.upgrade() {
            c.edit(&app, &key, &field, &value);
        }
    });
    let weak = app.as_weak();
    let c = controller.clone();
    app.on_bonus_preset(move |level| {
        if let Some(app) = weak.upgrade() {
            c.preset(&app, level);
        }
    });
    let weak = app.as_weak();
    let c = controller.clone();
    app.on_bonus_select_node(move |key| {
        if let Some(app) = weak.upgrade() {
            c.select_node(&app, &key);
        }
    });
    let weak = app.as_weak();
    let c = controller.clone();
    app.on_bonus_white_preset(move || {
        if let Some(app) = weak.upgrade() {
            c.white_preset(&app);
        }
    });
    let weak = app.as_weak();
    let c = controller.clone();
    app.on_bonus_optimize(move || {
        if let Some(app) = weak.upgrade() {
            c.optimize(&app);
        }
    });
    app.on_bonus_open_cross_check(move || {
        open_reference("https://geota.co.kr/gersang/calculator/bonus");
    });
    controller.refresh(app);
}

fn open_reference(url: &str) {
    let url = url.to_owned();
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        let result = std::process::Command::new("/usr/bin/open")
            .arg(url)
            .status();
        #[cfg(windows)]
        let result = std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", &url])
            .status();
        if let Err(error) = result {
            eprintln!("Cannot open bonus reference: {error}");
        }
    });
}

#[cfg(test)]
mod tests;
