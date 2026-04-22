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
    pub links: Vec<String>,
    #[allow(dead_code)]
    pub images: Vec<String>,
    #[allow(dead_code)]
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

        // Skip non-HTML content early to avoid parsing binary data
        if let Some(ct) = response.headers().get("content-type") {
            let ct_str = ct.to_str().unwrap_or("");
            if should_skip_content_type(ct_str) {
                return Err(anyhow!("Skipping non-HTML content type: {}", ct_str));
            }
        }

        let html = response.text().await?;
        let document = Html::parse_document(&html);

        let title = Self::extract_title(&document);
        let links = Self::extract_links(&document, &url);
        let images = Self::extract_images(&document, &url);
        let text = if extract_text {
            Self::extract_text_content(&document)
        } else {
            String::new()
        };

        Ok(FetchResult {
            url: url.to_string(),
            title,
            text,
            links,
            images,
            status_code,
        })
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResultItem>> {
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

        // DuckDuckGo HTML results are in .result elements
        let result_selector = Selector::parse(".result").unwrap();
        let title_selector = Selector::parse(".result__a").unwrap();
        let snippet_selector = Selector::parse(".result__snippet").unwrap();

        let mut results = Vec::new();

        for element in document.select(&result_selector) {
            let title_elem = element.select(&title_selector).next();
            let snippet_elem = element.select(&snippet_selector).next();

            if let Some(title_elem) = title_elem {
                let title = title_elem.text().collect::<String>().trim().to_string();
                let href = title_elem
                    .value()
                    .attr("href")
                    .unwrap_or("")
                    .trim()
                    .to_string();

                let snippet = snippet_elem
                    .map(|e| e.text().collect::<String>().trim().to_string())
                    .unwrap_or_default();

                // DuckDuckGo redirects through their own URL - extract real URL if needed
                let url = if href.starts_with("/") || href.starts_with("http") {
                    if href.contains("duckduckgo.com/l/") {
                        // Extract URL from redirect
                        Self::extract_url_from_ddg_redirect(&href).unwrap_or(href)
                    } else {
                        href
                    }
                } else {
                    continue;
                };

                if !title.is_empty() {
                    results.push(SearchResultItem {
                        title,
                        url,
                        snippet,
                    });
                }
            }

            if results.len() >= limit {
                break;
            }
        }

        info!("Found {} search results", results.len());
        Ok(results)
    }

    // ── HTML extraction helpers ──────────────────────────────────────────

    fn extract_title(document: &Html) -> Option<String> {
        let title_selector = Selector::parse("title").ok()?;
        document
            .select(&title_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
    }

    fn extract_text_content(document: &Html) -> String {
        let html_str = document.html();
        let decorator = html2text::render::TrivialDecorator::new();
        html2text::from_read_with_decorator(html_str.as_bytes(), usize::MAX, decorator)
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    fn extract_links(document: &Html, base_url: &Url) -> Vec<String> {
        let selector = Selector::parse("a[href]").unwrap();
        let mut links = Vec::new();

        for element in document.select(&selector) {
            if let Some(href) = element.value().attr("href")
                && let Ok(url) = base_url.join(href)
            {
                let url_str = url.to_string();
                if url_str.starts_with("http") && !links.contains(&url_str) {
                    links.push(url_str);
                }
            }
        }

        links
    }

    fn extract_images(document: &Html, base_url: &Url) -> Vec<String> {
        let selector = Selector::parse("img[src]").unwrap();
        let mut images = Vec::new();

        for element in document.select(&selector) {
            if let Some(src) = element.value().attr("src")
                && let Ok(url) = base_url.join(src)
            {
                let url_str = url.to_string();
                if !images.contains(&url_str) {
                    images.push(url_str);
                }
            }
        }

        images
    }

    // ── Utility ────────────────────────────────────────────────────────

    fn extract_url_from_ddg_redirect(href: &str) -> Option<String> {
        // DDG redirect URLs contain the real URL as a query parameter
        if let Some(pos) = href.find("uddg=") {
            let encoded = &href[pos + 5..];
            return urlencoding::decode(encoded).ok().map(|s| s.to_string());
        }
        None
    }
}

/// Skip non-HTML content types that can't be usefully parsed.
fn should_skip_content_type(content_type: &str) -> bool {
    let ct = content_type.to_lowercase();
    ct.contains("image/")
        || ct.contains("video/")
        || ct.contains("audio/")
        || ct.contains("application/pdf")
        || ct.contains("application/zip")
        || ct.contains("application/octet-stream")
        || ct.contains("application/rss+xml")
        || ct.contains("application/xml")
        || ct.contains("text/xml")
}
