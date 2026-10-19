//! Batched item and defeat-target references for quest list and roadmap icons.
use std::collections::{HashMap, HashSet};

use super::rows::{ItemRow, MonsterRow, QuestStep};
use super::text::{chrome_text, commas_text, kind_name, lead, step_hint, step_value, trail};
use super::Catalog;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestItemRef {
    pub key: String,
    pub primary: String,
    pub alternate: String,
    /// A formatted count without the multiplication sign; empty means unrecorded.
    pub quantity: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestItemSet {
    /// Verified required items and defeat targets, in recorded step order.
    pub required: Vec<QuestItemRef>,
    pub rewards: Vec<QuestItemRef>,
    /// Other objectives and unresolved target references remain visible as text.
    pub other_objectives: String,
}

fn item_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "item" | "book" | "wearitem" | "useitem"
    )
}

fn quantity_text(quantity: &str) -> String {
    let quantity = quantity.trim();
    if quantity.is_empty() || quantity == "0" {
        String::new()
    } else {
        commas_text(quantity)
    }
}

fn target_text(name: &str, quantity: &str) -> String {
    let quantity = quantity_text(quantity);
    if quantity.is_empty() {
        name.to_string()
    } else {
        format!("× {quantity} {name}")
    }
}

fn bilingual_name(lang: &str, en: &str, zh: &str) -> String {
    let primary = lead(lang, en, zh);
    let alternate = trail(lang, en, zh);
    if alternate.is_empty() {
        primary
    } else {
        format!("{primary} / {alternate}")
    }
}

fn fallback_name(step: &QuestStep, lang: &str) -> String {
    let hint = step_hint(&step.label);
    if !hint.is_empty() {
        hint
    } else if !step.code.trim().is_empty() && step.code.trim() != "0" {
        format!("#{}", step.code.trim())
    } else {
        chrome_text(lang, "道具未註明", "Item not recorded")
    }
}

fn item_reference(step: &QuestStep, item: &ItemRow, code: i64, lang: &str) -> QuestItemRef {
    let primary = lead(lang, &item.en, &item.zh);
    QuestItemRef {
        key: format!("item:{code}"),
        primary: if primary.is_empty() {
            fallback_name(step, lang)
        } else {
            primary
        },
        alternate: trail(lang, &item.en, &item.zh),
        quantity: quantity_text(&step.qty),
    }
}

fn monster_fallback(step: &QuestStep, lang: &str) -> String {
    let hint = step_hint(&step.label);
    if !hint.is_empty() {
        hint
    } else if !step.code.trim().is_empty() && step.code.trim() != "0" {
        format!("#{}", step.code.trim())
    } else {
        chrome_text(lang, "怪物未註明", "Monster not recorded")
    }
}

fn monster_reference(step: &QuestStep, monster: &MonsterRow, lang: &str) -> QuestItemRef {
    let primary = lead(lang, &monster.en, &monster.zh);
    QuestItemRef {
        key: format!("monster:{}", monster.code),
        primary: if primary.is_empty() {
            monster_fallback(step, lang)
        } else {
            primary
        },
        alternate: trail(lang, &monster.en, &monster.zh),
        quantity: quantity_text(&step.qty),
    }
}

fn monster_group_name(
    step: &QuestStep,
    monsters: &HashMap<&str, &MonsterRow>,
    lang: &str,
) -> String {
    if !step.code.contains(',') {
        return monster_fallback(step, lang);
    }
    let names: Vec<_> = step
        .code
        .split(',')
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(|code| {
            monsters
                .get(code)
                .map(|monster| bilingual_name(lang, &monster.en, &monster.zh))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("#{code}"))
        })
        .collect();
    if names.is_empty() {
        monster_fallback(step, lang)
    } else {
        // One bracketed group and one quantity preserve the recorded relation
        // without claiming that every variant needs its own separate count.
        format!("[{}]", names.join("; "))
    }
}

impl Catalog {
    /// One batched steps read and cached names; safe to call for the entire list.
    pub fn quest_item_previews(&self, ids: &[i64], lang: &str) -> HashMap<i64, QuestItemSet> {
        let wanted: HashSet<_> = ids.iter().copied().collect();
        let mut sets: HashMap<_, _> = self
            .index
            .quests
            .iter()
            .filter(|quest| wanted.contains(&quest.id))
            .map(|quest| (quest.id, QuestItemSet::default()))
            .collect();
        if sets.is_empty() {
            return sets;
        }
        let items: HashMap<_, _> = self
            .index
            .items
            .iter()
            .filter_map(|item| {
                item.code_str
                    .parse::<i64>()
                    .ok()
                    .filter(|code| *code > 0)
                    .map(|code| (code, item))
            })
            .collect();
        let monsters: HashMap<_, _> = self
            .index
            .monsters
            .iter()
            .filter(|monster| !monster.code.is_empty() && monster.code != "0")
            .map(|monster| (monster.code.as_str(), monster))
            .collect();
        let npcs: HashMap<_, _> = self
            .index
            .npcs
            .iter()
            .map(|npc| (npc.code, bilingual_name(lang, &npc.en, &npc.zh)))
            .collect();
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT quest_id, role, kind, code, qty, label FROM quest_steps
             WHERE lower(role) IN ('objective', 'requirement', 'reward') ORDER BY quest_id, slot",
        ) else {
            return sets;
        };
        let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                QuestStep {
                    role: row.get(1)?,
                    kind: row.get(2)?,
                    code: row.get(3)?,
                    qty: row.get(4)?,
                    label: row.get(5)?,
                },
            ))
        }) else {
            return sets;
        };
        let mut other: HashMap<i64, Vec<String>> = HashMap::new();
        for (id, step) in rows.filter_map(Result::ok) {
            let Some(set) = sets.get_mut(&id) else {
                continue;
            };
            let reward = step.role.eq_ignore_ascii_case("reward");
            if item_kind(&step.kind) {
                let known = step
                    .code
                    .trim()
                    .parse::<i64>()
                    .ok()
                    .filter(|code| *code > 0)
                    .and_then(|code| items.get(&code).map(|item| (code, *item)));
                if let Some((code, item)) = known {
                    let item = item_reference(&step, item, code, lang);
                    // Keep distinct occurrences/roles/quantities instead of merging requirements.
                    if reward {
                        set.rewards.push(item);
                    } else {
                        set.required.push(item);
                    }
                } else if !reward {
                    other.entry(id).or_default().push(format!(
                        "{}: {}",
                        kind_name(&step.kind, lang),
                        target_text(&fallback_name(&step, lang), &step.qty),
                    ));
                }
                continue;
            }
            if !step.role.eq_ignore_ascii_case("objective") {
                continue;
            }
            let kind = step.kind.to_ascii_lowercase();
            if kind.is_empty() || kind == "0" {
                continue;
            }
            let line = match kind.as_str() {
                "monster" => {
                    if let Some(monster) = monsters.get(step.code.trim()) {
                        set.required.push(monster_reference(&step, monster, lang));
                        continue;
                    }
                    format!(
                        "{}: {}",
                        chrome_text(lang, "討伐", "Defeat"),
                        target_text(&monster_group_name(&step, &monsters, lang), &step.qty)
                    )
                }
                "npc" => {
                    let name = step
                        .code
                        .parse::<i64>()
                        .ok()
                        .and_then(|code| npcs.get(&code))
                        .filter(|name| !name.is_empty())
                        .cloned()
                        .unwrap_or_else(|| {
                            let hint = step_hint(&step.label);
                            if hint.is_empty() {
                                step.code.clone()
                            } else {
                                hint
                            }
                        });
                    format!("{}: {name}", chrome_text(lang, "對話", "Talk to"))
                }
                _ => {
                    let parts = [step_hint(&step.label), step_value(&step.code, &step.qty)]
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>();
                    if parts.is_empty() {
                        continue;
                    }
                    format!("{}: {}", kind_name(&step.kind, lang), parts.join(" · "))
                }
            };
            other.entry(id).or_default().push(line);
        }
        for (id, lines) in other {
            if let Some(set) = sets.get_mut(&id) {
                // The chip tooltip needs complete grouped names; the UI can elide its inline text.
                set.other_objectives = lines.join(" · ");
            }
        }
        sets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_icons_resolve_required_items_and_multiple_rewards_in_both_languages() {
        let catalog = Catalog::open().expect("db");
        let id = |code: &str| {
            catalog
                .index
                .quests
                .iter()
                .find(|quest| quest.source == "main" && quest.code == code)
                .unwrap()
                .id
        };
        for lang in ["en-zh", "zh-en"] {
            let sets = catalog.quest_item_previews(&[id("50001"), id("53020")], lang);
            let first = &sets[&id("50001")];
            assert_eq!(first.required.len(), 1);
            assert_eq!(first.required[0].key, "item:140");
            assert_eq!(first.required[0].quantity, "1");
            assert_eq!(
                first
                    .rewards
                    .iter()
                    .map(|item| (item.key.as_str(), item.quantity.as_str()))
                    .collect::<Vec<_>>(),
                vec![("item:8762", "1"), ("item:11385", "2")]
            );
            assert!(first.other_objectives.is_empty());
            let elder = &sets[&id("53020")].required[0];
            assert_eq!(elder.key, "item:2565");
            let item = catalog
                .index
                .items
                .iter()
                .find(|item| item.code_str == "2565")
                .unwrap();
            assert_eq!(elder.primary, lead(lang, &item.en, &item.zh));
            assert_eq!(elder.alternate, trail(lang, &item.en, &item.zh));
            assert!(!elder.primary.is_empty());
            assert!(!elder.alternate.is_empty());
        }
    }

    #[test]
    fn mixed_quests_keep_combat_and_invalid_item_references_remain_text() {
        let catalog = Catalog::open().expect("db");
        let id = |code: &str| {
            catalog
                .index
                .quests
                .iter()
                .find(|quest| quest.source == "main" && quest.code == code)
                .unwrap()
                .id
        };
        let sets = catalog.quest_item_previews(
            &[id("71700"), id("71914"), id("71921"), id("50008")],
            "en-zh",
        );
        let mixed = &sets[&id("71700")];
        assert_eq!(mixed.required.len(), 2);
        assert_eq!(mixed.required[0].key, "monster:E152");
        assert_eq!(mixed.required[0].quantity, "150");
        assert_eq!(mixed.required[1].key, "item:4507");
        assert_eq!(mixed.required[1].quantity, "3");
        assert!(mixed.other_objectives.is_empty());
        let pond = &sets[&id("71914")];
        assert_eq!(
            pond.required
                .iter()
                .map(|target| (target.key.as_str(), target.quantity.as_str()))
                .collect::<Vec<_>>(),
            vec![("monster:H59", "50"), ("item:6472", "3")]
        );
        assert!(pond.other_objectives.is_empty());
        let monster = catalog
            .index
            .monsters
            .iter()
            .find(|monster| monster.code == "H59")
            .unwrap();
        for lang in ["en-zh", "zh-en"] {
            let localized = catalog.quest_item_previews(&[id("71914")], lang);
            let target = &localized[&id("71914")].required[0];
            assert_eq!(target.primary, lead(lang, &monster.en, &monster.zh));
            assert_eq!(target.alternate, trail(lang, &monster.en, &monster.zh));
            assert!(!target.primary.is_empty());
            assert!(!target.alternate.is_empty());
        }
        // Comma-separated variants share a recorded quantity. Do not turn that
        // into a separate per-monster count without knowing its semantics.
        let variants = &sets[&id("71921")];
        assert_eq!(variants.required.len(), 1);
        assert_eq!(variants.required[0].key, "item:6846");
        assert!(variants.other_objectives.starts_with("Defeat: × 10 ["));
        assert_eq!(variants.other_objectives.matches("× 10").count(), 2);
        for code in ["H111", "H131"] {
            let monster = catalog
                .index
                .monsters
                .iter()
                .find(|monster| monster.code == code)
                .unwrap();
            assert!(variants.other_objectives.contains(&bilingual_name(
                "en-zh",
                &monster.en,
                &monster.zh
            )));
        }
        for code in ["H150", "H134", "H117", "H149", "H132"] {
            assert!(variants.other_objectives.contains(&format!("#{code}")));
        }
        assert!(!variants.other_objectives.ends_with('…'));
        let unresolved = &sets[&id("50008")];
        assert!(unresolved.required.is_empty());
        assert!(unresolved.other_objectives.contains("× 5"));
        assert!(unresolved.other_objectives.contains("Item not recorded"));
    }

    #[test]
    fn item_sets_include_empty_quests_but_never_missing_ids_or_fake_items() {
        let catalog = Catalog::open().expect("db");
        let mut ids: Vec<_> = catalog.index.quests.iter().map(|quest| quest.id).collect();
        ids.extend([ids[0], i64::MIN]);
        let sets = catalog.quest_item_previews(&ids, "zh-en");
        assert_eq!(sets.len(), catalog.index.quests.len());
        assert!(!sets.contains_key(&i64::MIN));
        let empty = catalog
            .index
            .quests
            .iter()
            .find(|quest| quest.source == "main" && quest.code == "71975")
            .unwrap()
            .id;
        assert_eq!(sets[&empty], QuestItemSet::default());
        for set in sets.values() {
            for target in set.required.iter().chain(&set.rewards) {
                if let Some(code) = target.key.strip_prefix("item:") {
                    assert!(code.parse::<i64>().unwrap() > 0);
                    assert!(catalog
                        .index
                        .items
                        .iter()
                        .any(|known| known.code_str == code));
                } else {
                    let code = target.key.strip_prefix("monster:").unwrap();
                    assert!(catalog
                        .index
                        .monsters
                        .iter()
                        .any(|known| known.code == code));
                }
                assert!(!target.primary.is_empty());
            }
            assert!(set
                .rewards
                .iter()
                .all(|reward| reward.key.starts_with("item:")));
        }
        assert!(catalog.quest_item_previews(&[], "en-zh").is_empty());
        assert!(catalog.quest_item_previews(&[i64::MIN], "en-zh").is_empty());
    }

    #[test]
    fn repeated_items_keep_distinct_roles_and_quantities() {
        let mut catalog = Catalog::open().expect("db");
        let id = catalog.index.quests[0].id;
        catalog.conn = rusqlite::Connection::open_in_memory().unwrap();
        catalog.conn.execute_batch(
            "CREATE TABLE quest_steps (quest_id INTEGER, slot INTEGER, role TEXT, kind TEXT, code TEXT, qty TEXT, label TEXT);",
        ).unwrap();
        for (slot, role, kind, code, qty, label) in [
            (0, "objective", "Item", "140", "1", ""),
            (1, "requirement", "Item", "140", "1", ""),
            (2, "objective", "UseItem", "140", "2", ""),
            (3, "reward", "Item", "140", "3", ""),
            (4, "objective", "Item", "999999999", "4", "Unresolved item"),
            (
                5,
                "objective",
                "Monster",
                "MissingMonster",
                "7",
                "Lost monster",
            ),
        ] {
            catalog
                .conn
                .execute(
                    "INSERT INTO quest_steps VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![id, slot, role, kind, code, qty, label],
                )
                .unwrap();
        }
        let sets = catalog.quest_item_previews(&[id], "en-zh");
        let set = &sets[&id];
        assert_eq!(
            set.required
                .iter()
                .map(|item| item.quantity.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "1", "2"]
        );
        assert!(set.required.iter().all(|item| item.key == "item:140"));
        assert_eq!(set.rewards.len(), 1);
        assert_eq!(set.rewards[0].quantity, "3");
        assert_eq!(
            set.other_objectives,
            "Item: × 4 Unresolved item · Defeat: × 7 Lost monster"
        );
        assert!(!set
            .required
            .iter()
            .any(|target| target.key == "monster:MissingMonster"));
    }

    #[test]
    fn grouped_monster_names_keep_one_quantity_and_unknown_codes() {
        let catalog = Catalog::open().expect("db");
        let monster = catalog
            .index
            .monsters
            .iter()
            .find(|monster| monster.code == "M25")
            .unwrap();
        let monsters = HashMap::from([("M25", monster)]);
        let step = QuestStep {
            role: "objective".into(),
            kind: "Monster".into(),
            code: "M25,MissingMonster".into(),
            qty: "12".into(),
            label: "group label".into(),
        };
        for lang in ["en-zh", "zh-en"] {
            let text = target_text(&monster_group_name(&step, &monsters, lang), &step.qty);
            assert_eq!(
                text,
                format!(
                    "× 12 [{}; #MissingMonster]",
                    bilingual_name(lang, &monster.en, &monster.zh)
                )
            );
            assert_eq!(text.matches("× 12").count(), 1);
        }
    }
}
