import httpx
from bs4 import BeautifulSoup

from ._utils import sanitize_url


def web_fetch(url: str, format: str = "markdown") -> str:
    """
    Fetch content from a URL and return in specified format.

    Args:
        url: The URL to fetch (must start with http:// or https://)
        format: Output format - "markdown" (default), "text", or "html"

    Returns:
        Content in requested format, truncated to 8000 chars
    """
    MAX_SIZE = 5 * 1024 * 1024  # 5MB
    DEFAULT_TIMEOUT = 30

    try:
        # Validate URL
        url = sanitize_url(url)
        if not url.startswith(("http://", "https://")):
            return "Error: URL must start with http:// or https://"

        # Build headers based on format
        accept_headers = {
            "markdown": "text/markdown, text/html, text/plain, */*",
            "text": "text/plain, text/html, */*",
            "html": "text/html, */*",
        }

        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.0.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.0",
            "Accept": accept_headers.get(format, "text/html, */*"),
            "Accept-Language": "en-US,en;q=0.9",
        }

        # Fetch content
        with httpx.Client(timeout=DEFAULT_TIMEOUT, follow_redirects=True) as client:
            response = client.get(url, headers=headers)

            # Check content length
            content_length = response.headers.get("content-length")
            if content_length and int(content_length) > MAX_SIZE:
                return "Error: Response too large (exceeds 5MB limit)"

            content = response.text
            if len(content.encode("utf-8")) > MAX_SIZE:
                return "Error: Response too large (exceeds 5MB limit)"

            content_type = response.headers.get("content-type", "").lower()

            # Handle based on requested format
            if format == "html":
                return content[:8000]

            elif format == "text":
                if "text/html" in content_type:
                    soup = BeautifulSoup(content, "html.parser")
                    # Remove script/style elements
                    for tag in soup(["script", "style", "noscript", "iframe"]):
                        tag.decompose()
                    text = soup.get_text(separator="\n", strip=True)
                    # Clean up whitespace
                    lines = (line.strip() for line in text.splitlines())
                    text = "\n".join(line for line in lines if line)
                    return text[:8000]
                return content[:8000]

            elif format == "markdown":
                if "text/html" in content_type:
                    soup = BeautifulSoup(content, "html.parser")

                    # Convert common HTML to markdown-like formatting
                    md_parts = []

                    # Title
                    title = soup.find("title")
                    if title:
                        md_parts.append(f"# {title.get_text(strip=True)}\n")

                    # Headings
                    for i in range(1, 7):
                        for heading in soup.find_all(f"h{i}"):
                            text = heading.get_text(strip=True)
                            if text:
                                md_parts.append(f"{'#' * i} {text}\n")

                    # Paragraphs
                    for p in soup.find_all("p"):
                        text = p.get_text(strip=True)
                        if text:
                            md_parts.append(f"{text}\n")

                    # Links
                    for a in soup.find_all("a", href=True):
                        text = a.get_text(strip=True)
                        href = a["href"]
                        if text and href:
                            md_parts.append(f"[{text}]({href})\n")

                    # Lists
                    for ul in soup.find_all(["ul", "ol"]):
                        for li in ul.find_all("li"):
                            text = li.get_text(strip=True)
                            if text:
                                md_parts.append(f"- {text}\n")

                    result = "\n".join(md_parts)
                    return (
                        result[:8000]
                        if result
                        else soup.get_text(separator="\n", strip=True)[:8000]
                    )

                return content[:8000]

            else:
                return content[:8000]

    except httpx.TimeoutException:
        return "Error: Request timed out (30s limit)"
    except Exception as e:
        return f"Error: {e}"
