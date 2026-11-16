"""Contract tests: api-v1 estimate endpoints (urls/estimate.py, 3 URL entries).

PORTED BUG PINNED: the estimate URL patterns are defined in
`pi_dash/api/urls/estimate.py` but never registered in
`pi_dash/api/urls/__init__.py`, so every estimate route answers 404 — for
authenticated callers and anonymous ones alike (URL resolution runs before
auth, so there is no 401/403 oracle here). The Rust port must reproduce the
404 until a follow-up decides to register the routes; when that happens this
module becomes the shape suite for the newly live endpoints.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http  # noqa: E402

KEY = "estimates"


def base(seed):
    return (f"/api/v1/workspaces/{seed['ws_a']['slug']}"
            f"/projects/{seed['project']['id']}/estimates")


def test_estimate_collection_unregistered(seed):
    http.get(seed["keys"][KEY], base(seed) + "/", expect=404)
    http.post(seed["keys"][KEY], base(seed) + "/",
              json={"name": "CT Est", "type": "points"}, expect=404)
    http.patch(seed["keys"][KEY], base(seed) + "/",
               json={"name": "CT Est2"}, expect=404)
    http.delete(seed["keys"][KEY], base(seed) + "/", expect=404)


def test_estimate_points_unregistered(seed):
    est = "00000000-0000-0000-0000-000000000000"
    http.get(seed["keys"][KEY], f"{base(seed)}/{est}/estimate-points/", expect=404)
    http.post(seed["keys"][KEY], f"{base(seed)}/{est}/estimate-points/",
              json=[{"key": 1, "value": "1", "description": ""}], expect=404)
    http.patch(seed["keys"][KEY],
               f"{base(seed)}/{est}/estimate-points/{est}/", expect=404)
    http.delete(seed["keys"][KEY],
                f"{base(seed)}/{est}/estimate-points/{est}/", expect=404)


def test_estimate_routes_anonymous_404(seed):
    # Unknown paths 404 before authentication runs: no credential oracle.
    http.get(None, base(seed) + "/", expect=404)
    http.get("ct-bogus-token", base(seed) + "/", expect=404)
