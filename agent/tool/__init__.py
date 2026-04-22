"""Tools for litert buddy chat server."""

# from .web_browser import web_browser
from .agent_explore import agent_find_email, agent_find_phone, explore_website
from .get_weather import get_weather
from .mcp_client import mcp_browse_web, mcp_extract_links, mcp_search_web
from .web_fetch import web_fetch
from .web_search import web_search

# __all__ = ["web_browser", "get_weather", "web_search", "web_fetch"]

__all__ = [
    "agent_find_email",
    "agent_find_phone",
    "explore_website",
    "mcp_browse_web",
    "mcp_extract_links",
    "mcp_search_web",
    #
    "get_weather",
    "web_fetch",
    "web_search",
]
