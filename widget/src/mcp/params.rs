use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Zh,
    En,
}
impl Language {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Zh => "zh-en",
            Self::En => "en-zh",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Item,
    Monster,
    Recipe,
    Quest,
    Hero,
    Npc,
    Set,
}
impl Category {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Monster => "monster",
            Self::Recipe => "recipe",
            Self::Quest => "quest",
            Self::Hero => "hero",
            Self::Npc => "npc",
            Self::Set => "set",
        }
    }
}

fn default_limit() -> usize {
    20
}
fn default_quantity() -> i64 {
    1
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Search {
    pub category: Category,
    /// Chinese/English/Korean name, alias or item/monster code. Empty browses the category.
    #[serde(default)]
    #[schemars(length(max = 200))]
    pub query: String,
    #[serde(default)]
    pub language: Language,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 50))]
    pub limit: usize,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    /// Exact key returned by search_catalog, for example monster:M24 or item:6213.
    #[schemars(length(min = 1, max = 100))]
    pub key: String,
    #[serde(default)]
    pub language: Language,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Item {
    #[schemars(range(min = 1))]
    pub item_code: i64,
    #[serde(default)]
    pub language: Language,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 50))]
    pub limit: usize,
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    #[default]
    Prerequisites,
    Unlocks,
    Chain,
}
impl Direction {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Prerequisites => "prerequisites",
            Self::Unlocks => "unlocks",
            Self::Chain => "chain",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Quest {
    /// Internal quest ID from a quest: key, not the in-game quest code.
    #[schemars(range(min = 1))]
    pub quest_id: i64,
    #[serde(default)]
    pub direction: Direction,
    #[serde(default)]
    pub language: Language,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 50))]
    pub limit: usize,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Price {
    pub item_code: i64,
    #[serde(default)]
    pub enhance: i64,
    /// User-supplied price per unit. Zero is an explicit free material.
    #[schemars(range(min = 0))]
    pub unit_price: i64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Calculate {
    #[schemars(range(min = 1))]
    pub recipe_id: i64,
    /// Desired output units, not number of crafting batches.
    #[serde(default = "default_quantity")]
    #[schemars(range(min = 1))]
    pub quantity: i64,
    /// Omit unpriced materials. They remain unknown rather than becoming zero.
    #[serde(default)]
    #[schemars(length(max = 128))]
    pub prices: Vec<Price>,
    #[serde(default)]
    pub language: Language,
}

pub fn validate_page(offset: usize, limit: usize) -> Result<(), String> {
    if !(1..=50).contains(&limit) || offset > 1_000_000 {
        Err("limit must be 1–50 and offset must be 0–1000000".into())
    } else {
        Ok(())
    }
}

pub fn validate_text(text: &str, maximum: usize) -> Result<(), String> {
    if text.chars().count() > maximum || text.chars().any(char::is_control) {
        Err(format!(
            "Text must contain at most {maximum} characters and no control characters"
        ))
    } else {
        Ok(())
    }
}
