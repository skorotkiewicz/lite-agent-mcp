"""Shared utilities for tools."""


def sanitize_url(url: str) -> str:
    """Clean and validate URL string."""
    return url.strip().replace('<|"|>', "").strip()
