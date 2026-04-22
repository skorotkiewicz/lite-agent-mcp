"""Agentic exploration wrapper for MCP tools.

Implements multi-step website exploration WITHOUT relying on the LLM
to plan multiple tool calls. Local small models (4B) struggle with
iterative reasoning, so we handle the exploration loop in Python code.
"""

import re
from typing import Callable

from .mcp_client import mcp_browse_web, mcp_extract_links


def _extract_urls(text: str) -> list[str]:
    """Extract URLs from MCP extract_links output."""
    urls = []
    for line in text.split("\n"):
        line = line.strip()
        if line.startswith("http://") or line.startswith("https://"):
            urls.append(line)
    return urls


def _is_promising_link(url: str, search_terms: list[str]) -> bool:
    """Check if a URL looks promising based on search terms."""
    url_lower = url.lower()
    # Direct term match in URL
    for term in search_terms:
        if term.lower() in url_lower:
            return True
    # Common contact/info pages
    promising_patterns = [
        "/contact",
        "/about",
        "/info",
        "/help",
        "/support",
        "/team",
        "/people",
        "/staff",
        "/directory",
    ]
    for pattern in promising_patterns:
        if pattern in url_lower:
            return True
    return False


def _find_in_text(text: str, pattern: str) -> str | None:
    """Search for a pattern in text and return the match."""
    if not text:
        return None

    # Common pattern matchers
    matchers: dict[str, Callable[[str], str | None]] = {
        "phone": lambda t: _find_phone(t),
        "email": lambda t: _find_email(t),
        "address": lambda t: _find_address(t),
    }

    matcher = matchers.get(pattern.lower())
    if matcher:
        return matcher(text)

    # Generic keyword search
    pattern_lower = pattern.lower()
    lines = text.split("\n")
    for line in lines:
        if pattern_lower in line.lower():
            return line.strip()

    return None


def _find_phone(text: str) -> str | None:
    """Extract phone number from text."""
    # Common phone patterns
    phone_patterns = [
        # International with optional country code and multiple groups
        r"\+\d{1,3}[\s.-]?\(?\d{1,4}\)?[\s.-]?\d{1,4}[\s.-]?\d{2,4}(?:[\s.-]?\d{2,4})?",
        # Numbers with parentheses
        r"\(\d+\)[\s.-]?\d{2,4}[\s.-]?\d{2,4}(?:[\s.-]?\d{2,4})?",
        # US format
        r"\b\d{3}[\s.-]\d{3}[\s.-]\d{4}\b",
    ]

    for pattern in phone_patterns:
        matches = re.findall(pattern, text)
        for match in matches:
            if len(match) < 7:
                continue

            # Extract just the digits
            digits_only = re.sub(r"\D", "", match)

            # Skip if it's clearly a date (8 digits starting with 19/20/21)
            if len(digits_only) == 8 and re.match(r"^(19|20|21)\d{2}", digits_only):
                continue

            # Skip if it looks like a date with separators
            # YYYY-MM-DD, DD-MM-YYYY, MM-DD-YYYY
            if re.match(
                r"^(\d{4}[/.-]\d{1,2}[/.-]\d{1,2}|\d{1,2}[/.-]\d{1,2}[/.-]\d{4})$",
                match,
            ):
                continue

            return match
    return None


def _find_email(text: str) -> str | None:
    """Extract email from text."""
    pattern = r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b"
    matches = re.findall(pattern, text)
    return matches[0] if matches else None


def _find_address(text: str) -> str | None:
    """Extract address from text."""
    # Simple heuristic: look for lines with numbers and street keywords
    keywords = [
        "street",
        "st.",
        "avenue",
        "ave.",
        "road",
        "rd.",
        "lane",
        "ln.",
        "drive",
        "dr.",
    ]
    lines = text.split("\n")
    for line in lines:
        line_lower = line.lower()
        if any(kw in line_lower for kw in keywords) and re.search(r"\d", line):
            return line.strip()
    return None


def explore_website(target: str, url: str, max_depth: int = 2) -> str:
    """Explore a website to find specific information.

    This function handles the multi-step exploration loop that small local
    models (4B) cannot plan themselves. It:
    1. Browses the initial URL
    2. Looks for the target information
    3. If not found, extracts links and explores promising ones
    4. Returns result or "not found"

    Args:
        target: What to search for ("phone", "email", "address", or keywords)
        url: Starting URL to explore
        max_depth: How many link levels to explore (default 2)

    Returns:
        Found information or "not found" message
    """
    print(f"[Agent] Starting exploration: looking for '{target}' on {url}")

    # Step 1: Browse initial page
    result = mcp_browse_web(url)
    found = _find_in_text(result, target)
    if found:
        return f"Found {target}: {found}\n(Source: {url})"

    print("[Agent] Not found on initial page, extracting links...")

    # Step 2: Extract links
    links_text = mcp_extract_links(url)
    urls = _extract_urls(links_text)

    if not urls:
        return f"Could not find {target} on {url}. No links to explore."

    # Step 3: Filter promising links
    search_terms = [target, "contact", "about", "info"]
    promising = [u for u in urls if _is_promising_link(u, search_terms)]

    # Also try exact subpage variants
    base = url.rstrip("/")
    subpages = [
        f"{base}/contact",
        f"{base}/about",
        f"{base}/contact/",
        f"{base}/about/",
    ]
    for sp in subpages:
        if sp not in promising and sp not in urls:
            promising.append(sp)

    print(f"[Agent] Found {len(promising)} promising links to check")

    # Step 4: Explore each promising link
    checked = {url}
    for link in promising[:10]:  # Limit to 10 to avoid too many requests
        if link in checked:
            continue
        checked.add(link)

        print(f"[Agent] Checking: {link}")
        try:
            result = mcp_browse_web(link)
            found = _find_in_text(result, target)
            if found:
                return f"Found {target}: {found}\n(Source: {link})"
        except Exception as e:
            print(f"[Agent] Error checking {link}: {e}")
            continue

    return f"Could not find {target} after exploring {len(checked)} pages on {url}."


def agent_find_phone(url: str) -> str:
    """Find phone number on a website.

    Args:
        url: Website URL to search

    Returns:
        Phone number or not found message
    """
    return explore_website("phone", url)


def agent_find_email(url: str) -> str:
    """Find email address on a website.

    Args:
        url: Website URL to search

    Returns:
        Email or not found message
    """
    return explore_website("email", url)
