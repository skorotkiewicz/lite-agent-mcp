import httpx
from bs4 import BeautifulSoup

from ._utils import sanitize_url


def web_browser(url: str) -> str:
    """Fetch and extract text from a webpage."""
    try:
        url = sanitize_url(url)
        with httpx.Client(timeout=10, follow_redirects=True) as client:
            response = client.get(url)
            soup = BeautifulSoup(response.content, "html.parser")
            for tag in soup(["script", "style"]):
                tag.decompose()
            return soup.get_text(separator="\n", strip=True)[:8000]
    except Exception as e:
        return f"Error: {e}"
