//! Exact minimum-experience plans for an already selected promotion tree.
//!
//! A state records a loss from that node's all-cap attribute total. It retains
//! the greatest cumulative experience saving for that EXACT loss. Child states
//! are convolved, then the actual promotion divisor is applied before the
//! node's own level contribution is added. Thus fractional points are never
//! pooled across promotions and neither greedy per-node minima nor independent
//! lower bounds are mistaken for a jointly attainable plan.
use super::*;

#[path = "bonus_experience.rs"]
mod experience;

fn cumulative_experience(level: i64) -> Result<u64, String> {
    experience::cumulative_experience(level).ok_or_else(|| {
        "經驗曲線只支援 1–260 級 / Experience curve supports levels 1–260 / 경험치 표는 1–260레벨을 지원합니다".into()
    })
}

/// One globally minimum-XP plan within the modeled, selected route. All levels
/// are simultaneous: the returned inputs still attain `bonus` after every floor.
/// This is an arithmetic training plan. It includes the modeled level minima,
/// but does not certify quest, item, nationality, or other promotion eligibility.
/// White-robe training itself is not included in the experience objective; its
/// completed 300 points are an explicit fixed prerequisite for legend plans.
/// Synthetic statues are charged the XP of training a source from level one to
/// their recorded level; the source's earlier, unmodeled formation is excluded.
#[derive(Clone, Debug)]
pub struct BonusMinimumPlan {
    pub inputs: BTreeMap<String, BonusInput>,
    pub bonus: i64,
    pub total_experience: u64,
    pub maximum_total_experience: u64,
    pub saved_experience: u64,
    pub material_count: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Saving {
    experience: u64,
    // A deterministic secondary objective when two configurations have exactly
    // equal cumulative XP. It never trades away a primary-objective improvement.
    levels: i64,
}

impl std::ops::Add for Saving {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            experience: self.experience + rhs.experience,
            levels: self.levels + rhs.levels,
        }
    }
}

#[derive(Clone, Debug)]
struct Entry {
    saving: Saving,
    level: i64,
    child_losses: Vec<usize>,
}

struct SolvedNode<'a> {
    node: &'a BonusNode,
    entries: Vec<Option<Entry>>,
    children: Vec<SolvedNode<'a>>,
}

struct Reference<'a> {
    maximum_inputs: &'a BTreeMap<String, BonusInput>,
    maximum_lines: BTreeMap<&'a str, &'a BonusLine>,
    minimum_lines: BTreeMap<&'a str, &'a BonusLine>,
}

fn contribution(line: &BonusLine, root: bool, level_only: bool) -> i64 {
    if root {
        line.bonus
    } else if level_only {
        line.level
    } else {
        line.total_stats
    }
}

impl Reference<'_> {
    fn solve<'a>(
        &self,
        node: &'a BonusNode,
        root: bool,
        level_only: bool,
        indian_white_history: i64,
        budget: usize,
    ) -> Result<SolvedNode<'a>, String> {
        let maximum = self.maximum_lines[node.key.as_str()];
        let minimum = self.minimum_lines[node.key.as_str()];
        let budget = budget.min(
            (contribution(maximum, root, level_only) - contribution(minimum, root, level_only))
                as usize,
        );
        let input = &self.maximum_inputs[&node.key];
        let indian_white_history =
            if node.nation == 10000 && input.white_before_india && maximum.white > 0 {
                maximum.white
            } else {
                indian_white_history
            };
        let child_nodes = if level_only {
            Vec::new()
        } else {
            node.effective_children(self.maximum_inputs)
        };
        let mut children = Vec::new();
        let mut combinations = vec![Some(Entry {
            saving: Saving::default(),
            level: 1,
            child_losses: Vec::new(),
        })];
        // max_numerator / denominator is exactly the engine's incoming bonus.
        // All white points and India history are fixed; only pure attributes
        // (or levels for a level-only recipe) vary in this optimization.
        let factor = node.white_divisor.max(1);
        let denominator = node.divisor * factor;
        let max_numerator = if child_nodes.is_empty() {
            0
        } else {
            let pure: i64 = child_nodes
                .iter()
                .map(|child| {
                    let line = self.maximum_lines[child.key.as_str()];
                    if node.uses_levels {
                        line.level
                    } else {
                        line.total_stats
                            + if node.nation == 10000 && matches!(node.rank, 1 | 2) {
                                indian_white_history
                            } else {
                                0
                            }
                    }
                })
                .sum();
            let whites: i64 = if node.white_divisor > 0 {
                child_nodes
                    .iter()
                    .map(|child| self.maximum_lines[child.key.as_str()].white)
                    .sum()
            } else {
                0
            };
            pure * factor + whites * node.divisor
        };
        if !child_nodes.is_empty() {
            if denominator <= 0 {
                return Err(invalid(
                    node,
                    "尚無可驗證的紅利公式 / Bonus formula unavailable / 보너스 공식 미확인",
                ));
            }
            debug_assert_eq!(max_numerator / denominator, maximum.bonus);
            let minimum_bonus = (maximum.bonus - budget as i64).max(0);
            let available_loss: i64 = child_nodes
                .iter()
                .map(|child| {
                    contribution(
                        self.maximum_lines[child.key.as_str()],
                        false,
                        node.uses_levels,
                    ) - contribution(
                        self.minimum_lines[child.key.as_str()],
                        false,
                        node.uses_levels,
                    )
                })
                .sum();
            let child_budget = ((max_numerator - minimum_bonus * denominator) / factor)
                .min(available_loss) as usize;
            // Real client trees are far below this bound. Reject malformed or
            // hypothetical oversized models before allocating a huge DP table.
            if child_budget > 200_000 {
                return Err("紅利鏈超出精確規劃範圍 / Bonus tree exceeds the exact planner limit / 정확한 계획 범위를 초과했습니다".into());
            }
            for child in child_nodes {
                let solved = self.solve(
                    child,
                    false,
                    node.uses_levels,
                    indian_white_history,
                    child_budget,
                )?;
                let next_len =
                    (combinations.len() + solved.entries.len() - 1).min(child_budget + 1);
                let mut next: Vec<Option<Entry>> = vec![None; next_len];
                for (prior_loss, prior) in combinations.iter().enumerate() {
                    let Some(prior) = prior else { continue };
                    for (child_loss, entry) in solved
                        .entries
                        .iter()
                        .enumerate()
                        .take(next_len.saturating_sub(prior_loss))
                    {
                        let Some(entry) = entry else { continue };
                        let loss = prior_loss + child_loss;
                        let saving = prior.saving + entry.saving;
                        if next[loss]
                            .as_ref()
                            .is_none_or(|current| saving > current.saving)
                        {
                            let mut child_losses = prior.child_losses.clone();
                            child_losses.push(child_loss);
                            next[loss] = Some(Entry {
                                saving,
                                level: 1,
                                child_losses,
                            });
                        }
                    }
                }
                combinations = next;
                children.push(solved);
            }
        }
        let mut entries: Vec<Option<Entry>> = vec![None; budget + 1];
        let level_range = if root {
            1..=1
        } else {
            node.min_level..=node.max_level
        };
        let maximum_experience = if root {
            0
        } else {
            cumulative_experience(node.max_level)?
        };
        for level in level_range {
            if !root && !node.allows_level(level) {
                continue;
            }
            let own_loss = if root {
                0
            } else if level_only {
                node.max_level - level
            } else {
                maximum.level_points - bonus_level_points(level)?
            } as usize;
            if own_loss > budget {
                continue;
            }
            let own_saving = Saving {
                experience: maximum_experience - cumulative_experience(level)?,
                levels: if root { 0 } else { node.max_level - level },
            };
            for (child_loss, combination) in combinations.iter().enumerate() {
                let Some(combination) = combination else {
                    continue;
                };
                let bonus_loss = if children.is_empty() {
                    0
                } else {
                    maximum.bonus - (max_numerator - child_loss as i64 * factor) / denominator
                } as usize;
                let loss = own_loss + bonus_loss;
                if loss > budget {
                    continue;
                }
                let saving = own_saving + combination.saving;
                if entries[loss]
                    .as_ref()
                    .is_none_or(|current| saving > current.saving)
                {
                    entries[loss] = Some(Entry {
                        saving,
                        level,
                        child_losses: combination.child_losses.clone(),
                    });
                }
            }
        }
        Ok(SolvedNode {
            node,
            entries,
            children,
        })
    }
}

impl SolvedNode<'_> {
    fn reconstruct(&self, loss: usize, inputs: &mut BTreeMap<String, BonusInput>) {
        let entry = self.entries[loss]
            .as_ref()
            .expect("reachable exact-plan state");
        inputs
            .get_mut(&self.node.key)
            .expect("maximum input exists")
            .level = entry.level.to_string();
        for (child, child_loss) in self.children.iter().zip(&entry.child_losses) {
            child.reconstruct(*child_loss, inputs);
        }
    }
}

/// Minimize the sum of cumulative client mercenary XP across all used material
/// stages, from level one, while attaining the selected route's verified cap.
/// The target's new level-one form is excluded. Alternative sources stay fixed;
/// manual inherited values are replaced by the complete modeled history.
/// Legends assume all eligible white-robe inputs are 300 (before India where
/// applicable). Those assumptions match `calculate_max_bonus` exactly.
pub fn calculate_minimum_perfect_plan(
    tree: &BonusNode,
    inputs: &BTreeMap<String, BonusInput>,
) -> Result<BonusMinimumPlan, String> {
    let maximum = calculate_max_bonus(tree, inputs)?;
    if !maximum.supports_perfect() {
        return Err("此路線尚未確認完整完美上限 / This route has no verified complete maximum / 전체 최대 보너스가 확인되지 않은 경로입니다".into());
    }
    let maximum_inputs = maximum_bonus_inputs(tree, inputs)?;
    let maximum_result = calculate_bonus(tree, &maximum_inputs)?;
    let mut minimum_inputs = maximum_inputs.clone();
    fn set_minimum(node: &BonusNode, inputs: &mut BTreeMap<String, BonusInput>, level_only: bool) {
        inputs
            .get_mut(&node.key)
            .expect("selected input exists")
            .level = node.min_level.to_string();
        if !level_only {
            let children = node.effective_children(inputs);
            for child in children {
                set_minimum(child, inputs, node.uses_levels);
            }
        }
    }
    set_minimum(tree, &mut minimum_inputs, false);
    let minimum_result = calculate_bonus(tree, &minimum_inputs)?;
    let reference = Reference {
        maximum_inputs: &maximum_inputs,
        maximum_lines: maximum_result
            .lines
            .iter()
            .map(|line| (line.key.as_str(), line))
            .collect(),
        minimum_lines: minimum_result
            .lines
            .iter()
            .map(|line| (line.key.as_str(), line))
            .collect(),
    };
    let solved = reference.solve(tree, true, false, 0, 0)?;
    let saving = solved.entries[0]
        .as_ref()
        .ok_or_else(|| "No exact perfect plan exists for the modeled route".to_string())?
        .saving;
    let mut plan_inputs = maximum_inputs.clone();
    solved.reconstruct(0, &mut plan_inputs);
    let verified = calculate_bonus(tree, &plan_inputs)?;
    if verified.bonus != maximum.bonus {
        return Err("Exact bonus plan failed arithmetic verification".into());
    }
    let maximum_total_experience = maximum_result
        .lines
        .iter()
        .filter(|line| line.key != tree.key)
        .try_fold(0_u64, |total, line| {
            cumulative_experience(line.level).map(|xp| total + xp)
        })?;
    Ok(BonusMinimumPlan {
        inputs: plan_inputs,
        bonus: verified.bonus,
        total_experience: maximum_total_experience - saving.experience,
        maximum_total_experience,
        saved_experience: saving.experience,
        material_count: maximum_result.lines.len() - 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn fixture_node(key: &str, base: i64, divisor: i64, children: Vec<BonusNode>) -> BonusNode {
        BonusNode {
            key: key.into(),
            code: key.into(),
            title: key.into(),
            alternate_name: String::new(),
            base_stats: base,
            rank: 7,
            nation: 0,
            children,
            predecessor_options: Vec::new(),
            divisor,
            white_divisor: 0,
            rule: String::new(),
            source: String::new(),
            white_enabled: false,
            manual_required: false,
            warning: String::new(),
            uses_levels: false,
            min_level: 1,
            max_level: 6,
            starting_bonus: 0,
        }
    }

    #[test]
    fn exact_planner_matches_exhaustive_search_with_multiple_floors() {
        let a = fixture_node("a", 2, 0, vec![]);
        let b = fixture_node("b", 1, 0, vec![]);
        let p = fixture_node("p", 2, 2, vec![a, b]);
        let c = fixture_node("c", 1, 0, vec![]);
        let root = fixture_node("root", 0, 13, vec![p, c]);
        let plan = calculate_minimum_perfect_plan(&root, &BTreeMap::new()).unwrap();
        let mut brute_minimum = u64::MAX;
        let mut minimum_level_sum = (i64::MAX, u64::MAX);
        for a in 1..=6 {
            for b in 1..=6 {
                for p in 1..=6 {
                    for c in 1..=6 {
                        let inputs: BTreeMap<_, _> = [("a", a), ("b", b), ("p", p), ("c", c)]
                            .into_iter()
                            .map(|(key, level)| {
                                (
                                    key.to_string(),
                                    BonusInput {
                                        level: level.to_string(),
                                        ..BonusInput::default()
                                    },
                                )
                            })
                            .collect();
                        if calculate_bonus(&root, &inputs).unwrap().bonus == plan.bonus {
                            let cost = [a, b, p, c]
                                .into_iter()
                                .map(|level| cumulative_experience(level).unwrap())
                                .sum();
                            brute_minimum = brute_minimum.min(cost);
                            minimum_level_sum = minimum_level_sum.min((a + b + p + c, cost));
                        }
                    }
                }
            }
        }
        assert_eq!(plan.total_experience, brute_minimum);
        // Minimum XP is not the same objective as minimum summed levels:
        // (4,4,5,6) costs 680 XP at 19 levels, while the cheapest 18-level
        // configuration (3,3,6,6) costs 700 XP. The planner must choose XP.
        assert_eq!(plan.total_experience, 680);
        assert_eq!(minimum_level_sum, (18, 700));
        assert!(plan.saved_experience > 0);
        assert_eq!(
            plan.total_experience + plan.saved_experience,
            plan.maximum_total_experience
        );
        assert_eq!(plan.material_count, 4);
        assert_eq!(
            calculate_bonus(&root, &plan.inputs).unwrap().bonus,
            plan.bonus
        );
    }

    #[test]
    fn exact_planner_handles_level_recipes_and_retains_selected_or_route() {
        let mut a = fixture_node("choice:a", 10, 0, vec![]);
        a.min_level = 3;
        let b = fixture_node("choice:b", 200, 0, vec![]);
        let mut conversion = fixture_node("conversion", 100, 1, vec![]);
        conversion.uses_levels = true;
        conversion.predecessor_options = vec![a, b];
        let root = fixture_node("root", 0, 5, vec![conversion]);
        let inputs = BTreeMap::from([(
            "conversion".to_string(),
            BonusInput {
                predecessor: "choice:a".into(),
                inherited: "9999".into(),
                ..BonusInput::default()
            },
        )]);
        let plan = calculate_minimum_perfect_plan(&root, &inputs).unwrap();
        assert_eq!(plan.inputs["conversion"].predecessor, "choice:a");
        assert!(plan.inputs["conversion"].inherited.is_empty());
        assert!(plan.inputs.contains_key("choice:a"));
        assert!(!plan.inputs.contains_key("choice:b"));
        assert!(plan.inputs["choice:a"].level.parse::<i64>().unwrap() >= 3);
        let mut brute_minimum = u64::MAX;
        for level in 1..=6 {
            for source_level in 3..=6 {
                let mut candidate = plan.inputs.clone();
                candidate.get_mut("conversion").unwrap().level = level.to_string();
                candidate.get_mut("choice:a").unwrap().level = source_level.to_string();
                if calculate_bonus(&root, &candidate).unwrap().bonus == plan.bonus {
                    brute_minimum = brute_minimum.min(
                        cumulative_experience(level).unwrap()
                            + cumulative_experience(source_level).unwrap(),
                    );
                }
            }
        }
        assert_eq!(plan.total_experience, brute_minimum);
    }

    #[test]
    fn minimum_xp_legend_and_wisdom_plans_verify_at_known_maxima() {
        let catalog = Catalog::open_for_mcp().unwrap();
        for (code, bonus) in [
            ("T92", 1013),
            ("V16", 1080),
            ("V27", 955),
            ("V26", 968),
            ("V28", 956),
            ("V29", 976),
        ] {
            let tree = catalog.bonus_tree(code, "en").unwrap();
            let start = Instant::now();
            let plan = calculate_minimum_perfect_plan(&tree, &BTreeMap::new()).unwrap();
            eprintln!(
                "{code}: {} XP, saved {}, {} materials, {:?}",
                plan.total_experience,
                plan.saved_experience,
                plan.material_count,
                start.elapsed()
            );
            assert_eq!(plan.bonus, bonus, "{code}");
            assert!(plan.saved_experience > 0, "{code}");
            let result = calculate_bonus(&tree, &plan.inputs).unwrap();
            assert_eq!(result.bonus, bonus, "{code}");
            assert_eq!(
                result
                    .lines
                    .iter()
                    .filter(|line| line.key != tree.key)
                    .map(|line| cumulative_experience(line.level).unwrap())
                    .sum::<u64>(),
                plan.total_experience,
                "{code}"
            );
            if tree.rank == 11 {
                for child in &tree.children {
                    assert_eq!(plan.inputs[&child.key].white, "300");
                    if child.nation == 10000 {
                        assert!(plan.inputs[&child.key].white_before_india);
                    }
                }
            }
        }
    }

    #[test]
    fn minimum_xp_plan_rejects_unverified_complete_scope() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let tree = catalog.bonus_tree("D118", "en").unwrap();
        assert!(calculate_minimum_perfect_plan(&tree, &BTreeMap::new()).is_err());
    }

    #[test]
    fn exact_planner_respects_statue_ten_level_records() {
        let mut statue = fixture_node("statue:0", 0, 0, vec![]);
        statue.rank = -1;
        statue.min_level = 100;
        statue.max_level = 200;
        let mut root = fixture_node("root", 0, 31, vec![statue]);
        root.uses_levels = true;
        let plan = calculate_minimum_perfect_plan(&root, &BTreeMap::new()).unwrap();
        assert_eq!(plan.bonus, 6);
        assert_eq!(plan.inputs["statue:0"].level, "190");
        assert_eq!(plan.total_experience, cumulative_experience(190).unwrap());
    }

    #[test]
    fn exact_planner_verifies_every_supported_catalog_target() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let started = Instant::now();
        let mut count = 0;
        let mut slowest = (String::new(), std::time::Duration::ZERO);
        for candidate in catalog.bonus_candidates("", "", "en") {
            let tree = catalog.bonus_tree(&candidate.code, "en").unwrap();
            let Ok(maximum) = calculate_max_bonus(&tree, &BTreeMap::new()) else {
                continue;
            };
            if !maximum.supports_perfect() {
                continue;
            }
            let start = Instant::now();
            let plan = calculate_minimum_perfect_plan(&tree, &BTreeMap::new())
                .unwrap_or_else(|error| panic!("{}: {error}", candidate.code));
            let elapsed = start.elapsed();
            if elapsed > slowest.1 {
                slowest = (candidate.code.clone(), elapsed);
            }
            assert_eq!(plan.bonus, maximum.bonus, "{}", candidate.code);
            assert_eq!(
                calculate_bonus(&tree, &plan.inputs).unwrap().bonus,
                maximum.bonus,
                "{}",
                candidate.code
            );
            assert!(plan.total_experience <= plan.maximum_total_experience);
            count += 1;
        }
        eprintln!(
            "Exact planner: {count} supported targets in {:?}; slowest {} {:?}",
            started.elapsed(),
            slowest.0,
            slowest.1
        );
        assert!(count >= 50);
    }
}
