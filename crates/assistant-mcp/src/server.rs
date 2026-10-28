//! MCP server handler over the assistant-memory engine.
// Roots and logging are deprecated upstream (SEP-2577) but remain part of ADR 0001 §1b.
#![allow(deprecated)]
use std::sync::{Arc, Mutex};

use assistant_memory::Memory;
use assistant_memory::resolve::{
    ScopeArgs, apply_default_scope, fallback_resolution_roots, normalize_file_roots,
    resolve_project_scope,
};
use assistant_memory::tools::{self, ToolResult};
use rmcp::ServerHandler;
use rmcp::model::*;
use rmcp::service::{NotificationContext, RequestContext, RoleServer};
use serde_json::{Value, json};

use crate::{VERSION, prompts::PROMPTS, schemas};

pub type SharedMemory = Arc<Mutex<Memory>>;

/// Seed the registry from `~/.shelbymcp/projects.seed.json` (empty by default).
pub fn seed_registry(memory: &SharedMemory) {
    let seed = assistant_memory::seed::load_seed(&assistant_memory::seed::seed_default_path());
    let projects = assistant_memory::seed::to_registry_seeds(&seed);
    if let Err(e) =
        assistant_memory::seed::ensure_seed_projects(&memory.lock().unwrap().conn, &projects)
    {
        eprintln!("[WARN] project seed failed: {e}");
    }
}

#[derive(Clone)]
pub struct MemoryServer {
    memory: SharedMemory,
    /// Memoized client roots (file paths); cleared on `roots/list_changed`.
    roots: Arc<tokio::sync::Mutex<Option<Vec<String>>>>,
    log_level: Arc<Mutex<LoggingLevel>>,
}

impl MemoryServer {
    pub fn new(memory: SharedMemory) -> Self {
        Self {
            memory,
            roots: Arc::new(tokio::sync::Mutex::new(None)),
            log_level: Arc::new(Mutex::new(LoggingLevel::Info)),
        }
    }

    /// Await and memoize the client's roots so the first scoped call cannot
    /// race ahead with the server's launch directory (ADR 0001 §1b.1).
    async fn resolution_roots(&self, ctx: &RequestContext<RoleServer>) -> Vec<String> {
        let mut cached = self.roots.lock().await;
        if let Some(paths) = cached.as_ref() {
            return paths.clone();
        }
        let paths = match ctx.peer.list_roots().await {
            Ok(result) => {
                let uris: Vec<String> = result.roots.iter().map(|r| r.uri.clone()).collect();
                normalize_file_roots(&uris)
            }
            Err(_) => std::env::current_dir()
                .map(|d| fallback_resolution_roots(&d.to_string_lossy()))
                .unwrap_or_default(),
        };
        *cached = Some(paths.clone());
        paths
    }

    fn scope_args(args: &Value) -> ScopeArgs<'_> {
        ScopeArgs {
            project_id: args["project_id"].as_str(),
            project_identifier: args["project_identifier"].as_str(),
            include_shared: args["include_shared"].as_bool(),
            shared_only: args["shared_only"].as_bool().unwrap_or(false),
            all_projects: args["all_projects"].as_bool().unwrap_or(false),
        }
    }

    /// Rewrite scope fields per `applyDefaultScope`, or return the fail-closed error.
    fn scoped_args(m: &Memory, args: Value, paths: &[String]) -> Result<Value, ToolResult> {
        let applied = apply_default_scope(&m.conn, &Self::scope_args(&args), paths)
            .map_err(|e| tools::error("temporary_failure", e.to_string()))?;
        match applied {
            Ok(scope) => {
                let mut out = args;
                let o = out.as_object_mut().expect("object args");
                match scope.project_id {
                    Some(pid) => {
                        o.insert("project_id".into(), json!(pid));
                        o.insert("project_identifier".into(), json!(scope.project_identifier));
                    }
                    None => {
                        o.remove("project_id");
                        o.remove("project_identifier");
                    }
                }
                o.insert("include_shared".into(), json!(scope.include_shared));
                o.insert("shared_only".into(), json!(scope.shared_only));
                if scope.all_projects {
                    o.insert("all_projects".into(), json!(true));
                }
                Ok(out)
            }
            Err(reference) => Err(tools::error(
                "project_scope_invalid",
                format!(
                    "Project scope could not be resolved ({}). Pass a registered project_id or project_identifier.",
                    reference.kind()
                ),
            )),
        }
    }

    async fn dispatch(
        &self,
        name: &str,
        args: Value,
        ctx: &RequestContext<RoleServer>,
    ) -> ToolResult {
        if name == "capture_thought"
            && let Err(error) = tools::validate_capture_thought_input(&args)
        {
            return error;
        }
        if let Err(message) = schemas::validate_tool_input(name, &args) {
            return tools::error("invalid_input", message);
        }
        let needs_roots = matches!(
            name,
            "capture_thought"
                | "search_thoughts"
                | "list_thoughts"
                | "get_brief"
                | "select_context"
        );
        let paths = if needs_roots {
            self.resolution_roots(ctx).await
        } else {
            vec![]
        };
        let m = self.memory.lock().unwrap();
        match name {
            "capture_thought" => {
                let detected = match resolve_project_scope(&m.conn, &paths, None) {
                    Ok(s) => s,
                    Err(e) => return tools::error("temporary_failure", e.to_string()),
                };
                tools::capture_thought(&m, &args, &detected)
            }
            "search_thoughts" | "list_thoughts" | "get_brief" | "select_context" => {
                let scoped = match Self::scoped_args(&m, args, &paths) {
                    Ok(a) => a,
                    Err(e) => return e,
                };
                match name {
                    "search_thoughts" => tools::search_thoughts_tool(&m, &scoped),
                    "list_thoughts" => tools::list_thoughts_tool(&m, &scoped),
                    "get_brief" => tools::get_brief_tool(&m, &scoped),
                    _ => tools::select_context_tool(&m, &scoped),
                }
            }
            "get_thought" => tools::get_thought_tool(&m, &args),
            "update_thought" => tools::update_thought_tool(&m, &args),
            "delete_thought" => tools::delete_thought_tool(&m, &args),
            "manage_edges" => tools::manage_edges_tool(&m, &args),
            "explore_graph" => tools::explore_graph_tool(&m, &args),
            "expand_neighbors" => tools::expand_neighbors_tool(&m, &args),
            "thought_stats" => tools::thought_stats_tool(&m),
            other => tools::error("invalid_input", format!("Unknown tool: {other}")),
        }
    }
}

impl ServerHandler for MemoryServer {
    /// Protocol 2026-07-28 (SEP-2549) makes `ttlMs` and `cacheScope` required on every
    /// list result, and strict clients such as Claude Code reject a list without them.
    /// rmcp 3.1 has no fields for those hints, so do not agree to that version; a client
    /// that asks for it is negotiated down to 2025-11-25.
    fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
        // ponytail: drop this override once rmcp emits SEP-2549 cache hints on list results.
        std::borrow::Cow::Borrowed(&[
            ProtocolVersion::V_2024_11_05,
            ProtocolVersion::V_2025_03_26,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_11_25,
        ])
    }

    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_prompts()
            .enable_resources()
            .enable_logging()
            .enable_completions()
            .build();
        let mut info = ServerInfo::new(capabilities);
        let mut implementation = Implementation::new("shelbymcp", VERSION);
        implementation.title = Some("Shelby MCP".into());
        info.server_info = implementation;
        info.instructions = Some(crate::prompts::INITIALIZATION_INSTRUCTIONS.into());
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(schemas::tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        let result = self.dispatch(request.name.as_ref(), args, &ctx).await;
        let content = vec![ContentBlock::text(result.text)];
        Ok((if result.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        })
        .into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let prompts = PROMPTS
            .iter()
            .map(|p| {
                let mut prompt = Prompt::new(p.name, Some(p.description), None);
                prompt.title = Some(p.title.into());
                prompt
            })
            .collect();
        Ok(ListPromptsResult::with_all_items(prompts))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        let Some(p) = PROMPTS.iter().find(|p| p.name == request.name) else {
            return Err(ErrorData::invalid_params(
                format!("unknown prompt: {}", request.name),
                None,
            ));
        };
        Ok(
            GetPromptResult::new(vec![PromptMessage::new_text(Role::User, p.text)])
                .with_description(p.description)
                .into(),
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let mut resource = Resource::new("shelbymcp://status", "status");
        resource.description = Some("ShelbyMCP server status".into());
        resource.mime_type = Some("application/json".into());
        Ok(ListResourcesResult::with_all_items(vec![resource]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if request.uri != "shelbymcp://status" {
            return Err(ErrorData::resource_not_found(
                format!("unknown resource: {}", request.uri),
                None,
            ));
        }
        let text = json!({ "status": "ok", "tools": 11, "prompts": 3 }).to_string();
        Ok(
            ReadResourceResult::new(vec![ResourceContents::TextResourceContents {
                uri: request.uri,
                mime_type: Some("application/json".into()),
                text,
                meta: None,
            }])
            .into(),
        )
    }

    async fn complete(
        &self,
        request: CompleteRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, ErrorData> {
        let prefix = request.argument.value;
        let m = self.memory.lock().unwrap();
        let values: Vec<String> = match request.argument.name.as_str() {
            "topic" | "topics" => m
                .distinct_array_values("topics", &prefix, 20)
                .unwrap_or_default(),
            "person" | "people" => m
                .distinct_array_values("people", &prefix, 20)
                .unwrap_or_default(),
            "project" => m
                .distinct_values("project", &prefix, 20)
                .unwrap_or_default(),
            "source" => m.distinct_values("source", &prefix, 20).unwrap_or_default(),
            "type" => [
                "note",
                "decision",
                "task",
                "question",
                "reference",
                "insight",
            ]
            .iter()
            .filter(|t| t.starts_with(&prefix.to_lowercase()))
            .map(|t| t.to_string())
            .collect(),
            "edge_type" => assistant_memory::edges::VALID_EDGE_TYPES
                .iter()
                .filter(|t| t.starts_with(&prefix.to_lowercase()))
                .map(|t| t.to_string())
                .collect(),
            _ => vec![],
        };
        let mut info =
            CompletionInfo::new(values).map_err(|e| ErrorData::internal_error(e, None))?;
        info.has_more = Some(false);
        info.total = Some(info.values.len() as u32);
        Ok(CompleteResult::new(info))
    }

    async fn set_level(
        &self,
        request: SetLevelRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        *self.log_level.lock().unwrap() = request.level;
        Ok(())
    }

    async fn on_roots_list_changed(&self, _ctx: NotificationContext<RoleServer>) {
        *self.roots.lock().await = None;
    }
}
