use anyhow::Result;
use axum::{
    Router,
    extract::{Json, State},
    response::IntoResponse,
    routing::{get, post},
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

mod browser;
mod mcp;

use browser::Browser;
use mcp::{MCPRequest, MCPServer, Tool, ToolInputSchema};

#[derive(Parser)]
#[command(name = "llm-helper")]
#[command(about = "LLM Helper Server with MCP and web browsing")]
struct Args {
    #[arg(short, long, default_value = "3000")]
    port: u16,

    #[arg(long)]
    mcp_stdio: bool,

    #[arg(long)]
    mcp_http: bool,
}

#[derive(Clone)]
struct AppState {
    browser: Arc<Browser>,
    mcp_server: Arc<MCPServer>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct BrowseRequest {
    url: String,
    #[serde(default)]
    extract_text: bool,
    #[serde(default)]
    wait_seconds: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct BrowseResponse {
    url: String,
    title: Option<String>,
    text_content: String,
    links: Vec<String>,
    images: Vec<String>,
    status_code: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SearchRequest {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    5
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SearchResult {
    query: String,
    results: Vec<browser::SearchResultItem>,
}

async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "llm-helper",
        "version": env!("CARGO_PKG_VERSION"),
        "features": ["web_browsing", "mcp"]
    }))
}

async fn browse_web(
    State(state): State<AppState>,
    Json(req): Json<BrowseRequest>,
) -> Result<impl IntoResponse, AppError> {
    info!("Browsing URL: {}", req.url);

    let result = state.browser.fetch(&req.url, req.extract_text).await?;

    Ok(Json(BrowseResponse {
        url: req.url,
        title: result.title,
        text_content: result.text,
        links: result.links,
        images: result.images,
        status_code: result.status_code,
    }))
}

async fn search_web(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Result<impl IntoResponse, AppError> {
    info!("Searching for: {}", req.query);

    let results = state.browser.search(&req.query, req.limit).await?;

    Ok(Json(SearchResult {
        query: req.query,
        results,
    }))
}

async fn mcp_tools(State(state): State<AppState>) -> impl IntoResponse {
    let tools = state.mcp_server.list_tools().await;
    Json(serde_json::json!({
        "tools": tools
    }))
}

async fn mcp_invoke(
    State(state): State<AppState>,
    Json(req): Json<MCPRequest>,
) -> Result<impl IntoResponse, AppError> {
    let response = state
        .mcp_server
        .handle_request(req, state.browser.clone())
        .await;
    Ok(Json(response))
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
        .route("/health", get(health_check))
        .route("/browse", post(browse_web))
        .route("/search", post(search_web))
        .route("/mcp/tools", get(mcp_tools))
        .route("/mcp/invoke", post(mcp_invoke))
        .with_state(state)
}

async fn run_http_server(port: u16, state: AppState) -> Result<()> {
    let addr = format!("127.0.0.1:{}", port);
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

    let state = AppState {
        browser,
        mcp_server,
    };

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
        description:
            "Fetch and extract content from a webpage. Returns text content, links, and metadata."
                .to_string(),
        input_schema: ToolInputSchema {
            r#type: "object".to_string(),
            properties: {
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
            },
            required: vec!["url".to_string()],
        },
    });

    server.register_tool(Tool {
        name: "search_web".to_string(),
        description: "Search the web for information. Returns search results with titles, URLs, and snippets.".to_string(),
        input_schema: ToolInputSchema {
            r#type: "object".to_string(),
            properties: {
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
            },
            required: vec!["query".to_string()],
        },
    });

    server.register_tool(Tool {
        name: "extract_links".to_string(),
        description: "Extract all links from a webpage.".to_string(),
        input_schema: ToolInputSchema {
            r#type: "object".to_string(),
            properties: {
                let mut props = HashMap::new();
                props.insert(
                    "url".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "The URL to extract links from"
                    }),
                );
                props
            },
            required: vec!["url".to_string()],
        },
    });

    server
}
