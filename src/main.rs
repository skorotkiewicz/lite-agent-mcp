use anyhow::Result;
use axum::{
    Router,
    extract::{Json, State},
    response::IntoResponse,
    routing::post,
};
use clap::Parser;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

mod browser;
mod mcp;

use browser::Browser;
use mcp::{MCPRequest, MCPServer, Session, Tool, ToolInputSchema, create_session_id};

#[derive(Parser)]
#[command(name = "llm-helper")]
#[command(about = "LLM Helper Server with MCP")]
struct Args {
    #[arg(short, long, default_value = "3000")]
    port: u16,

    #[arg(long)]
    mcp_stdio: bool,
}

#[derive(Clone)]
struct AppState {
    browser: Arc<Browser>,
    mcp_server: Arc<MCPServer>,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl AppState {
    fn new(browser: Arc<Browser>, mcp_server: Arc<MCPServer>) -> Self {
        Self {
            browser,
            mcp_server,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

/// MCP HTTP endpoint - POST only
async fn mcp_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Option<Json<MCPRequest>>,
) -> Result<impl IntoResponse, AppError> {
    // Check for session ID in header
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // POST request with JSON-RPC message
    let Some(Json(request)) = body else {
        return Ok((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "error": { "code": -32700, "message": "Parse error: missing request body" },
                "id": null
            })),
        )
            .into_response());
    };

    // Handle initialize request (creates new session if no session ID)
    let is_initialize = request.method == "initialize";

    let session_id = if let Some(sid) = session_id {
        sid
    } else if is_initialize {
        // New session for initialize request
        let new_session_id = create_session_id();
        info!("New MCP session from initialize: {}", new_session_id);
        new_session_id
    } else {
        return Ok((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "error": { "code": -32000, "message": "Bad Request: No valid session ID provided" },
                "id": request.id
            })),
        )
            .into_response());
    };

    // Create session if it doesn't exist (for initialize)
    if !state.sessions.read().await.contains_key(&session_id) {
        state.sessions.write().await.insert(session_id.clone(), ());
    }

    // Process the request
    let response = state
        .mcp_server
        .handle_request(request, state.browser.clone())
        .await;

    // Regular JSON response
    let mut response = match response {
        Some(resp) => Json(resp).into_response(),
        None => axum::http::StatusCode::ACCEPTED.into_response(),
    };

    response.headers_mut().insert(
        "mcp-session-id",
        axum::http::HeaderValue::from_str(&session_id).unwrap(),
    );

    Ok(response)
}

struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let error_message = format!("Error: {}", self.0);
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": error_message
            })),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        AppError(err.into())
    }
}

fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/mcp", post(mcp_handler))
        .with_state(state)
}

async fn run_http_server(port: u16, state: AppState) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    info!("Starting HTTP server on {}", addr);

    let app = create_app(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Server ready at http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn run_mcp_stdio(state: AppState) -> Result<()> {
    info!("Starting MCP stdio server");
    mcp::run_stdio_server(state.mcp_server, state.browser).await
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let browser = Arc::new(Browser::new().await?);
    let mcp_server = Arc::new(create_mcp_server());

    let state = AppState::new(browser, mcp_server);

    if args.mcp_stdio {
        run_mcp_stdio(state).await?;
    } else {
        run_http_server(args.port, state).await?;
    }

    Ok(())
}

fn create_mcp_server() -> MCPServer {
    let server = MCPServer::new("llm-helper", env!("CARGO_PKG_VERSION"));

    server.register_tool(Tool {
        name: "browse_web".to_string(),
        title: Some("Browse Web".to_string()),
        description:
            "Fetch and extract content from a webpage. Returns text content, links, and metadata."
                .to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "url".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "The URL to browse"
                    }),
                );
                props.insert(
                    "extract_text".to_string(),
                    serde_json::json!({
                        "type": "boolean",
                        "description": "Whether to extract readable text (default: true)",
                        "default": true
                    }),
                );
                props
            }),
            required: Some(vec!["url".to_string()]),
        },
    });

    server.register_tool(Tool {
        name: "search_web".to_string(),
        title: Some("Search Web".to_string()),
        description: "Search the web for information. Returns search results with titles, URLs, and snippets.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "query".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "The search query"
                    }),
                );
                props.insert(
                    "limit".to_string(),
                    serde_json::json!({
                        "type": "integer",
                        "description": "Maximum number of results (default: 5)",
                        "default": 5
                    }),
                );
                props
            }),
            required: Some(vec!["query".to_string()]),
        },
    });

    server.register_tool(Tool {
        name: "extract_links".to_string(),
        title: Some("Extract Links".to_string()),
        description: "Extract all links from a webpage.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "url".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "The URL to extract links from"
                    }),
                );
                props
            }),
            required: Some(vec!["url".to_string()]),
        },
    });

    server
}
