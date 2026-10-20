//! Bounded, data-only calculation trees. Child snapshots never refer to mutable
//! library IDs, and child totals are calculated for the parent's actual demand.
use super::*;

const INVALID_PATH: &str = "找不到材料 / Material path not found";

impl CalcDocument {
    pub fn calculate(&self) -> Result<CalcBreakdown, String> {
        self.calculate_for(self.quantity, 0, &mut BTreeSet::new(), &mut 0)
    }

    pub fn ingredient_at(&self, path: &[usize]) -> Option<&CalcIngredient> {
        if path.is_empty() || path.len() > MAX_NESTED_DEPTH + 1 {
            return None;
        }
        let (index, rest) = path.split_first()?;
        let material = self.materials.get(*index)?;
        if rest.is_empty() {
            Some(material)
        } else {
            material.nested.as_deref()?.ingredient_at(rest)
        }
    }

    pub fn ingredient_at_mut(&mut self, path: &[usize]) -> Option<&mut CalcIngredient> {
        if path.is_empty() || path.len() > MAX_NESTED_DEPTH + 1 {
            return None;
        }
        let (index, rest) = path.split_first()?;
        let material = self.materials.get_mut(*index)?;
        if rest.is_empty() {
            Some(material)
        } else {
            material.nested.as_deref_mut()?.ingredient_at_mut(rest)
        }
    }

    /// Replace this ingredient's source atomically. None restores its retained
    /// direct price; Some embeds an independent, portable recipe snapshot.
    pub fn set_nested(
        &mut self,
        path: &[usize],
        nested: Option<CalcDocument>,
    ) -> Result<(), String> {
        // Validate before cloning so mutations only copy a bounded tree.
        self.calculate()?;
        if path.is_empty() || path.len() > MAX_NESTED_DEPTH + 1 {
            return Err(INVALID_PATH.into());
        }
        let mut next = self.clone();
        let quantity = next.quantity;
        next.replace_nested(path, nested, quantity)?;
        next.calculate()?;
        *self = next;
        Ok(())
    }

    fn replace_nested(
        &mut self,
        path: &[usize],
        nested: Option<CalcDocument>,
        required: i64,
    ) -> Result<(), String> {
        let (&index, rest) = path.split_first().ok_or(INVALID_PATH)?;
        let batches = required / self.output + i64::from(required % self.output != 0);
        let material = self.materials.get_mut(index).ok_or(INVALID_PATH)?;
        let quantity = material.per_batch.checked_mul(batches).ok_or(OVERFLOW)?;
        let adding = nested.is_some();
        if rest.is_empty() {
            material.nested = nested.map(|mut child| {
                child.quantity = quantity;
                Box::new(child)
            });
        } else {
            material
                .nested
                .as_deref_mut()
                .ok_or(INVALID_PATH)?
                .replace_nested(rest, nested, quantity)?;
        }
        if adding {
            // Every ancestor advertises support for its nested material tree.
            self.version = NESTED_VERSION;
        }
        Ok(())
    }

    pub(in crate::db) fn calculate_for(
        &self,
        required: i64,
        depth: usize,
        ancestors: &mut BTreeSet<i64>,
        material_count: &mut usize,
    ) -> Result<CalcBreakdown, String> {
        if depth > MAX_NESTED_DEPTH {
            return Err("材料最多展開 8 層 / Calculations support at most 8 nested levels".into());
        }
        if !matches!(self.version, VERSION | NESTED_VERSION) {
            return Err(format!(
                "不支援的計算版本 {} / Unsupported calculation version",
                self.version
            ));
        }
        if self.version == VERSION && self.materials.iter().any(|row| row.nested.is_some()) {
            return Err(
                "舊版計算不可包含子配方 / Legacy calculations cannot contain nested recipes".into(),
            );
        }
        if !valid_text(&self.catalog_version, 64, true)
            || !valid_text(&self.language, 32, false)
            || !valid_text(&self.title, 1024, false)
            || !valid_text(&self.source, 256, false)
            || self.recipe_id <= 0
            || self.item_code <= 0
            || self.quantity <= 0
            || required <= 0
            || self.output <= 0
            || self.fee_per_batch < 0
            || self.learn_fee.is_some_and(|fee| fee < 0)
            || self.success.is_some_and(|rate| !(1..=100).contains(&rate))
            || self.materials.is_empty()
            || self.materials.len() > MAX_MATERIALS
        {
            return Err("配方資料或數量無效 / Invalid recipe data or quantity".into());
        }
        *material_count = material_count
            .checked_add(self.materials.len())
            .ok_or(OVERFLOW)?;
        if *material_count > MAX_TOTAL_MATERIALS {
            return Err(
                "展開後最多 512 列材料 / Calculations support at most 512 material rows".into(),
            );
        }
        if !ancestors.insert(self.item_code) {
            return Err("子配方循環引用同一道具 / Nested recipes contain an item cycle".into());
        }
        let batches = required / self.output + i64::from(required % self.output != 0);
        let output_units = self.output.checked_mul(batches).ok_or(OVERFLOW)?;
        let fee = self.fee_per_batch.checked_mul(batches).ok_or(OVERFLOW)?;
        let mut keys = BTreeSet::new();
        let mut material_total = 0_i64;
        let mut priced = 0;
        let mut leaf_count = 0;
        let mut priced_leaves = 0;
        let mut materials = Vec::with_capacity(self.materials.len());
        for material in &self.materials {
            let key = ItemKey {
                code: material.code,
                enhance: material.enhance,
            };
            if key.code <= 0
                || key.enhance < 0
                || !keys.insert(key)
                || material.per_batch <= 0
                || !valid_text(&material.title, 1024, false)
                || material.unit_price.is_some_and(|price| price < 0)
            {
                return Err("材料資料重複或無效 / Duplicate or invalid material data".into());
            }
            let quantity = material.per_batch.checked_mul(batches).ok_or(OVERFLOW)?;
            let (line_total, known_total, unit_price, nested) = if let Some(child) =
                &material.nested
            {
                if material.enhance != 0 || child.item_code != material.code {
                    return Err(
                        "子配方產出與材料不符 / Nested recipe output does not match the material"
                            .into(),
                    );
                }
                let cost = child.calculate_for(quantity, depth + 1, ancestors, material_count)?;
                leaf_count += cost.leaf_count;
                priced_leaves += cost.priced_leaves;
                (cost.total, cost.known_total, None, Some(Box::new(cost)))
            } else {
                let total = material
                    .unit_price
                    .map(|price| price.checked_mul(quantity).ok_or(OVERFLOW))
                    .transpose()?;
                leaf_count += 1;
                priced_leaves += usize::from(total.is_some());
                (total, total.unwrap_or(0), material.unit_price, None)
            };
            priced += usize::from(line_total.is_some());
            material_total = material_total.checked_add(known_total).ok_or(OVERFLOW)?;
            materials.push(CalcLine {
                key,
                title: material.title.clone(),
                quantity,
                unit_price,
                line_total,
                known_total,
                nested,
            });
        }
        ancestors.remove(&self.item_code);
        let known_total = material_total.checked_add(fee).ok_or(OVERFLOW)?;
        Ok(CalcBreakdown {
            batches,
            output_units,
            fee,
            material_total,
            known_total,
            total: (priced_leaves == leaf_count).then_some(known_total),
            priced,
            leaf_count,
            priced_leaves,
            materials,
        })
    }
}
