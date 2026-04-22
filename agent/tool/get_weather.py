import httpx

from ._utils import sanitize_url


def get_weather(location: str) -> str:
    """Get current weather for a location."""
    try:
        clean_location = sanitize_url(location).replace(" ", "+")
        url = f"https://wttr.in/{clean_location}?format=3"
        with httpx.Client(timeout=10) as client:
            response = client.get(url)
            return response.text.strip()
    except Exception as e:
        return f"Error: {e}"
