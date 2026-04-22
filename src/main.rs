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
use tokio::sync::RwLock;
use tracing::info;

mod browser;
mod mcp;

use browser::Browser;
use mcp::{MCPRequest, MCPServer, Session, Tool, ToolInputSchema, create_session_id};

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

    // Return 204 No Content for notifications (no response needed)
    match response {
        Some(resp) => Ok(Json(resp).into_response()),
        None => Ok(axum::http::StatusCode::NO_CONTENT.into_response()),
    }
}

/// Streamable HTTP MCP endpoint - GET for SSE, POST for JSON-RPC
async fn mcp_get_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    use axum::response::sse::Event;
    use axum::response::sse::Sse;
    use tokio::sync::broadcast;
    use tokio_stream::StreamExt;
    use tokio_stream::wrappers::BroadcastStream;

    // GET /mcp requires a session ID (Mcp-Session-Id header)
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let Some(session_id) = session_id else {
        // No session ID provided - return 400 Bad Request
        return Ok((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32000,
                    "message": "Bad Request: Mcp-Session-Id header required for SSE stream"
                },
                "id": null
            })),
        )
            .into_response());
    };

    // Check if session exists
    if !state.sessions.read().await.contains_key(&session_id) {
        return Ok((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32000,
                    "message": "Session not found"
                },
                "id": null
            })),
        )
            .into_response());
    }

    info!("MCP SSE stream opened for session: {}", session_id);

    // Create broadcast channel for this SSE connection
    let (tx, rx) = broadcast::channel(100);

    // Update session with the new sender (for server-initiated messages)
    {
        let mut sessions = state.sessions.write().await;
        if let Some(session) = sessions.get_mut(&session_id) {
            session.tx = tx.clone();
        }
    }

    let stream =
        BroadcastStream::new(rx).map(move |result| -> Result<Event, std::convert::Infallible> {
            match result {
                Ok(msg) => Ok(Event::default().data(msg)),
                Err(_) => Ok(Event::default().data("error")),
            }
        });

    Ok(Sse::new(stream).into_response())
}

async fn mcp_post_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Option<Json<MCPRequest>>,
) -> Result<impl IntoResponse, AppError> {
    use tokio::sync::broadcast;
    use tokio_stream::StreamExt;
    use tokio_stream::wrappers::BroadcastStream;

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
        let (tx, _rx) = broadcast::channel(100);
        let session = Session {
            id: session_id.clone(),
            tx: tx.clone(),
        };
        state
            .sessions
            .write()
            .await
            .insert(session_id.clone(), session);
    }

    // Check Accept header to determine response format
    let accept_header = headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");
    let wants_sse = accept_header.contains("text/event-stream");

    // Process the request
    let response = state
        .mcp_server
        .handle_request(request, state.browser.clone())
        .await;

    // Handle SSE response if requested (for requests that want streaming)
    if wants_sse && let Some(resp) = response {
        use axum::response::sse::{Event, Sse};

        let response_json = serde_json::to_string(&resp).unwrap();
        let (tx, rx) = broadcast::channel(1);
        let _ = tx.send(response_json);

        let stream = BroadcastStream::new(rx).map(
            move |result| -> Result<Event, std::convert::Infallible> {
                match result {
                    Ok(msg) => Ok(Event::default().data(msg)),
                    Err(_) => Ok(Event::default().data("error")),
                }
            },
        );

        let mut response = Sse::new(stream).into_response();
        response.headers_mut().insert(
            "mcp-session-id",
            axum::http::HeaderValue::from_str(&session_id).unwrap(),
        );
        return Ok(response);
    }

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

fn create_app(state: AppState, mcp_http: bool) -> Router {
    let mut router = Router::new()
        .route("/health", get(health_check))
        .route("/browse", post(browse_web))
        .route("/search", post(search_web));

    if mcp_http {
        // Streamable HTTP: separate GET and POST handlers
        router = router.route("/mcp", get(mcp_get_handler).post(mcp_post_handler));
        info!("MCP Streamable HTTP endpoint enabled at /mcp (GET for SSE, POST for JSON-RPC)");
    } else {
        router = router
            .route("/mcp/tools", get(mcp_tools))
            .route("/mcp/invoke", post(mcp_invoke));
    }

    router.with_state(state)
}

async fn run_http_server(port: u16, mcp_http: bool, state: AppState) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    info!("Starting HTTP server on {}", addr);

    let app = create_app(state, mcp_http);
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

    if args.mcp_stdio && args.mcp_http {
        anyhow::bail!("Cannot use both --mcp-stdio and --mcp-http. Choose one mode.");
    }

    if args.mcp_stdio {
        run_mcp_stdio(state).await?;
    } else {
        run_http_server(args.port, args.mcp_http, state).await?;
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

    server.register_tool(Tool {
        name: "smart_search".to_string(),
        title: Some("Smart Search".to_string()),
        description: "Intelligently search a website for specific information. Recursively follows relevant pages (contact, about, etc.) until it finds the query. Returns the page content where the information was found.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "url".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "The starting URL to search from"
                    }),
                );
                props.insert(
                    "query".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "What to search for (e.g., 'phone number', 'email address', 'pricing', 'contact information')"
                    }),
                );
                props.insert(
                    "max_depth".to_string(),
                    serde_json::json!({
                        "type": "integer",
                        "description": "Maximum pages to crawl (default: 10)",
                        "default": 10
                    }),
                );
                props
            }),
            required: Some(vec!["url".to_string(), "query".to_string()]),
        },
    });

    server
}
