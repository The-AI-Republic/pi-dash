"""Exact route coverage: the document must expose every ``/api/v1/`` route.

Byte-identity with Django's rendering is *not* required of the Rust port
(utoipa generates its own serialization) — but route coverage is: the same
set of ``{path: [methods]}``. The committed ``routes_golden.json`` is that
set as Django serves it today.

When Django legitimately gains or loses a v1 route, refresh deliberately::

    REGEN_GOLDEN=1 pytest v1_openapi/test_routes_golden.py

and review the golden diff as part of the change — never blanket-update it
to chase green.
"""

import json
import os

import pytest

GOLDEN = os.path.join(os.path.dirname(os.path.abspath(__file__)), "routes_golden.json")


def route_map(doc: dict) -> dict:
    return {path: sorted(m.lower() for m in ops) for path, ops in sorted(doc["paths"].items())}


@pytest.mark.contract
def test_routes_match_golden(doc):
    live = route_map(doc)
    if os.environ.get("REGEN_GOLDEN") == "1":
        with open(GOLDEN, "w") as f:
            json.dump(live, f, indent=2)
            f.write("\n")
        print(f"\nregenerated {GOLDEN} ({len(live)} paths)")
        return
    with open(GOLDEN) as f:
        golden = json.load(f)
    assert live == golden
