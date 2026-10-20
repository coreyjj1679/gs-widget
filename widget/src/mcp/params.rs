use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Zh,
    En,
    Ko,
}
impl Language {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Zh => "zh-en",
            Self::En => "en-zh",
            Self::Ko => "ko-en",
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
    Town,
    Facility,
    Building,
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
            Self::Town => "town",
            Self::Facility => "facility",
            Self::Building => "building",
        }
    }
}

fn default_limit() -> usize {
    20
}
fn default_quantity() -> i64 {
    1
}

fn default_query_limit() -> usize {
    5
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum QueryAction {
    /// Infer overview or acquisition from a short question, such as 雀天要點合？.
    #[default]
    Auto,
    Overview,
    /// How to obtain, combine, promote, awaken or craft the resolved record.
    Acquisition,
    /// Compare two to eight exact keys in one call.
    Compare,
    /// Resolve an item and calculate direct materials in one call.
    Calculate,
}

impl QueryAction {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Overview => "overview",
            Self::Acquisition => "acquisition",
            Self::Compare => "compare",
            Self::Calculate => "calculate",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryCatalog {
    /// A short original question, name, code or game nickname; e.g. 雀天要點合？.
    /// Use query OR keys, not both. The server resolves known terminology;
    /// ambiguous names return candidates instead of choosing a record.
    #[serde(default)]
    #[schemars(length(max = 200))]
    pub query: String,
    /// Exact keys for batch lookup. Use 2–8 keys with action=compare.
    #[serde(default)]
    #[schemars(length(max = 8))]
    pub keys: Vec<String>,
    #[serde(default)]
    pub action: QueryAction,
    /// Optional category hint when resolving a query.
    #[serde(default)]
    pub category: Option<Category>,
    #[serde(default)]
    pub language: Language,
    /// Maximum candidates or related rows per result; use existing paginated
    /// tools to retrieve complete lists.
    #[serde(default = "default_query_limit")]
    #[schemars(range(min = 1, max = 10))]
    pub limit: usize,
    /// Offset within ambiguous name candidates; follow next_call rather than guessing.
    #[serde(default)]
    #[schemars(range(max = 1000000))]
    pub candidate_offset: usize,
    /// Acquisition sources to fetch. Empty infers a narrow scope from explicit
    /// drop/shop/craft wording, otherwise returns all recorded source types.
    #[serde(default)]
    pub sources: Vec<AcquisitionSource>,
    /// Fetch only these detail sections for overview/compare. Empty uses a compact overview.
    #[serde(default)]
    pub sections: Vec<EntitySection>,
    /// Desired output units. Supplying quantity with action=auto selects calculate.
    #[serde(default)]
    pub quantity: Option<i64>,
    /// Choose a recipe if the item has multiple routes; never selected by guesswork.
    #[serde(default)]
    pub recipe_id: Option<i64>,
    #[serde(default)]
    #[schemars(length(max = 128))]
    pub prices: Vec<Price>,
}

pub fn validate_query(args: &QueryCatalog) -> Result<(), String> {
    validate_text(&args.query, 200)?;
    let has_query = !args.query.trim().is_empty();
    if has_query == !args.keys.is_empty() {
        return Err("Provide exactly one non-empty query or keys list".into());
    }
    if args.keys.len() > 8 || !(1..=10).contains(&args.limit) {
        return Err("Use at most 8 keys and a limit from 1 to 10".into());
    }
    if args.candidate_offset > 1_000_000 || (args.candidate_offset > 0 && !args.keys.is_empty()) {
        return Err("candidate_offset must be 0–1000000 and applies only to a query".into());
    }
    let mut seen = std::collections::HashSet::new();
    for key in &args.keys {
        validate_text(key, 100)?;
        if key.trim().is_empty() || !seen.insert(key.trim()) {
            return Err("Keys must be non-empty and unique".into());
        }
    }
    if matches!(args.action, QueryAction::Compare) && args.keys.len() < 2 {
        return Err("Comparison requires 2–8 exact keys; resolve names with query first".into());
    }
    validate_prices(&args.prices)?;
    if args.quantity.is_some_and(|n| n <= 0) || args.recipe_id.is_some_and(|n| n <= 0) {
        return Err("quantity and recipe_id must be positive".into());
    }
    let calculate = matches!(args.action, QueryAction::Calculate)
        || (matches!(args.action, QueryAction::Auto) && args.quantity.is_some());
    if !calculate
        && (args.quantity.is_some() || args.recipe_id.is_some() || !args.prices.is_empty())
    {
        return Err(
            "quantity, recipe_id and prices require action=calculate (or auto with quantity)"
                .into(),
        );
    }
    if !args.sources.is_empty()
        && (!args.sections.is_empty()
            || calculate
            || matches!(args.action, QueryAction::Compare | QueryAction::Overview))
    {
        return Err("sources apply only to acquisition; use sections for overview/compare".into());
    }
    if !args.sections.is_empty() && matches!(args.action, QueryAction::Acquisition) {
        return Err("sections apply to overview/compare; use sources for acquisition".into());
    }
    if args.sources.len() > 5 || args.sections.len() > 6 || (calculate && !args.sections.is_empty())
    {
        return Err(
            "Use at most 5 sources or 6 sections; calculation has its own result fields".into(),
        );
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionSource {
    Recipes,
    MonsterDrops,
    Shops,
    Production,
    QuestRewards,
}
impl AcquisitionSource {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Recipes => "recipes",
            Self::MonsterDrops => "monster_drops",
            Self::Shops => "shops",
            Self::Production => "production",
            Self::QuestRewards => "quest_rewards",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EntitySection {
    Overview,
    Stats,
    Skills,
    Effects,
    Related,
    Production,
}
impl EntitySection {
    pub fn code(&self) -> String {
        match self {
            Self::Overview => "overview",
            Self::Stats => "stats",
            Self::Skills => "skills",
            Self::Effects => "effects",
            Self::Related => "related",
            Self::Production => "production",
        }
        .into()
    }
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
    /// Category-specific structured filters. catalog_info lists supported values.
    #[serde(default)]
    pub filters: crate::db::mcp::SearchFilters,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    /// Exact key returned by search_catalog, for example monster:M24 or item:6213.
    #[schemars(length(min = 1, max = 100))]
    pub key: String,
    #[serde(default)]
    pub language: Language,
    /// Empty returns full details; specify sections to reduce work and response size.
    #[serde(default)]
    pub sections: Vec<EntitySection>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Relations {
    pub key: String,
    /// Relation name from get_entity.relations or catalog_info.relations.
    pub relation: String,
    #[serde(default)]
    pub language: Language,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
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

pub fn validate_prices(prices: &[Price]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    if prices.len() > 128 {
        return Err("Use at most 128 material prices".into());
    }
    for price in prices {
        if price.item_code <= 0
            || price.enhance < 0
            || price.unit_price < 0
            || !seen.insert((price.item_code, price.enhance))
        {
            return Err("Material prices require positive item codes, non-negative enhancement/prices and unique materials".into());
        }
    }
    Ok(())
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
