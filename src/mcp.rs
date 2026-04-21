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
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: ToolInputSchema,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolInputSchema {
    pub r#type: String,
    pub properties: HashMap<String, serde_json::Value>,
    pub required: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolContent {
    pub r#type: String,
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

    pub async fn handle_request(&self, request: MCPRequest, browser: Arc<Browser>) -> MCPResponse {
        match request.method.as_str() {
            "tools/list" => self.handle_tools_list(request.id).await,
            "tools/call" => self.handle_tool_call(request, browser).await,
            _ => MCPResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(MCPError {
                    code: -32601,
                    message: format!("Method '{}' not found", request.method),
                    data: None,
                }),
            },
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
                r#type: "text".to_string(),
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
                r#type: "text".to_string(),
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
                r#type: "text".to_string(),
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
                r#type: "text".to_string(),
                text,
            }],
            is_error: None,
        })
    }
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
        let response_json = serde_json::to_string(&response)?;

        info!("Sending: {}", response_json);

        stdout.write_all(response_json.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}
