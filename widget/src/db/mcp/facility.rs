//! Preserve facility-specific requirements alongside the standard recipe payload.
use super::*;

impl Catalog {
    pub(super) fn mcp_attach_facility(&self, row: &mut Value, recipe_id: i64, lang: &str) {
        let Some(facility) = self.facility_recipe(recipe_id) else {
            return;
        };
        let mut book = self.mcp_item_ref(facility.book_item, lang);
        book["quantity_per_batch"] = json!(1);
        let locations: Vec<_> = facility
            .locations
            .iter()
            .map(|location| {
                json!({
                    "village_code": location.village_code,
                    "village_name_ko": location.village_name_ko,
                    "town": if location.town_code.is_empty() { None } else {
                        self.mcp_reference(&format!("town:{}", location.town_code), lang)
                    },
                })
            })
            .collect();
        row["facility"] = json!({
            "book": book,
            "duration_days": facility.duration_days,
            "influence_type": facility.influence_type,
            "influence_value": facility.influence_value,
            "influence_label": self.facility_influence_label(facility.influence_type, lang),
            "locations": locations,
            "scope": "The manufacturing book is already included once per batch in materials. Duration and influence are recorded conditions, not converted to monetary costs.",
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facility_recipe_id(catalog: &Catalog, code: &str) -> i64 {
        catalog
            .index
            .recipes
            .iter()
            .find(|recipe| recipe.source == "facility" && recipe.result_str == code)
            .expect("facility recipe")
            .id
    }

    fn assert_dragon_kukri_conditions(row: &Value) {
        let facility = &row["facility"];
        assert_eq!(facility["book"]["key"], "item:4656");
        assert_eq!(facility["book"]["quantity_per_batch"], 1);
        assert_eq!(facility["duration_days"], 5);
        assert_eq!(facility["influence_type"], 2);
        assert_eq!(facility["influence_value"], 750);
        assert!(!facility["locations"].as_array().unwrap().is_empty());
        for location in facility["locations"].as_array().unwrap() {
            assert!(!location["village_code"].as_str().unwrap().is_empty());
            assert!(!location["village_name_ko"].as_str().unwrap().is_empty());
            // Raw village IDs are not town IDs. Only expose a town reference
            // when the catalog has an explicit resolved mapping.
            if !location["town"].is_null() {
                assert!(location["town"]["key"]
                    .as_str()
                    .unwrap()
                    .starts_with("town:"));
            }
        }
    }

    #[test]
    fn facility_conditions_survive_recipe_lookup_entity_and_acquisition() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let book = catalog.calculator_catalog().unwrap();
        let recipe_id = facility_recipe_id(&catalog, "4570");
        let recipes = catalog.mcp_recipes(&book, 4570, "en", 0, 50).unwrap();
        let recipe = recipes["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["recipe_id"] == recipe_id)
            .unwrap();
        assert_eq!(recipe["calculable"], true);
        assert_dragon_kukri_conditions(recipe);
        assert_eq!(recipe["recipe"]["fee_per_batch"], 100_000_000);
        assert_eq!(
            recipe["recipe"]["materials"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|material| material["code"] == 4656)
                .count(),
            1
        );

        let overview = catalog
            .mcp_entity_sections(&format!("recipe:{recipe_id}"), "en", &["overview".into()])
            .unwrap();
        assert_dragon_kukri_conditions(&overview);
        let acquisition = catalog
            .mcp_query_scoped(
                "",
                &["item:4570".into()],
                Some("item"),
                "acquisition",
                "en",
                50,
                &["recipes".into()],
                &[],
                0,
            )
            .unwrap();
        let recipes = acquisition["results"][0]["acquisition"]["recipes"]["results"]
            .as_array()
            .unwrap();
        assert_dragon_kukri_conditions(
            recipes
                .iter()
                .find(|row| row["recipe_id"] == recipe_id)
                .unwrap(),
        );

        let relations = catalog
            .mcp_relations("item:4570", "recipes", "en", 0, 50)
            .unwrap();
        assert_dragon_kukri_conditions(
            relations["results"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["recipe_id"] == recipe_id)
                .unwrap(),
        );
        let uses = catalog
            .mcp_relations("item:4656", "uses", "en", 0, 50)
            .unwrap();
        let usage = uses["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["recipe_id"] == recipe_id)
            .unwrap();
        assert_eq!(usage["output_key"], "item:4570");
        assert_dragon_kukri_conditions(usage);
    }

    #[test]
    fn facility_calculation_keeps_conditions_and_counts_books_per_batch() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let book = catalog.calculator_catalog().unwrap();
        let recipe_id = facility_recipe_id(&catalog, "4570");
        let result = catalog
            .mcp_query_calculate(
                &book,
                "item:4570",
                Some(recipe_id),
                3,
                &[(4656, 0, 1_000)],
                "en",
                50,
            )
            .unwrap();
        assert_dragon_kukri_conditions(&result);
        assert_eq!(result["status"], "calculated");
        assert_eq!(result["crafting_fee"], 300_000_000);
        let materials = result["materials"].as_array().unwrap();
        let book = materials
            .iter()
            .find(|material| material["item_code"] == 4656)
            .unwrap();
        assert_eq!(book["quantity"], 3);
        assert_eq!(book["line_total"], 3_000);
        assert_eq!(result["known_total"], 300_003_000);
        assert!(result["total"].is_null());
    }

    #[test]
    fn manufacturing_books_map_to_output_recipes_in_both_directions() {
        let catalog = Catalog::open_for_mcp().unwrap();
        let book = catalog.calculator_catalog().unwrap();
        for (book_code, output_code) in [(5896, 5625), (5897, 5626)] {
            let recipes = catalog
                .mcp_recipes(&book, output_code, "en", 0, 50)
                .unwrap();
            let recipes = recipes["results"].as_array().unwrap();
            let uses = catalog
                .mcp_relations(&format!("item:{book_code}"), "uses", "en", 0, 50)
                .unwrap();
            let uses = uses["results"].as_array().unwrap();
            for source in ["baekje", "unified"] {
                let recipe = recipes.iter().find(|row| row["source"] == source).unwrap();
                assert_eq!(recipe["calculable"], true);
                let materials = recipe["recipe"]["materials"].as_array().unwrap();
                let books: Vec<_> = materials
                    .iter()
                    .filter(|row| row["code"] == book_code)
                    .collect();
                assert_eq!(books.len(), 1);
                assert_eq!(books[0]["per_batch"], 1);
                let usage = uses
                    .iter()
                    .find(|row| row["recipe_id"] == recipe["recipe_id"])
                    .unwrap();
                assert_eq!(usage["output_key"], format!("item:{output_code}"));
                assert_eq!(usage["requirements"][0]["quantity"], 1);
            }
        }
    }
}
