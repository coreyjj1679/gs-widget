//! Local stdio MCP, using the official SDK for framing, negotiation and lifecycle.
use std::sync::{Mutex, OnceLock};

use rmcp::{model::*, service::RequestContext, ErrorData, RoleServer, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::db;
mod params;
use params::*;
mod output;

struct CatalogState {
    catalog: db::Catalog,
    calculator: Option<db::CalcCatalog>,
}
struct GersangMcp {
    state: Mutex<CatalogState>,
    info: Value,
}

fn tool<T: JsonSchema>(name: &'static str, description: &'static str) -> Tool {
    let schema = serde_json::to_value(schemars::schema_for!(T)).expect("JSON schema");
    Tool::new(
        name,
        description,
        schema.as_object().expect("object schema").clone(),
    )
    .with_raw_output_schema(std::sync::Arc::new(output::schema(name)))
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
}

fn tools() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| vec![
        tool::<QueryCatalog>("query_catalog", "Start here: pass the original Gersang / 巨商 question or nickname (e.g. 雀天要點合？). Resolves names and returns compact requested facts. Explicit drop/shop/craft wording narrows acquisition sources; sources can override. action=calculate with quantity resolves an item and computes direct materials in one call, asking for a recipe choice when needed. keys batch up to 8 records; compare aligns numeric stats. Use sections for focused details and follow next_call for more records."),
        tool::<Empty>("catalog_info", "Read the installed offline Gersang catalog version, supported categories and record counts. No live market data."),
        tool::<Search>("search_catalog", "Search items, monsters, recipes, quests, heroes, NPCs, sets, towns, facilities or buildings by name/code/alias. Optional filters apply before pagination: consult catalog_info for category-specific values (region, nation, element, boss, item classification, recipe level). Returns stable keys; follow next_call for every match."),
        tool::<Entity>("get_entity", "Read any returned entity key, including town:, bay: (facility) and yard: (building). Use sections to fetch only overview, typed stats, skills, effects, related or production where supported; empty returns full details. Related lists are summaries with counts and executable get_relations calls."),
        tool::<Relations>("get_relations", "Retrieve complete paginated relationships from an exact key. Item relations: shops, production, quest_rewards, uses (reverse material usage), recipes, drops, sets, characters. Monster: drops/quests; set: members; town: goods; bay: products; yard: facilities; npc: quests; hero/quest/recipe: related. Follow next_call until null. No GUI display caps."),
        tool::<Item>("find_item_drops", "Find all monsters that drop an exact item_code, with locations, solo-only flags and per-slot drop probabilities. drop_rate is a fraction from 0 to 1; unknown rates are null. Multiple slots remain separate in drops; no combined probability is inferred. Paginated; excludes shops and quest rewards."),
        tool::<Quest>("get_quest_dependencies", "Get immediate prerequisites, immediate unlocks or the connected quest chain. quest_id is the internal ID from search_catalog, not the game's quest code. Paginated."),
        tool::<Item>("get_recipes", "List all recipes producing an exact item_code. Complete calculable recipes include direct materials, batch yield and fees; incomplete/reinforcement recipes are explicitly marked unavailable for calculation."),
        tool::<Calculate>("calculate_recipe", "Calculate direct materials and crafting costs for a desired output quantity using a recipe_id from get_recipes. Prices must come from the user. Missing prices remain unknown. Does not save or change anything. Excludes learning fees and failed attempts."),
    ])
}

fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments).map_err(|error| format!("Invalid arguments: {error}"))
}

fn add_calculation_args(arguments: &mut Value, args: &QueryCatalog) {
    arguments["quantity"] = json!(args.quantity.unwrap_or(1));
    arguments["limit"] = json!(args.limit);
    if let Some(id) = args.recipe_id {
        arguments["recipe_id"] = json!(id);
    }
    arguments["prices"] = json!(args
        .prices
        .iter()
        .map(|p| json!({
            "item_code": p.item_code, "enhance": p.enhance, "unit_price": p.unit_price
        }))
        .collect::<Vec<_>>());
}

impl GersangMcp {
    fn new(catalog: db::Catalog) -> Result<Self, String> {
        let info = catalog.mcp_info()?;
        Ok(Self {
            state: Mutex::new(CatalogState {
                catalog,
                calculator: None,
            }),
            info,
        })
    }

    fn invoke(&self, name: &str, arguments: Value) -> Result<Value, String> {
        if arguments.to_string().len() > 64 * 1024 {
            return Err("Tool arguments exceed 64 KiB".into());
        }
        if name == "catalog_info" {
            let _: Empty = parse(arguments)?;
            return Ok(self.info.clone());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Catalog is unavailable; reconnect the MCP server")?;
        let continuation_arguments = arguments.clone();
        let mut result = match name {
            "query_catalog" => {
                let args: QueryCatalog = parse(arguments)?;
                validate_query(&args)?;
                let calculate = matches!(args.action, QueryAction::Calculate)
                    || (matches!(args.action, QueryAction::Auto) && args.quantity.is_some());
                let action = if calculate {
                    "calculate"
                } else if matches!(args.action, QueryAction::Auto) && !args.sections.is_empty() {
                    "overview"
                } else {
                    args.action.code()
                };
                let sources: Vec<_> = args.sources.iter().map(|s| s.code().to_string()).collect();
                let sections: Vec<_> = args.sections.iter().map(EntitySection::code).collect();
                let mut response = state.catalog.mcp_query_scoped(
                    &args.query,
                    &args.keys,
                    args.category.as_ref().map(Category::code),
                    action,
                    args.language.code(),
                    args.limit,
                    &sources,
                    &sections,
                    args.candidate_offset,
                )?;
                if calculate {
                    // Keep quantity/prices across candidate choices, without loading the calculator for unresolved names.
                    if response["next_call"].is_object() {
                        add_calculation_args(&mut response["next_call"]["arguments"], &args);
                    }
                    if let Some(calls) = response["next_calls"].as_array_mut() {
                        for call in calls {
                            add_calculation_args(&mut call["arguments"], &args);
                        }
                    }
                    if response["status"] == "resolved" {
                        let rows = response["results"].as_array().cloned().unwrap_or_default();
                        if rows.len() != 1 || response["truncated"] == true {
                            response["status"] = json!("ambiguous");
                            response["candidates"] = json!(rows);
                            response["results"] = json!([]);
                            response["next_calls"] = json!(rows.iter().map(|row| {
                                let mut arguments = json!({"keys": [row["key"]], "action": "calculate", "language": args.language.code().split('-').next().unwrap_or("zh")});
                                add_calculation_args(&mut arguments, &args);
                                json!({"tool": "query_catalog", "requires_choice": true, "arguments": arguments})
                            }).collect::<Vec<_>>());
                        } else {
                            if state.calculator.is_none() {
                                state.calculator = Some(state.catalog.calculator_catalog()?);
                            }
                            let prices: Vec<_> = args
                                .prices
                                .iter()
                                .map(|p| (p.item_code, p.enhance, p.unit_price))
                                .collect();
                            response["results"][0]["calculation"] =
                                state.catalog.mcp_query_calculate(
                                    state.calculator.as_ref().unwrap(),
                                    rows[0]["key"].as_str().unwrap(),
                                    args.recipe_id,
                                    args.quantity.unwrap_or(1),
                                    &prices,
                                    args.language.code(),
                                    args.limit,
                                )?;
                        }
                    }
                }
                Ok(response)
            }

            "search_catalog" => {
                let args: Search = parse(arguments)?;
                validate_page(args.offset, args.limit)?;
                validate_text(&args.query, 200)?;
                state.catalog.mcp_search_filtered(
                    args.category.code(),
                    &args.query,
                    args.language.code(),
                    args.offset,
                    args.limit,
                    &args.filters,
                )
            }
            "get_entity" => {
                let args: Entity = parse(arguments)?;
                validate_text(&args.key, 100)?;
                if args.sections.len() > 6 {
                    return Err("Use at most 6 sections".into());
                }
                state.catalog.mcp_entity_sections(
                    &args.key,
                    args.language.code(),
                    &args
                        .sections
                        .iter()
                        .map(EntitySection::code)
                        .collect::<Vec<_>>(),
                )
            }
            "get_relations" => {
                let args: Relations = parse(arguments)?;
                validate_text(&args.key, 100)?;
                validate_text(&args.relation, 40)?;
                validate_page(args.offset, args.limit)?;
                state.catalog.mcp_relations(
                    &args.key,
                    &args.relation,
                    args.language.code(),
                    args.offset,
                    args.limit,
                )
            }
            "find_item_drops" | "get_recipes" => {
                let args: Item = parse(arguments)?;
                validate_page(args.offset, args.limit)?;
                if args.item_code <= 0 {
                    return Err("item_code must be positive".into());
                }
                if name == "find_item_drops" {
                    state.catalog.mcp_drops(
                        args.item_code,
                        args.language.code(),
                        args.offset,
                        args.limit,
                    )
                } else {
                    if state.calculator.is_none() {
                        state.calculator = Some(state.catalog.calculator_catalog()?);
                    }
                    state.catalog.mcp_recipes(
                        state.calculator.as_ref().expect("loaded calculator"),
                        args.item_code,
                        args.language.code(),
                        args.offset,
                        args.limit,
                    )
                }
            }
            "get_quest_dependencies" => {
                let args: Quest = parse(arguments)?;
                validate_page(args.offset, args.limit)?;
                if args.quest_id <= 0 {
                    return Err("quest_id must be positive".into());
                }
                state.catalog.mcp_quests(
                    args.quest_id,
                    args.direction.code(),
                    args.language.code(),
                    args.offset,
                    args.limit,
                )
            }
            "calculate_recipe" => {
                let args: Calculate = parse(arguments)?;
                if args.prices.len() > 128 || args.recipe_id <= 0 || args.quantity <= 0 {
                    return Err(
                        "Use a positive recipe_id and quantity, with at most 128 material prices"
                            .into(),
                    );
                }
                validate_prices(&args.prices)?;
                if state.calculator.is_none() {
                    state.calculator = Some(state.catalog.calculator_catalog()?);
                }
                let prices: Vec<_> = args
                    .prices
                    .iter()
                    .map(|p| (p.item_code, p.enhance, p.unit_price))
                    .collect();
                state.catalog.mcp_calculate(
                    state.calculator.as_ref().expect("loaded calculator"),
                    args.recipe_id,
                    args.quantity,
                    &prices,
                    args.language.code(),
                )
            }
            _ => Err(format!("Unknown tool: {name}")),
        }?;
        if result.get("next_offset").is_some() {
            result["next_call"] = if result["next_offset"].is_number() {
                let mut arguments = continuation_arguments;
                arguments["offset"] = result["next_offset"].clone();
                json!({"tool": name, "arguments": arguments})
            } else {
                Value::Null
            };
        }
        Ok(result)
    }

    fn result(&self, name: &str, arguments: Value) -> CallToolResult {
        match self.invoke(name, arguments) {
            Ok(mut value) => {
                value["catalog_version"] = self.info["catalog_version"].clone();
                if value.to_string().len() > 256 * 1024 {
                    CallToolResult::structured_error(
                        json!({"error": "Result exceeds 256 KiB. Use sections, a narrower query or a smaller limit.", "error_code": "RESULT_TOO_LARGE"}),
                    )
                } else {
                    CallToolResult::structured(value)
                }
            }
            Err(message) => CallToolResult::structured_error(output::error(message)),
        }
    }
}

impl ServerHandler for GersangMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("gersang", env!("CARGO_PKG_VERSION")))
            .with_instructions("Read-only Gersang / 巨商 offline reference. Start with query_catalog using the original short question/nickname without guessing a translation. Use action=calculate and quantity for a single item materials/cost task; preserve ambiguity and ask users to choose when requires_choice is true. Request only needed sources/sections; use search_catalog filters for comparisons, catalog_info for allowed filter values, and get_relations/next_call for complete lists. Empty or truncated summaries are not evidence that other sources do not exist. keys batches up to 8 records; compare aligns numeric stats. language supports zh/en/ko. Offline data may lag the game. Never invent missing rates, conditions or market prices; costs use only supplied prices.")
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if request.is_some_and(|request| request.cursor.is_some()) {
            return Err(ErrorData::invalid_params(
                "The tool list has no further pages",
                None,
            ));
        }
        Ok(ListToolsResult {
            tools: tools().to_vec(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !tools().iter().any(|tool| tool.name == request.name) {
            return Err(ErrorData::invalid_params("Unknown tool", None));
        }
        Ok(self
            .result(
                &request.name,
                Value::Object(request.arguments.unwrap_or_default()),
            )
            .into())
    }
}

pub fn run() -> Result<(), String> {
    // Keep the Windows updater from replacing a catalog in use by an MCP client.
    let _guard = crate::update::running_guard()?;
    let server = GersangMcp::new(db::Catalog::open_for_mcp()?)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let service = server
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| e.to_string())?;
        service.waiting().await.map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[cfg(test)]
mod tests;
