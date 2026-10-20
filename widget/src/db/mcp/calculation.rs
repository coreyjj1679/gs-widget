use super::*;

impl Catalog {
    pub fn mcp_query_calculate(
        &self,
        book: &CalcCatalog,
        key: &str,
        recipe_id: Option<i64>,
        quantity: i64,
        prices: &[(i64, i64, i64)],
        lang: &str,
        limit: usize,
    ) -> Result<Value, String> {
        let (kind, id) = key.split_once(':').ok_or("Invalid key")?;
        let ids: Vec<_> = self
            .index
            .recipes
            .iter()
            .filter(|row| match kind {
                "item" => row.result_str == id,
                "recipe" => Some(row.id) == id.parse().ok(),
                _ => false,
            })
            .collect();
        if !matches!(kind, "item" | "recipe") {
            return Ok(
                json!({"status": "unsupported_kind", "note": "Quantity calculation supports item crafting recipes. Hero acquisition lists direct requirements without calculating a recursive total."}),
            );
        }
        if let Some(chosen) = recipe_id {
            if !ids.iter().any(|row| row.id == chosen) {
                return Err(
                    "recipe_id does not produce the selected item or match the selected recipe"
                        .into(),
                );
            }
        }
        if ids.is_empty() {
            return Ok(
                json!({"status": "not_recorded", "note": "No crafting recipe is recorded; this does not mean the item is free or unobtainable."}),
            );
        }
        if ids.len() > 1 && recipe_id.is_none() {
            let prices: Vec<_> = prices.iter().map(|&(item_code, enhance, unit_price)| json!({"item_code": item_code,"enhance": enhance,"unit_price": unit_price})).collect();
            return Ok(json!({"status": "choose_recipe", "total": ids.len(),
                "candidates": ids.iter().take(limit).map(|row| json!({"key": format!("recipe:{}", row.id), "recipe_id": row.id, "source": row.source,
                    "next_call": {"tool": "query_catalog", "requires_choice": true,
                        "arguments": {"keys": [key], "action": "calculate", "recipe_id": row.id, "quantity": quantity, "prices": prices, "language": language_code(lang)}}
                })).collect::<Vec<_>>(),
                "truncated": ids.len() > limit,
                "next_call": if ids.len() > limit { json!({"tool": "get_recipes", "arguments": {"item_code": ids[0].result_str.parse::<i64>().unwrap_or(0), "offset": limit, "limit": limit, "language": language_code(lang)}}) } else { Value::Null },
                "note": "Choose a recorded route; no recipe is selected by guessing."}));
        }
        let id = recipe_id.unwrap_or(ids[0].id);
        // Validate support before treating a missing material/fee as a zero-cost recipe.
        if let Err(reason) = self.calculator_document(book, id, lang) {
            return Ok(json!({"status": "unavailable", "recipe_id": id, "reason": reason}));
        }
        let mut result = self.mcp_calculate(book, id, quantity, prices, lang)?;
        result["status"] = json!("calculated");
        Ok(result)
    }
}
