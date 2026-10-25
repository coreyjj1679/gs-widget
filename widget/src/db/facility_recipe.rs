use super::text::{chrome_text, commas, lead3};
use super::Catalog;

pub(in crate::db) struct FacilityRecipeLocation {
    pub village_code: String,
    pub village_name_ko: String,
    pub town_code: String,
}

pub(in crate::db) struct FacilityRecipe {
    pub book_item: i64,
    pub duration_days: i64,
    pub influence_type: i64,
    pub influence_value: i64,
    pub locations: Vec<FacilityRecipeLocation>,
}

impl Catalog {
    pub(in crate::db) fn facility_recipe(&self, id: i64) -> Option<FacilityRecipe> {
        // Optional tables keep catalogs from before the facility import readable.
        let mut recipe = self
            .conn
            .query_row(
                "SELECT book_item, duration_days, influence_type, influence_value
             FROM recipe_facility WHERE recipe_id = ?1",
                [id],
                |row| {
                    Ok(FacilityRecipe {
                        book_item: row.get(0)?,
                        duration_days: row.get(1)?,
                        influence_type: row.get(2)?,
                        influence_value: row.get(3)?,
                        locations: Vec::new(),
                    })
                },
            )
            .ok()?;
        if let Ok(mut statement) = self.conn.prepare(
            "SELECT village_code, village_name_ko, town_code
             FROM recipe_facility_locations WHERE recipe_id = ?1 ORDER BY village_code",
        ) {
            recipe.locations = statement
                .query_map([id], |row| {
                    Ok(FacilityRecipeLocation {
                        village_code: row.get(0)?,
                        village_name_ko: row.get(1)?,
                        town_code: row.get(2)?,
                    })
                })
                .ok()?
                .filter_map(Result::ok)
                .collect();
        }
        Some(recipe)
    }

    pub(in crate::db) fn facility_influence_label(&self, kind: i64, lang: &str) -> String {
        // InfluenceStoreBuyList maps type 2 explicitly to OnlineText 14709.
        // Do not interpret an unknown faction code as a known nation's friendship.
        if kind == 2 {
            let text = |language: &str| {
                self.conn.query_row(
                "SELECT text FROM strings WHERE object='OnlineText.dat' AND id='00014709' AND lang=?1",
                [language], |row| row.get::<_, String>(0),
            ).unwrap_or_default()
            };
            return lead3(lang, &text("en"), &text("zh"), "인도 우호도");
        }
        format!(
            "{} ({kind})",
            chrome_text(lang, "友好度類型", "Friendship type")
        )
    }

    pub(in crate::db) fn facility_recipe_facts(
        &self,
        id: i64,
        lang: &str,
    ) -> Vec<(String, String)> {
        let Some(recipe) = self.facility_recipe(id) else {
            return Vec::new();
        };
        let mut facts = Vec::new();
        if recipe.book_item > 0 {
            let (name, alternate) = self.related_item_names(recipe.book_item, lang);
            let name = if alternate.is_empty() { name } else { format!("{name} · {alternate}") };
            facts.push((
                chrome_text(lang, "所需製造書", "Required recipe book"),
                format!("{name} × 1"),
            ));
        }
        facts.push((
            chrome_text(lang, "製作時間", "Production time"),
            format!(
                "{} {}",
                commas(recipe.duration_days),
                chrome_text(lang, "日", "days")
            ),
        ));
        if recipe.influence_value > 0 {
            facts.push((
                self.facility_influence_label(recipe.influence_type, lang),
                commas(recipe.influence_value),
            ));
        }
        if !recipe.locations.is_empty() {
            let places = recipe
                .locations
                .iter()
                .map(|location| {
                    if !location.town_code.is_empty() {
                        self.town_name(&location.town_code, lang)
                    } else if !location.village_name_ko.is_empty() {
                        location.village_name_ko.clone()
                    } else {
                        format!("#{}", location.village_code)
                    }
                })
                .collect::<Vec<_>>()
                .join(" · ");
            facts.push((chrome_text(lang, "製作地點", "Crafting location"), places));
        }
        facts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facility_recipe_preserves_book_time_friendship_and_locations() {
        let catalog = Catalog::open().unwrap();
        let id = catalog
            .conn
            .query_row(
                "SELECT id FROM recipes WHERE source='facility' AND result_item=4570",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        let recipe = catalog.facility_recipe(id).unwrap();
        assert_eq!((recipe.book_item, recipe.duration_days), (4656, 5));
        assert_eq!((recipe.influence_type, recipe.influence_value), (2, 750));
        assert!(recipe
            .locations
            .iter()
            .any(|place| place.village_code == "644"));
        let facts = catalog.facility_recipe_facts(id, "en");
        assert!(facts
            .iter()
            .any(|(label, value)| label == "Production time" && value == "5 days"));
        assert!(facts
            .iter()
            .any(|(label, value)| label == "India Friendship" && value == "750"));
        assert!(facts
            .iter()
            .any(|(label, value)| label == "Required recipe book" && value.ends_with(" × 1")));
        assert!(catalog.facility_recipe(742).is_none());
        assert!(catalog.facility_recipe_facts(742, "en").is_empty());
        assert!(catalog.facility_influence_label(999, "en").contains("999"));
    }
}
