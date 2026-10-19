use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use rmcp::ErrorData;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, GetPromptRequestParams, GetPromptResponse, Implementation, InitializeResult,
    ListPromptsResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::Value;

use super::{Call, prompts, tools, url};

const INSTRUCTIONS: &str = "Filmlab is the photo editor open on this Mac; these tools drive it live. Loop: status → open_folder → \
list_photos → open_photo → change (set_controls, set_film, command) → look (preview, analyze) → repeat. Control ids, \
commands and stocks come from catalog. Every change is one undo step and is saved to the photo's XMP sidecar. Editing \
and look tools take an optional photo (an id) to work on any photo without opening it; calls on different photos run \
at once, so several agents can each take their own.";

const CALL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone)]
struct Filmlab {
    calls: mpsc::UnboundedSender<Call>,
}

impl ServerHandler for Filmlab {
    fn get_info(&self) -> InitializeResult {
        let mut info = InitializeResult::new(ServerCapabilities::builder().enable_tools().enable_prompts().build());
        info.server_info = Implementation::new("filmlab", env!("CARGO_PKG_VERSION"));
        info.instructions = Some(INSTRUCTIONS.into());
        info
    }

    async fn list_tools(&self, _: Option<PaginatedRequestParams>, _: RequestContext<RoleServer>) -> Result<ListToolsResult, ErrorData> {
        // why: the tools never change while Filmlab runs, so clients may keep the list for an hour
        Ok(ListToolsResult::with_all_items(tools::definitions()).with_ttl_ms(3_600_000).with_cache_scope(CacheScope::Public))
    }

    async fn list_prompts(&self, _: Option<PaginatedRequestParams>, _: RequestContext<RoleServer>) -> Result<ListPromptsResult, ErrorData> {
        Ok(ListPromptsResult::with_all_items(prompts::list()))
    }

    async fn get_prompt(&self, request: GetPromptRequestParams, _: RequestContext<RoleServer>) -> Result<GetPromptResponse, ErrorData> {
        let args = request.arguments.unwrap_or_default();
        prompts::get(&request.name, &args).map(Into::into).ok_or_else(|| ErrorData::invalid_params(format!("unknown prompt {}", request.name), None))
    }

    async fn call_tool(&self, request: CallToolRequestParams, context: RequestContext<RoleServer>) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        if !tools::exists(&name) {
            return Err(ErrorData::invalid_params(format!("unknown tool {name}"), None));
        }
        let args = request.arguments.map_or(Value::Object(Default::default()), Value::Object);
        let (reply, answer) = oneshot::channel();
        // why: a request without client info reports rmcp's own default name, which isn't the agent
        let client = context.client_info().map(|i| i.title.clone().unwrap_or_else(|| i.name.clone())).filter(|n| n != "rmcp");
        self.calls.unbounded_send(Call { name, args, client, reply }).map_err(|_| ErrorData::internal_error("Filmlab is closing", None))?;
        let output = match tokio::time::timeout(CALL_TIMEOUT, answer).await {
            Ok(Ok(output)) => output,
            Ok(Err(_)) => return Err(ErrorData::internal_error("Filmlab dropped the call", None)),
            Err(_) => tools::Output::error(format!("timed out after {} s", CALL_TIMEOUT.as_secs())),
        };
        let mut content = vec![ContentBlock::text(output.text)];
        content.extend(output.images.into_iter().map(|jpeg| {
            use base64::Engine as _;
            ContentBlock::image(base64::engine::general_purpose::STANDARD.encode(jpeg), "image/jpeg")
        }));
        Ok(if output.error { CallToolResult::error(content) } else { CallToolResult::success(content) }.into())
    }
}

fn write_discovery(port: u16) {
    let Some(home) = std::env::var_os("HOME") else { return };
    let dir = std::path::PathBuf::from(home).join("Library/Application Support/Filmlab");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("mcp.json"), serde_json::json!({"url": url(port), "pid": std::process::id()}).to_string());
}

// why: MCP's Streamable HTTP on loopback only, with Host and Origin checks against DNS rebinding; requests carrying any
// browser Origin are refused, since only local agents (which send none) should drive the editor
pub fn serve(port: u16, calls: mpsc::UnboundedSender<Call>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("filmlab-mcp-io").enable_all().build() {
        Ok(r) => r,
        Err(e) => return eprintln!("mcp: unable to start its runtime: {e}"),
    };
    runtime.block_on(async move {
        let config = StreamableHttpServerConfig::default().with_json_response(true).with_legacy_session_mode(false).enforce_origin_validation();
        let service = StreamableHttpService::new(move || Ok(Filmlab { calls: calls.clone() }), LocalSessionManager::default().into(), config);
        let router = axum::Router::new().nest_service("/mcp", service);
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => l,
            Err(e) => return eprintln!("mcp: unable to listen on 127.0.0.1:{port}: {e}"),
        };
        write_discovery(port);
        eprintln!("mcp: serving {}", url(port));
        if let Err(e) = axum::serve(listener, router).await {
            eprintln!("mcp: the server stopped: {e}");
        }
    });
}
