import httpx
from bs4 import BeautifulSoup

from ._utils import sanitize_url


def web_search(query: str, num_results: int = 8) -> str:
    """
    Search the web using DuckDuckGo and return results.

    Args:
        query: Search query string
        num_results: Number of results to return (default: 8, max: 10)

    Returns:
        Formatted search results with titles, URLs, and snippets
    """
    try:
        # Clean and validate query
        clean_query = sanitize_url(query)
        if not clean_query:
            return "Error: Empty search query"

        # Limit num_results
        num_results = min(max(num_results, 1), 10)

        # Build search URL
        url = f"https://html.duckduckgo.com/html/?q={clean_query.replace(' ', '+')}"
        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            "Accept-Language": "en-US,en;q=0.9",
        }

        with httpx.Client(timeout=15, headers=headers) as client:
            response = client.get(url)
            response.raise_for_status()

            soup = BeautifulSoup(response.content, "html.parser")
            results = []

            # Extract results
            for i, result in enumerate(soup.select(".result")[:num_results], 1):
                try:
                    # Title
                    title_elem = result.select_one(".result__title")
                    title = (
                        title_elem.get_text(strip=True) if title_elem else "No title"
                    )

                    # URL
                    link_elem = result.select_one(".result__title a")
                    href = str(link_elem.get("href", "") if link_elem else "")
                    # DuckDuckGo uses redirect URLs, extract actual URL
                    if href.startswith("//"):
                        href = "https:" + href
                    elif href.startswith("/"):
                        href = "https://duckduckgo.com" + href

                    # Snippet
                    snippet_elem = result.select_one(".result__snippet")
                    snippet = snippet_elem.get_text(strip=True) if snippet_elem else ""

                    # Format result
                    result_text = f"[{i}] {title}"
                    if href:
                        result_text += f"\n    URL: {href}"
                    if snippet:
                        snippet_clean = snippet[:300].replace("\n", " ")
                        result_text += f"\n    {snippet_clean}"

                    results.append(result_text)

                except Exception:
                    continue  # Skip malformed results

            if not results:
                return "No search results found. The search engine may be blocking automated requests."

            return f"Search results for '{clean_query}':\n\n" + "\n\n".join(results)

    except httpx.TimeoutException:
        return "Error: Search timed out (15s limit)"
    except httpx.HTTPStatusError as e:
        return f"Error: Search failed with status {e.response.status_code}"
    except Exception as e:
        return f"Error searching: {e}"
