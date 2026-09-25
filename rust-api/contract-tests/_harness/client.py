"""HTTP client fixtures shared by every contract-test domain.

The suite runs against a live backend (Django today, the Rust server
through the proxy tomorrow). ``BASE_URL`` selects the backend; redirects
are never followed so 3xx semantics stay observable.
"""

import os

import httpx
import pytest


def get_base_url() -> str:
    try:
        return os.environ["BASE_URL"]
    except KeyError as exc:
        raise pytest.UsageError(
            "BASE_URL is not set (e.g. BASE_URL=http://127.0.0.1:8000)"
        ) from exc


@pytest.fixture(scope="session")
def base_url() -> str:
    return get_base_url().rstrip("/")


@pytest.fixture()
def client(base_url: str) -> httpx.Client:
    with httpx.Client(base_url=base_url, timeout=10, follow_redirects=False) as c:
        yield c
