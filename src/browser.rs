use anyhow::{Result, anyhow};
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartSearchResult {
    pub found: bool,
    pub query: String,
    pub source_url: String,
    pub matches: Vec<String>,
    pub context: String,
}

// ── Internal helpers for smart_search ────────────────────────────────────────

/// A hyperlink together with its visible anchor text.
#[derive(Debug, Clone)]
struct LinkWithText {
    url: String,
    anchor_text: String,
}

/// Priority-queue entry used by the best-first crawler.
#[derive(Debug, Clone, Eq, PartialEq)]
struct CrawlEntry {
    priority: i32,
    url: String,
}

impl Ord for CrawlEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Max-heap: highest priority popped first.
        self.priority.cmp(&other.priority)
    }
}

impl PartialOrd for CrawlEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Semantic concept graph: each concept maps to related terms that
/// websites commonly use in URLs, headings, and navigation.
/// This replaces the old hardcoded `match` arms with a data structure
/// that's easy to extend.
const CONCEPT_GRAPH: &[(&str, &[&str])] = &[
    // ── Contact / reachability ───────────────────────────────────────────
    (
        "phone",
        &[
            "contact",
            "tel",
            "phone",
            "telephone",
            "mobile",
            "call",
            "fax",
            "imprint",
            "impressum",
            "kontakt",
            "reach",
            "helpline",
        ],
    ),
    (
        "telephone",
        &[
            "contact",
            "tel",
            "phone",
            "telephone",
            "mobile",
            "call",
            "fax",
            "imprint",
            "impressum",
            "kontakt",
            "reach",
        ],
    ),
    (
        "tel",
        &[
            "contact",
            "tel",
            "phone",
            "telephone",
            "mobile",
            "call",
            "fax",
            "imprint",
            "impressum",
            "kontakt",
        ],
    ),
    (
        "mobile",
        &[
            "contact",
            "tel",
            "phone",
            "telephone",
            "mobile",
            "cell",
            "imprint",
            "impressum",
            "kontakt",
        ],
    ),
    (
        "cell",
        &["contact", "tel", "phone", "telephone", "mobile", "cell"],
    ),
    (
        "call",
        &["contact", "tel", "phone", "telephone", "call", "helpline"],
    ),
    (
        "fax",
        &["contact", "tel", "phone", "fax", "imprint", "impressum"],
    ),
    ("nummer", &["contact", "tel", "phone", "kontakt", "nummer"]),
    (
        "telefon",
        &["contact", "tel", "phone", "telephone", "kontakt", "telefon"],
    ),
    ("number", &["contact", "tel", "phone", "number"]),
    // ── Email / post ─────────────────────────────────────────────────────
    (
        "email",
        &[
            "contact",
            "email",
            "mail",
            "imprint",
            "impressum",
            "kontakt",
            "reach",
        ],
    ),
    (
        "mail",
        &[
            "contact",
            "email",
            "mail",
            "imprint",
            "impressum",
            "kontakt",
            "reach",
        ],
    ),
    (
        "post",
        &[
            "contact",
            "email",
            "mail",
            "imprint",
            "impressum",
            "kontakt",
        ],
    ),
    // ── Contact in general ───────────────────────────────────────────────
    (
        "contact",
        &[
            "contact",
            "about",
            "imprint",
            "impressum",
            "kontakt",
            "reach",
            "info",
            "support",
        ],
    ),
    (
        "reach",
        &[
            "contact",
            "about",
            "imprint",
            "impressum",
            "kontakt",
            "reach",
            "info",
        ],
    ),
    (
        "kontakt",
        &[
            "contact",
            "about",
            "imprint",
            "impressum",
            "kontakt",
            "reach",
            "info",
        ],
    ),
    // ── Location / address ───────────────────────────────────────────────
    (
        "address",
        &[
            "contact",
            "address",
            "location",
            "map",
            "imprint",
            "impressum",
            "kontakt",
            "about",
            "find",
            "directions",
        ],
    ),
    (
        "location",
        &[
            "contact",
            "address",
            "location",
            "map",
            "imprint",
            "impressum",
            "kontakt",
            "about",
            "find",
            "directions",
        ],
    ),
    (
        "where",
        &[
            "contact",
            "address",
            "location",
            "map",
            "about",
            "find",
            "directions",
        ],
    ),
    (
        "office",
        &["contact", "address", "location", "map", "about", "office"],
    ),
    (
        "adresse",
        &[
            "contact",
            "address",
            "adresse",
            "standort",
            "kontakt",
            "impressum",
        ],
    ),
    (
        "standort",
        &[
            "contact",
            "address",
            "standort",
            "kontakt",
            "impressum",
            "map",
        ],
    ),
    // ── Pricing ──────────────────────────────────────────────────────────
    (
        "price",
        &[
            "price",
            "pricing",
            "cost",
            "plan",
            "tier",
            "subscription",
            "buy",
            "purchase",
            "angebot",
            "angebot",
        ],
    ),
    (
        "pricing",
        &[
            "price",
            "pricing",
            "cost",
            "plan",
            "tier",
            "subscription",
            "buy",
            "purchase",
            "angebot",
        ],
    ),
    (
        "cost",
        &[
            "price",
            "pricing",
            "cost",
            "plan",
            "tier",
            "subscription",
            "buy",
            "purchase",
        ],
    ),
    (
        "fee",
        &[
            "price",
            "pricing",
            "cost",
            "fee",
            "plan",
            "tier",
            "subscription",
        ],
    ),
    (
        "preis",
        &["price", "pricing", "cost", "preis", "angebot", "kaufen"],
    ),
    // ── About / company ──────────────────────────────────────────────────
    (
        "about",
        &[
            "about",
            "team",
            "company",
            "who-we-are",
            "uber-uns",
            "unternehmen",
        ],
    ),
    (
        "company",
        &[
            "about",
            "team",
            "company",
            "who-we-are",
            "uber-uns",
            "unternehmen",
        ],
    ),
    (
        "team",
        &[
            "about",
            "team",
            "company",
            "who-we-are",
            "uber-uns",
            "unternehmen",
            "people",
            "staff",
        ],
    ),
    // ── Technical ────────────────────────────────────────────────────────
    (
        "backup",
        &[
            "backup",
            "archive",
            "recovery",
            "restore",
            "sicherung",
            "sicherungskopie",
        ],
    ),
    (
        "security",
        &[
            "security",
            "ssl",
            "tls",
            "https",
            "privacy",
            "datenschutz",
            "sicherheit",
        ],
    ),
    (
        "privacy",
        &[
            "privacy",
            "datenschutz",
            "gdpr",
            "dsgvo",
            "security",
            "policy",
        ],
    ),
    (
        "api",
        &[
            "api",
            "docs",
            "documentation",
            "developer",
            "reference",
            "endpoint",
        ],
    ),
    (
        "docs",
        &[
            "docs",
            "documentation",
            "guide",
            "tutorial",
            "help",
            "wiki",
            "readme",
        ],
    ),
    // ── Support ──────────────────────────────────────────────────────────
    (
        "help",
        &[
            "help",
            "support",
            "faq",
            "docs",
            "documentation",
            "guide",
            "tutorial",
        ],
    ),
    (
        "support",
        &["help", "support", "faq", "contact", "docs", "ticket"],
    ),
    ("faq", &["help", "support", "faq", "question", "answer"]),
];

/// Score threshold for early termination: if a page scores this high,
/// we consider it a near-perfect match and stop crawling.
const EARLY_TERMINATION_SCORE: i32 = 800;

// ── Browser implementation ───────────────────────────────────────────────────

impl Browser {
    pub async fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
                 AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/120.0.0.0 Safari/537.36",
            )
            .build()?;

        Ok(Self { client })
    }

    // ── Public API ───────────────────────────────────────────────────────

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
            html,
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

        let mut results = Vec::new();

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

    pub async fn smart_search(
        &self,
        start_url: &str,
        query: &str,
        max_depth: usize,
    ) -> Result<SmartSearchResult> {
        info!("Smart search for '{}' on {}", query, start_url);

        let keywords = Self::expand_query_keywords(query);
        info!("Expanded keywords: {:?}", keywords);

        let base_domain = Url::parse(start_url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_default();

        // Phase 0: Try sitemap.xml first — gives us the full site structure
        // without any crawling.
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: BinaryHeap<CrawlEntry> = BinaryHeap::new();

        if let Ok(sitemap_urls) = self.fetch_sitemap(start_url).await {
            info!(
                "Sitemap found with {} URLs — seeding queue",
                sitemap_urls.len()
            );
            for sitemap_url in &sitemap_urls {
                let norm = Self::normalize_url(sitemap_url);
                if norm.contains(&base_domain) && !visited.contains(&norm) {
                    // Score each sitemap URL by relevance so we crawl the best first
                    let score = Self::score_url_only(&norm, &keywords);
                    queue.push(CrawlEntry {
                        priority: score,
                        url: norm,
                    });
                }
            }
        }

        // Always include the start URL as fallback
        let start_norm = Self::normalize_url(start_url);
        if !visited.contains(&start_norm) {
            queue.push(CrawlEntry {
                priority: 0,
                url: start_norm,
            });
        }

        let mut best_match: Option<SmartSearchResult> = None;
        let mut best_score = 0i32;

        for _ in 0..max_depth {
            let Some(entry) = queue.pop() else {
                break;
            };
            let norm = Self::normalize_url(&entry.url);
            if visited.contains(&norm) {
                continue;
            }
            visited.insert(norm.clone());

            info!("Crawling (priority {}): {}", entry.priority, entry.url);

            let result = match self.fetch(&entry.url, true).await {
                Ok(r) => r,
                Err(e) => {
                    info!("  Failed to fetch: {}", e);
                    continue;
                }
            };

            // Parse document once for heading/meta scoring
            let document = Html::parse_document(&result.html);
            let headings = Self::extract_headings(&document);
            let meta_desc = Self::extract_meta_description(&document);
            let meta_keywords = Self::extract_meta_keywords(&document);

            let score = Self::score_page(
                &entry.url,
                &result.text,
                &headings,
                &meta_desc,
                &meta_keywords,
                &keywords,
                query,
            );
            info!("  Page score: {}", score);

            if score > best_score {
                best_score = score;

                // Build a focused snippet around the most relevant part
                let snippet = Self::extract_relevant_snippet(&result.text, &keywords, query);

                // Enrich context with any structured data we detected
                let phones = Self::detect_phone_numbers(&result.text);
                let emails = Self::detect_emails(&result.text);
                let mut context = snippet;
                if !phones.is_empty() {
                    context.push_str(&format!(
                        "\n\nDetected phone numbers: {}",
                        phones.join(", ")
                    ));
                }
                if !emails.is_empty() {
                    context.push_str(&format!(
                        "\n\nDetected email addresses: {}",
                        emails.join(", ")
                    ));
                }

                best_match = Some(SmartSearchResult {
                    found: true,
                    query: query.to_string(),
                    source_url: entry.url.clone(),
                    matches: vec![result.text.clone()],
                    context,
                });
                info!("  New best match (score {})!", score);

                // Early termination: if we're very confident, stop crawling
                if score >= EARLY_TERMINATION_SCORE {
                    info!(
                        "  Score {} >= {} — early termination!",
                        score, EARLY_TERMINATION_SCORE
                    );
                    break;
                }
            }

            // Enqueue discovered links, prioritised by relevance.
            let parsed_url =
                Url::parse(&entry.url).unwrap_or_else(|_| Url::parse("http://localhost").unwrap());
            let links = Self::extract_links_with_anchor_text(&document, &parsed_url);

            for link in links {
                let norm = Self::normalize_url(&link.url);
                if !visited.contains(&norm) && link.url.contains(&base_domain) {
                    let link_score = Self::score_link(&link, &keywords);
                    queue.push(CrawlEntry {
                        priority: link_score,
                        url: norm,
                    });
                }
            }
        }

        if let Some(result) = best_match {
            Ok(result)
        } else {
            Ok(SmartSearchResult {
                found: false,
                query: query.to_string(),
                source_url: start_url.to_string(),
                matches: vec![],
                context: String::new(),
            })
        }
    }

    // ── HTML extraction helpers ──────────────────────────────────────────

    fn extract_title(document: &Html) -> Option<String> {
        let title_selector = Selector::parse("title").ok()?;
        document
            .select(&title_selector)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
    }

    /// Extract text using html2text for much cleaner output than raw DOM walking.
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

    fn extract_images(document: &Html, base_url: &Url) -> Vec<String> {
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

    /// Extract all h1–h3 heading texts from the page.
    fn extract_headings(document: &Html) -> Vec<String> {
        let selector = Selector::parse("h1, h2, h3").unwrap();
        document
            .select(&selector)
            .map(|el| el.text().collect::<String>().trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    }

    /// Extract `<meta name="description" content="…">`.
    fn extract_meta_description(document: &Html) -> Option<String> {
        let selector = Selector::parse("meta[name='description']").ok()?;
        document
            .select(&selector)
            .next()
            .and_then(|el| el.value().attr("content").map(|s| s.trim().to_string()))
    }

    /// Extract `<meta name="keywords" content="…">`.
    fn extract_meta_keywords(document: &Html) -> Vec<String> {
        let selector = match Selector::parse("meta[name='keywords']") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        document
            .select(&selector)
            .next()
            .and_then(|el| el.value().attr("content").map(|s| s.to_string()))
            .map(|s| {
                s.split(',')
                    .map(|k| k.trim().to_lowercase())
                    .filter(|k| !k.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Like `extract_links` but also captures the visible anchor text.
    fn extract_links_with_anchor_text(document: &Html, base_url: &Url) -> Vec<LinkWithText> {
        let selector = Selector::parse("a[href]").unwrap();
        let mut seen = HashSet::new();
        let mut links = Vec::new();

        for element in document.select(&selector) {
            let Some(href) = element.value().attr("href") else {
                continue;
            };
            let Ok(url) = base_url.join(href) else {
                continue;
            };
            let url_str = url.to_string();
            if !url_str.starts_with("http") || seen.contains(&url_str) {
                continue;
            }
            seen.insert(url_str.clone());
            let anchor_text = element.text().collect::<String>().trim().to_lowercase();
            links.push(LinkWithText {
                url: url_str,
                anchor_text,
            });
        }

        links
    }

    // ── Smart-search intelligence ────────────────────────────────────────

    /// Simple English suffix-stripping stemmer.
    /// Not perfect, but handles the common cases:
    ///   "phones" → "phone", "backups" → "backup", "running" → "run"
    fn stem(word: &str) -> String {
        let w = word.to_lowercase();

        // Order matters: try longest suffixes first
        for suffix in &[
            "ication", "ational", "fulness", "ousness", "iveness", "ation", "ening", "ments",
            "ingly",
        ] {
            if w.ends_with(suffix) && w.len() > suffix.len() + 2 {
                return w[..w.len() - suffix.len()].to_string();
            }
        }

        for suffix in &[
            "ies",  // directories → director (imperfect but close enough)
            "ting", // computing → comput
            "ness", // usefulness → useful
            "ment", // development → develop
            "ance", // performance → perform
            "ence", // reference → refer
            "able", // configurable → configur
            "ible", // accessible → access
            "tion", // information → informa (imperfect)
            "sion", // decision → deci
            "ally", // basically → basic
            "ical", // technical → techn
            "full", // helpful → help
            "less", // useless → use
            "ing",  // running → runn (close enough for matching)
            "ous",  // dangerous → danger
            "ive",  // effective → effect
            "ful",  // helpful → help
            "ity",  // security → secur
            "ism",  // capitalism → capital
            "ist",  // specialist → special
            "ize",  // optimize → optim
            "ise",  // optimise → optim
            "ify",  // simplify → simpl
            "ate",  // activate → activ
            "ure",  // feature → feat
            "dom",  // freedom → free
            "ship", // leadership → leader
        ] {
            if w.ends_with(suffix) && w.len() > suffix.len() + 2 {
                return w[..w.len() - suffix.len()].to_string();
            }
        }

        // Plurals (after longer suffixes so "categories" isn't caught here)
        if w.ends_with("es") && w.len() > 4 && !w.ends_with("sses") {
            return w[..w.len() - 2].to_string();
        }
        if w.ends_with('s') && w.len() > 3 && !w.ends_with("ss") && !w.ends_with("us") {
            return w[..w.len() - 1].to_string();
        }

        w
    }

    /// Expand the raw query into a richer set of keywords by:
    ///   1. Adding stemmed forms of each word
    ///   2. Looking up related terms in the concept graph
    fn expand_query_keywords(query: &str) -> Vec<String> {
        let base: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| w.len() > 2)
            .collect();

        let mut expanded: Vec<String> = base.clone();

        // Add stemmed versions of each base word
        for word in &base {
            let stemmed = Self::stem(word);
            if stemmed != *word && !expanded.contains(&stemmed) {
                expanded.push(stemmed);
            }
        }

        // Look up each base word (and its stem) in the concept graph
        let mut to_lookup = base.clone();
        for word in &base {
            let stemmed = Self::stem(word);
            if !to_lookup.contains(&stemmed) {
                to_lookup.push(stemmed);
            }
        }

        for word in &to_lookup {
            for &(concept, related) in CONCEPT_GRAPH {
                if word == concept {
                    for term in related {
                        if !expanded.contains(&term.to_string()) {
                            expanded.push(term.to_string());
                        }
                    }
                }
            }
        }

        expanded
    }

    /// Detect international phone-number patterns (e.g. +49 159 0268 1236).
    fn detect_phone_numbers(text: &str) -> Vec<String> {
        let re = Regex::new(r"\+\d[\d\s\-\(\)/]{7,20}").unwrap();
        re.find_iter(text)
            .map(|m| m.as_str().trim().to_string())
            .filter(|p| {
                let digits: String = p.chars().filter(|c| c.is_ascii_digit()).collect();
                digits.len() >= 10
            })
            .collect()
    }

    /// Detect email patterns.
    fn detect_emails(text: &str) -> Vec<String> {
        let re = Regex::new(r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}").unwrap();
        re.find_iter(text).map(|m| m.as_str().to_string()).collect()
    }

    /// Score a fetched page for relevance to the query.
    /// Takes into account:
    ///   - Body text keyword frequency
    ///   - URL keyword matches
    ///   - Heading keyword matches (weighted higher)
    ///   - Meta description/keyword matches
    ///   - Structured data (phone numbers, emails) when query-relevant
    fn score_page(
        url: &str,
        text: &str,
        headings: &[String],
        meta_desc: &Option<String>,
        meta_keywords: &[String],
        keywords: &[String],
        query: &str,
    ) -> i32 {
        let text_lower = text.to_lowercase();
        let url_lower = url.to_lowercase();

        // 1) Keyword hits in body text.
        let matching_words = keywords
            .iter()
            .filter(|k| text_lower.contains(k.as_str()))
            .count();
        let mut score = (matching_words as i32) * 100;

        // 2) Keyword hits in the URL itself.
        let url_hits = keywords
            .iter()
            .filter(|k| url_lower.contains(k.as_str()))
            .count();
        score += (url_hits as i32) * 200;

        // 3) Headings — much stronger signal than body text.
        //    A heading "Contact Us" is a very clear page topic indicator.
        for heading in headings {
            let h_lower = heading.to_lowercase();
            let heading_hits = keywords
                .iter()
                .filter(|k| h_lower.contains(k.as_str()))
                .count();
            score += (heading_hits as i32) * 300;
        }

        // 4) Meta description — curated summary, strong relevance signal.
        if let Some(desc) = meta_desc {
            let desc_lower = desc.to_lowercase();
            let desc_hits = keywords
                .iter()
                .filter(|k| desc_lower.contains(k.as_str()))
                .count();
            score += (desc_hits as i32) * 250;
        }

        // 5) Meta keywords — explicitly tagged by the site author.
        let meta_hits = keywords
            .iter()
            .filter(|k| meta_keywords.contains(k))
            .count();
        score += (meta_hits as i32) * 300;

        // 6) Structured-data bonus: only when the query is specifically about
        //    this kind of data. No general bonus — a page having a phone
        //    number is irrelevant when the query is about "backup".
        let query_lower = query.to_lowercase();
        let phones = Self::detect_phone_numbers(text);
        let emails = Self::detect_emails(text);

        if !phones.is_empty() {
            let phone_terms = [
                "phone", "tel", "call", "mobile", "fax", "number", "contact", "reach", "telefon",
                "nummer",
            ];
            if phone_terms.iter().any(|t| query_lower.contains(t)) {
                score += 500;
            }
        }

        if !emails.is_empty() {
            let email_terms = ["email", "mail", "contact", "reach", "post"];
            if email_terms.iter().any(|t| query_lower.contains(t)) {
                score += 500;
            }
        }

        score
    }

    /// Score a URL alone (without fetching) — used for sitemap entries.
    fn score_url_only(url: &str, keywords: &[String]) -> i32 {
        let url_lower = url.to_lowercase();
        keywords
            .iter()
            .filter(|k| url_lower.contains(k.as_str()))
            .map(|_| 200)
            .sum()
    }

    /// Extract a focused snippet around the most relevant part of the text.
    /// Instead of dumping the entire page, returns the paragraph(s) that
    /// contain the most keyword hits.
    fn extract_relevant_snippet(text: &str, keywords: &[String], query: &str) -> String {
        // Split into paragraphs
        let paragraphs: Vec<&str> = text
            .split("\n\n")
            .flat_map(|p| p.split('\n'))
            .map(|p| p.trim())
            .filter(|p| p.len() > 20) // skip very short fragments
            .collect();

        if paragraphs.is_empty() {
            // Fallback: return first 2000 chars
            return text.chars().take(2000).collect();
        }

        // Score each paragraph by keyword density
        let mut scored: Vec<(i32, &str)> = paragraphs
            .iter()
            .map(|p| {
                let p_lower = p.to_lowercase();
                let mut s = 0i32;
                for k in keywords {
                    // Count occurrences, not just presence
                    s += (p_lower.matches(k.as_str()).count() as i32) * 100;
                }
                // Also match the full query phrase
                if p_lower.contains(query) {
                    s += 500;
                }
                (s, *p)
            })
            .filter(|(s, _)| *s > 0)
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));

        if scored.is_empty() {
            // No paragraph matched — return first few
            let combined: Vec<&str> = paragraphs.into_iter().take(3).collect();
            return combined.join("\n\n");
        }

        // Take top paragraphs up to a reasonable length
        let mut result = String::new();
        for (_, p) in scored.iter().take(5) {
            if result.len() > 3000 {
                break;
            }
            if !result.is_empty() {
                result.push_str("\n\n");
            }
            result.push_str(p);
        }

        if result.is_empty() {
            text.chars().take(2000).collect()
        } else {
            result
        }
    }

    /// Normalize a URL so that equivalent URLs compare equal:
    ///   - strip trailing '/' after the path
    ///   - lowercase the host
    ///   - drop fragment
    fn normalize_url(raw: &str) -> String {
        let Ok(mut u) = Url::parse(raw) else {
            return raw.to_string();
        };
        u.set_fragment(None);
        if let Some(host) = u.host_str() {
            let _ = u.set_host(Some(&host.to_lowercase()));
        }
        let mut s = u.to_string();
        while s.ends_with('/') && !s.ends_with("//") {
            s.pop();
        }
        s
    }

    /// Try to fetch and parse sitemap.xml from the same domain.
    /// Returns a list of URLs found in the sitemap.
    async fn fetch_sitemap(&self, start_url: &str) -> Result<Vec<String>> {
        let base = Url::parse(start_url)?;
        let sitemap_url = base.join("/sitemap.xml")?;

        info!("Checking for sitemap at: {}", sitemap_url);

        let response = self.client.get(sitemap_url.as_str()).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("No sitemap found"));
        }

        let xml = response.text().await?;
        let document = Html::parse_document(&xml);

        // Standard sitemap.xml uses <loc> tags
        let loc_selector = Selector::parse("loc").unwrap();
        let urls: Vec<String> = document
            .select(&loc_selector)
            .filter_map(|el| {
                let text = el.text().collect::<String>().trim().to_string();
                if text.starts_with("http") {
                    Some(text)
                } else {
                    None
                }
            })
            .collect();

        if urls.is_empty() {
            return Err(anyhow!("Sitemap has no URLs"));
        }

        Ok(urls)
    }

    /// Score a discovered link for crawl-priority.
    /// Anchor text is often more informative than the URL path.
    fn score_link(link: &LinkWithText, keywords: &[String]) -> i32 {
        let mut score = 0i32;
        let url_lower = link.url.to_lowercase();
        let anchor_lower = &link.anchor_text;

        for keyword in keywords {
            if url_lower.contains(keyword.as_str()) {
                score += 200;
            }
            if anchor_lower.contains(keyword.as_str()) {
                score += 300;
            }
        }

        score
    }
}
