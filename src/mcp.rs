use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::info;

use crate::browser::Browser;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MCPRequest {
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MCPResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<MCPError>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MCPError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub description: String,
    pub input_schema: ToolInputSchema,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ToolInputSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ToolContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

pub struct MCPServer {
    _name: String,
    _version: String,
    tools: Arc<std::sync::Mutex<Vec<Tool>>>,
}

impl MCPServer {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            _name: name.to_string(),
            _version: version.to_string(),
            tools: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn register_tool(&self, tool: Tool) {
        if let Ok(mut tools) = self.tools.lock() {
            tools.push(tool);
        }
    }

    pub async fn list_tools(&self) -> Vec<Tool> {
        // Acquire lock and clone - no await needed for std::sync::Mutex
        self.tools.lock().unwrap().clone()
    }

    pub async fn handle_request(
        &self,
        request: MCPRequest,
        browser: Arc<Browser>,
    ) -> Option<MCPResponse> {
        match request.method.as_str() {
            "initialize" => Some(self.handle_initialize(request.id, request.params).await),
            "notifications/initialized" => {
                info!("Client initialized notification received");
                None // Notifications don't return responses
            }
            "ping" => Some(self.handle_ping(request.id).await),
            "tools/list" => Some(self.handle_tools_list(request.id).await),
            "tools/call" => Some(self.handle_tool_call(request, browser).await),
            _ => Some(MCPResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(MCPError {
                    code: -32601,
                    message: format!("Method '{}' not found", request.method),
                    data: None,
                }),
            }),
        }
    }

    async fn handle_initialize(
        &self,
        id: Option<u64>,
        params: Option<serde_json::Value>,
    ) -> MCPResponse {
        info!("MCP initialize request received");

        // Parse client protocol version if provided
        let client_version = params
            .as_ref()
            .and_then(|p| p.get("protocolVersion"))
            .and_then(|v| v.as_str())
            .unwrap_or("2025-03-26");

        info!("Client protocol version: {}", client_version);

        // Return server capabilities
        let result = serde_json::json!({
            "protocolVersion": "2025-03-26",
            "serverInfo": {
                "name": self._name,
                "version": self._version
            },
            "capabilities": {
                "tools": {
                    "listChanged": false
                }
            }
        });

        MCPResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    async fn handle_ping(&self, id: Option<u64>) -> MCPResponse {
        MCPResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(serde_json::json!({})),
            error: None,
        }
    }

    async fn handle_tools_list(&self, id: Option<u64>) -> MCPResponse {
        let tools = self.tools.lock().unwrap().clone();
        let result = serde_json::json!({
            "tools": tools
        });

        MCPResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    async fn handle_tool_call(&self, request: MCPRequest, browser: Arc<Browser>) -> MCPResponse {
        let params = match request.params {
            Some(p) => p,
            None => {
                return MCPResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: None,
                    error: Some(MCPError {
                        code: -32602,
                        message: "Missing params".to_string(),
                        data: None,
                    }),
                };
            }
        };

        let tool_name = match params.get("name").and_then(|v| v.as_str()) {
            Some(name) => name,
            None => {
                return MCPResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: None,
                    error: Some(MCPError {
                        code: -32602,
                        message: "Missing tool name".to_string(),
                        data: None,
                    }),
                };
            }
        };

        let arguments = params.get("arguments").cloned().unwrap_or_default();

        let result = match tool_name {
            "browse_web" => self.tool_browse_web(browser, arguments).await,
            "search_web" => self.tool_search_web(browser, arguments).await,
            "extract_links" => self.tool_extract_links(browser, arguments).await,
            "smart_search" => self.tool_smart_search(browser, arguments).await,
            _ => Err(anyhow!("Unknown tool: {}", tool_name)),
        };

        match result {
            Ok(tool_result) => MCPResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(serde_json::to_value(tool_result).unwrap()),
                error: None,
            },
            Err(e) => MCPResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(MCPError {
                    code: -32000,
                    message: e.to_string(),
                    data: None,
                }),
            },
        }
    }

    async fn tool_browse_web(
        &self,
        browser: Arc<Browser>,
        args: serde_json::Value,
    ) -> Result<ToolResult> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing URL"))?;

        let extract_text = args
            .get("extract_text")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        info!("MCP tool browse_web: {}", url);
        let result = browser.fetch(url, extract_text).await?;

        let text = format!(
            "Title: {}\nURL: {}\nStatus: {}\n\nContent:\n{}\n\nLinks found: {}\nImages found: {}",
            result.title.as_deref().unwrap_or("N/A"),
            url,
            result.status_code,
            result.text,
            result.links.len(),
            result.images.len()
        );

        Ok(ToolResult {
            content: vec![ToolContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: None,
        })
    }

    async fn tool_search_web(
        &self,
        browser: Arc<Browser>,
        args: serde_json::Value,
    ) -> Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing query"))?;

        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

        info!("MCP tool search_web: {}", query);
        let results = browser.search(query, limit).await?;

        let mut text = format!("Search results for: '{}'\n\n", query);
        for (i, result) in results.iter().enumerate() {
            text.push_str(&format!(
                "{}. {}\n   URL: {}\n   {}\n\n",
                i + 1,
                result.title,
                result.url,
                result.snippet
            ));
        }

        Ok(ToolResult {
            content: vec![ToolContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: None,
        })
    }

    async fn tool_extract_links(
        &self,
        browser: Arc<Browser>,
        args: serde_json::Value,
    ) -> Result<ToolResult> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing URL"))?;

        info!("MCP tool extract_links: {}", url);
        let result = browser.fetch(url, false).await?;

        let text = format!("Links found on {}:\n\n{}", url, result.links.join("\n"));

        Ok(ToolResult {
            content: vec![ToolContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: None,
        })
    }

    async fn tool_smart_search(
        &self,
        browser: Arc<Browser>,
        args: serde_json::Value,
    ) -> Result<ToolResult> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing URL"))?;

        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing query"))?;

        let max_depth = args.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        info!("MCP tool smart_search: '{}' on {}", query, url);
        let result = browser.smart_search(url, query, max_depth).await?;

        let text = if result.found {
            format!(
                "Found '{}' on: {}\n\nContext:\n{}\n\nSearch successful!",
                result.query, result.source_url, result.context
            )
        } else {
            format!(
                "Could not find '{}' on {} (searched {} pages deep)\n\nThe information may not be publicly available on this website.",
                result.query, url, max_depth
            )
        };

        Ok(ToolResult {
            content: vec![ToolContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: None,
        })
    }
}

use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;

/// Session for MCP HTTP+SSE transport
pub struct Session {
    #[allow(dead_code)]
    pub id: String,
    #[allow(dead_code)]
    pub tx: broadcast::Sender<String>,
}

static SESSION_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn create_session_id() -> String {
    format!(
        "session_{}",
        SESSION_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

pub async fn run_stdio_server(server: Arc<MCPServer>, browser: Arc<Browser>) -> anyhow::Result<()> {
    info!("MCP stdio server starting");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let reader = BufReader::new(stdin);
    let mut stdout = stdout;
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        info!("Received: {}", line);

        let request: MCPRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let error_response = MCPResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(MCPError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                };
                let response_json = serde_json::to_string(&error_response)?;
                stdout.write_all(response_json.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                continue;
            }
        };

        let response = server.handle_request(request, browser.clone()).await;

        // Only send response if there is one (notifications return None)
        if let Some(resp) = response {
            let response_json = serde_json::to_string(&resp)?;
            info!("Sending: {}", response_json);
            stdout.write_all(response_json.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}
