//! Local stdio MCP, using the official SDK for framing, negotiation and lifecycle.
use std::sync::Mutex;

use rmcp::{model::*, service::RequestContext, ErrorData, RoleServer, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::db;
mod params;
use params::*;

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
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
}

fn tools() -> Vec<Tool> {
    vec![
        tool::<Empty>("catalog_info", "Read the installed offline Gersang catalog version, supported categories and record counts. No live market data."),
        tool::<Search>("search_catalog", "Search Gersang / 巨商 items, monsters, recipes, quests, heroes, NPCs or equipment sets by name, code or alias. Returns stable keys for get_entity. Use next_offset to page through every match."),
        tool::<Entity>("get_entity", "Read a record using an exact key from search_catalog. Includes bilingual details, related-record summary, and numeric combat stats and locations for monsters. Call for each monster to compare them."),
        tool::<Item>("find_item_drops", "Find all monsters that drop an exact item_code, with locations, solo-only flags and per-slot drop probabilities. drop_rate is a fraction from 0 to 1; unknown rates are null. Multiple slots remain separate in drops; no combined probability is inferred. Paginated; excludes shops and quest rewards."),
        tool::<Quest>("get_quest_dependencies", "Get immediate prerequisites, immediate unlocks or the connected quest chain. quest_id is the internal ID from search_catalog, not the game's quest code. Paginated."),
        tool::<Item>("get_recipes", "List all recipes producing an exact item_code. Complete calculable recipes include direct materials, batch yield and fees; incomplete/reinforcement recipes are explicitly marked unavailable for calculation."),
        tool::<Calculate>("calculate_recipe", "Calculate direct materials and crafting costs for a desired output quantity using a recipe_id from get_recipes. Prices must come from the user. Missing prices remain unknown. Does not save or change anything. Excludes learning fees and failed attempts."),
    ]
}

fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments).map_err(|error| format!("Invalid arguments: {error}"))
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
        if matches!(name, "get_recipes" | "calculate_recipe") && state.calculator.is_none() {
            state.calculator = Some(state.catalog.calculator_catalog()?);
        }
        let catalog = &state.catalog;
        let result = match name {
            "search_catalog" => {
                let args: Search = parse(arguments)?;
                validate_page(args.offset, args.limit)?;
                validate_text(&args.query, 200)?;
                Ok(catalog.mcp_search(
                    args.category.code(),
                    &args.query,
                    args.language.code(),
                    args.offset,
                    args.limit,
                ))
            }
            "get_entity" => {
                let args: Entity = parse(arguments)?;
                validate_text(&args.key, 100)?;
                catalog.mcp_entity(&args.key, args.language.code())
            }
            "find_item_drops" | "get_recipes" => {
                let args: Item = parse(arguments)?;
                validate_page(args.offset, args.limit)?;
                if args.item_code <= 0 {
                    return Err("item_code must be positive".into());
                }
                if name == "find_item_drops" {
                    catalog.mcp_drops(
                        args.item_code,
                        args.language.code(),
                        args.offset,
                        args.limit,
                    )
                } else {
                    catalog.mcp_recipes(
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
                catalog.mcp_quests(
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
                let prices: Vec<_> = args
                    .prices
                    .iter()
                    .map(|p| (p.item_code, p.enhance, p.unit_price))
                    .collect();
                catalog.mcp_calculate(
                    state.calculator.as_ref().expect("loaded calculator"),
                    args.recipe_id,
                    args.quantity,
                    &prices,
                    args.language.code(),
                )
            }
            _ => Err(format!("Unknown tool: {name}")),
        }?;
        Ok(result)
    }

    fn result(&self, name: &str, arguments: Value) -> CallToolResult {
        match self.invoke(name, arguments) {
            Ok(mut value) => {
                value["catalog_version"] = self.info["catalog_version"].clone();
                if value.to_string().len() > 256 * 1024 {
                    CallToolResult::structured_error(
                        json!({"error": "Result exceeds 256 KiB. Use a narrower query or a smaller limit."}),
                    )
                } else {
                    CallToolResult::structured(value)
                }
            }
            Err(message) => CallToolResult::structured_error(json!({"error": message})),
        }
    }
}

impl ServerHandler for GersangMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("gersang", env!("CARGO_PKG_VERSION")))
            .with_instructions("Read-only Gersang / 巨商 offline reference. Search first and use returned keys/IDs. Default output is Traditional Chinese with English names. Catalog data may lag the live game. Never invent missing drop rates or market prices. Cost calculation uses only user-supplied prices and does not save data.")
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
            tools: tools(),
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
