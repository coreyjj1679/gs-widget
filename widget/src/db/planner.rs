//! Read-only, complete recipe records used by the crafting cost calculator.
use std::collections::{BTreeMap, BTreeSet};

use super::text::label;
use super::Catalog;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ItemKey {
    pub code: i64,
    pub enhance: i64,
}

#[derive(Clone)]
pub struct PlannerCandidate {
    pub recipe_id: i64,
    pub code: i64,
    pub title: String,
    pub source: String,
    pub output: i64,
    pub fee: i64,
    pub success: Option<i64>,
}

struct Recipe {
    id: i64,
    source: String,
    result: i64,
    output: i64,
    fee: i64,
    learn_fee: Option<i64>,
    success: Option<i64>,
    materials: BTreeMap<ItemKey, i64>,
}

pub struct PlannerCatalog {
    recipes: BTreeMap<i64, Recipe>,
}

fn add(map: &mut BTreeMap<ItemKey, i64>, key: ItemKey, quantity: i64) -> Result<(), String> {
    let current = map.entry(key).or_default();
    *current = current
        .checked_add(quantity)
        .ok_or("數量過大 / Quantity exceeds the supported range")?;
    Ok(())
}

impl PlannerCatalog {
    fn load(conn: &rusqlite::Connection) -> Result<Self, String> {
        let mut recipes = BTreeMap::new();
        let mut invalid = BTreeSet::new();
        let mut stmt = conn
            .prepare(
                "SELECT r.id, r.source, r.result_item, r.result_qty, r.fee, r.learn_fee,
                    r.success_rate, i.code
             FROM recipes r LEFT JOIN items i ON i.code = r.result_item ORDER BY r.id",
            )
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    Recipe {
                        id: row.get(0)?,
                        source: row.get(1)?,
                        result: row.get(2)?,
                        output: row.get(3)?,
                        fee: row.get(4)?,
                        learn_fee: row.get(5)?,
                        success: row.get(6)?,
                        materials: BTreeMap::new(),
                    },
                    row.get::<_, Option<i64>>(7)?,
                ))
            })
            .map_err(|err| err.to_string())?;
        for row in rows {
            let (recipe, item) = row.map_err(|err| err.to_string())?;
            if recipe.id <= 0
                || recipe.result <= 0
                || item.is_none()
                || recipe.output <= 0
                || recipe.fee < 0
                || recipe.learn_fee.is_some_and(|fee| fee < 0)
                || recipe
                    .success
                    .is_some_and(|rate| !(1..=100).contains(&rate))
            {
                invalid.insert(recipe.id);
            }
            recipes.insert(recipe.id, recipe);
        }
        let mut stmt = conn
            .prepare(
                "SELECT m.recipe_id, m.item_code, m.enhance, m.qty, i.code
             FROM recipe_materials m LEFT JOIN items i ON i.code = m.item_code
             ORDER BY m.recipe_id, m.slot",
            )
            .map_err(|err| err.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    ItemKey {
                        code: row.get(1)?,
                        enhance: row.get(2)?,
                    },
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })
            .map_err(|err| err.to_string())?;
        for row in rows {
            let (id, key, quantity, item) = row.map_err(|err| err.to_string())?;
            if let Some(recipe) = recipes.get_mut(&id) {
                if key.code <= 0
                    || item.is_none()
                    || quantity <= 0
                    || key.enhance < 0
                    || add(&mut recipe.materials, key, quantity).is_err()
                {
                    invalid.insert(id);
                }
            }
        }
        recipes.retain(|id, recipe| !invalid.contains(id) && !recipe.materials.is_empty());
        Ok(Self { recipes })
    }
}

impl Catalog {
    pub fn planner_catalog(&self) -> Result<PlannerCatalog, String> {
        PlannerCatalog::load(&self.conn)
    }

    pub fn planner_recipe(
        &self,
        book: &PlannerCatalog,
        id: i64,
        lang: &str,
    ) -> Option<PlannerCandidate> {
        let recipe = book.recipes.get(&id)?;
        Some(PlannerCandidate {
            recipe_id: id,
            code: recipe.result,
            title: self.item_name(recipe.result, lang),
            source: label(&recipe.source, lang),
            output: recipe.output,
            fee: recipe.fee,
            success: recipe.success,
        })
    }
}

#[cfg(test)]
mod tests;
