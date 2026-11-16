"""Contract tests: non-canonical ``<uuid:scheduler_id>`` spellings.

PIDASHCONV-1205. Django routes the two scheduler-id paths with the
``<uuid:scheduler_id>`` converter (``api/urls/scheduler.py:26`` and
``:31``), whose regex is ``[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}``
— lowercase, hyphenated, canonical form only, matched case-sensitively. Any
other spelling matches no URL pattern and gets the resolver 404 *before*
authentication or any view code runs.

The Rust handlers parse the segment with ``Uuid::parse_str``
(``v1_schedulers/handlers.rs``), which also accepts uppercase hex, the simple
form (32 hex digits), the braced form and the URN form. Those segments parse,
so without a canonical gate Rust authenticates, runs the gates and serves the
request — 200 for a member, 401 without a key, 403 for a non-member, and the
``SchedulerNotFound`` / ``BindingNotFound`` 404 bodies otherwise.

Same root cause as PIDASHCONV-1095 / -1132 / -1170 / -1180 / -1193.
"""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http  # noqa: E402

# Its own key so the per-key rate throttle never couples this module to the
# detail/runs modules (every module authenticates with its own key).
KEY = "dt"


def _spellings(scheduler_id):
    # Every spelling ``uuid.UUID()`` (and Rust's ``Uuid::parse_str``) accepts
    # but Django's ``<uuid:>`` converter does not route.
    return {
        "uppercase": scheduler_id.upper(),
        "simple": scheduler_id.replace("-", ""),
        "braced": "{" + scheduler_id + "}",
        "urn": "urn:uuid:" + scheduler_id,
    }


def detail_url(seed, segment):
    return (
        f"/api/v1/workspaces/{seed['ws_a']['slug']}"
        f"/projects/{seed['project']['id']}/schedulers/{segment}/"
    )


def runs_url(seed, segment):
    return detail_url(seed, segment) + "runs/"


def _assert_resolver_404(res):
    # Resolving the URL precedes auth and the view entirely: the answer is
    # the resolver's HTML 404, never DRF's JSON body or a 200/401/403.
    assert res.status_code == 404, res.text
    assert res.headers["content-type"].startswith("text/html"), res.headers.get(
        "content-type"
    )


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
@pytest.mark.parametrize("build", [detail_url, runs_url], ids=["detail", "runs"])
def test_non_canonical_spelling_404s_for_member(seed, spelling, build):
    segment = _spellings(str(seed["scheduler"]["id"]))[spelling]
    res = http.get(seed["keys"][KEY], build(seed, segment), expect=404)
    _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
@pytest.mark.parametrize("build", [detail_url, runs_url], ids=["detail", "runs"])
def test_non_canonical_spelling_404s_anonymously(seed, spelling, build):
    # No X-Api-Key header at all: Django's resolver 404 precedes auth, so an
    # anonymous caller gets 404, never the 401 a parsed-but-non-canonical
    # segment produces in Rust.
    segment = _spellings(str(seed["scheduler"]["id"]))[spelling]
    with http.client(None) as client:
        res = client.get(build(seed, segment))
    _assert_resolver_404(res)
