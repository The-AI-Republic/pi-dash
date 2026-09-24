"""HTTP client factory for contract suites."""

import httpx

from . import config


def make_client(cookies: dict | None = None) -> httpx.Client:
    return httpx.Client(base_url=config.base_url(), cookies=cookies, timeout=30.0)
