use anyhow::{Result, anyhow};
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use tracing::info;
use url::Url;

pub struct Browser {
    client: Client,
}

#[derive(Debug, Clone)]
pub struct FetchResult {
    #[allow(dead_code)]
    pub url: String,
    pub title: Option<String>,
    pub text: String,
    #[allow(dead_code)]
    pub html: String,
    pub links: Vec<String>,
    pub images: Vec<String>,
    pub status_code: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

impl Browser {
    pub async fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()?;

        Ok(Self { client })
    }

    pub async fn fetch(&self, url: &str, extract_text: bool) -> Result<FetchResult> {
        let url = Url::parse(url).map_err(|e| anyhow!("Invalid URL: {}", e))?;

        info!("Fetching: {}", url);

        let response = self.client.get(url.as_str()).send().await?;
        let status_code = response.status().as_u16();

        if !response.status().is_success() {
            return Err(anyhow!(
                "HTTP error {}: {}",
                response.status(),
                response.status().canonical_reason().unwrap_or("Unknown")
            ));
        }

        let html = response.text().await?;
        let document = Html::parse_document(&html);

        let title = self.extract_title(&document);
        let links = self.extract_links(&document, &url);
        let images = self.extract_images(&document, &url);

        let text = if extract_text {
            self.extract_text_content(&document)
        } else {
            String::new()
        };

        Ok(FetchResult {
            url: url.to_string(),
            title,
            text,
            html,
            links,
            images,
            status_code,
        })
    }

    fn extract_title(&self, document: &Html) -> Option<String> {
        let title_selector = Selector::parse("title").ok()?;
        document
            .select(&title_selector)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
    }

    fn extract_text_content(&self, document: &Html) -> String {
        // Remove script and style elements
        let _script_selector = Selector::parse("script, style, noscript").unwrap();
        let mut text = String::new();

        // Get body or entire document
        let body_selector = Selector::parse("body").unwrap();
        let content = document.select(&body_selector).next();

        if let Some(body) = content {
            for node in body.children() {
                if let Some(element) = node.value().as_element() {
                    let tag_name = element.name();
                    if tag_name == "script" || tag_name == "style" || tag_name == "noscript" {
                        continue;
                    }
                    let Some(text_node) = scraper::ElementRef::wrap(node) else {
                        continue;
                    };
                    let node_text = text_node.text().collect::<String>();
                    if !node_text.trim().is_empty() {
                        text.push_str(&node_text);
                        text.push(' ');
                    }
                } else if let Some(text_node) = node.value().as_text() {
                    let t = text_node.trim();
                    if !t.is_empty() {
                        text.push_str(t);
                        text.push(' ');
                    }
                }
            }
        }

        // Clean up the text
        text = text
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        // Remove excessive whitespace
        let re = regex::Regex::new(r"\s+").ok();
        if let Some(re) = re {
            text = re.replace_all(&text, " ").to_string();
        }

        text.trim().to_string()
    }

    fn extract_links(&self, document: &Html, base_url: &Url) -> Vec<String> {
        let selector = Selector::parse("a[href]").unwrap();
        let mut links = Vec::new();

        for element in document.select(&selector) {
            let Some(href) = element.value().attr("href") else {
                continue;
            };
            let Ok(url) = base_url.join(href) else {
                continue;
            };
            let url_str = url.to_string();
            if url_str.starts_with("http") && !links.contains(&url_str) {
                links.push(url_str);
            }
        }

        links
    }

    fn extract_images(&self, document: &Html, base_url: &Url) -> Vec<String> {
        let selector = Selector::parse("img[src]").unwrap();
        let mut images = Vec::new();

        for element in document.select(&selector) {
            let Some(src) = element.value().attr("src") else {
                continue;
            };
            let Ok(url) = base_url.join(src) else {
                continue;
            };
            let url_str = url.to_string();
            if !images.contains(&url_str) {
                images.push(url_str);
            }
        }

        images
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResultItem>> {
        // Use DuckDuckGo HTML search as it's more accessible
        let search_url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        info!("Searching DuckDuckGo: {}", query);

        let response = self.client.get(&search_url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Search failed: HTTP {}", response.status()));
        }

        let html = response.text().await?;
        let document = Html::parse_document(&html);

        let mut results = Vec::new();

        // DuckDuckGo HTML results
        let result_selector = Selector::parse(".result").unwrap();
        let title_selector = Selector::parse(".result__title").unwrap();
        let snippet_selector = Selector::parse(".result__snippet").unwrap();
        let url_selector = Selector::parse(".result__url").unwrap();

        for (i, element) in document.select(&result_selector).enumerate() {
            if i >= limit {
                break;
            }

            let title = element
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            let snippet = element
                .select(&snippet_selector)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            let url = element
                .select(&url_selector)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .filter(|u| !u.is_empty())
                .map(|u| {
                    if u.starts_with("http") {
                        u
                    } else {
                        format!("https://{}", u)
                    }
                })
                .unwrap_or_else(|| "https://duckduckgo.com".to_string());

            if !title.is_empty() {
                results.push(SearchResultItem {
                    title,
                    url,
                    snippet,
                });
            }
        }

        if results.is_empty() {
            // Fallback: try to parse results differently
            let link_selector = Selector::parse(".links_main").unwrap();
            for (i, element) in document.select(&link_selector).enumerate() {
                if i >= limit {
                    break;
                }

                let a_selector = Selector::parse("a").unwrap();
                if let Some(link) = element.select(&a_selector).next() {
                    let title = link.text().collect::<String>().trim().to_string();
                    let href = link
                        .value()
                        .attr("href")
                        .map(|h| {
                            if h.starts_with("http") {
                                h.to_string()
                            } else {
                                format!("https://duckduckgo.com{}", h)
                            }
                        })
                        .unwrap_or_default();

                    let snippet = element
                        .text()
                        .collect::<String>()
                        .replace(&title, "")
                        .trim()
                        .to_string();

                    if !title.is_empty() {
                        results.push(SearchResultItem {
                            title,
                            url: href,
                            snippet,
                        });
                    }
                }
            }
        }

        info!("Found {} search results", results.len());
        Ok(results)
    }
}
