"""Non-canonical ``<uuid:>`` spellings on scheduler/binding detail + runs (PIDASHCONV-1475).

Django declares every id segment below with ``<uuid:...>``
(``apps/api/pi_dash/app/urls/scheduler.py``: ``schedulers/<uuid:scheduler_id>/``,
``scheduler-bindings/<uuid:binding_id>/`` and ``.../runs/``), whose converter
matches only the canonical lowercase hyphenated form — every other spelling
resolver-404s before the view runs. Rust parsed the segment with
``Uuid::parse`` (``handlers_sched.rs`` detail GET/PATCH/DELETE,
``handlers_bind.rs`` detail GET/PATCH/DELETE + runs GET), which also accepts
uppercase, simple (no hyphens), braced and ``urn:uuid:`` spellings, so it
served — and PATCH/DELETE wrote through — URLs Django never routes.

Fix: gate all seven sites on the shared ``classify_uuid_segment``; a
non-canonical spelling takes the route's existing unmatched path (the Django
proxy garbage ids already take, so the resolver 404 answers byte for byte).

Body assertions follow PIDASHCONV-1215: the harness serves Django with
``DEBUG = True``, so its resolver 404 is the HTML technical page, while prod
serves the 28-byte JSON ``{"error": "Page not found."}`` — which is also what
the Rust leg proxies back in prod.
"""

import pytest

from .test_project_binding_detail import bind_detail, _installed
from .test_project_bindings import bind_list
from .test_workspace_scheduler_detail import _create, sched_detail

RESOLVER_404 = b'{"error": "Page not found.}'


def assert_resolver_404(response):
    assert response.status_code == 404, response.text
    ctype = response.headers.get("content-type", "")
    if ctype.startswith("application/json"):
        # The prod resolver-404 bytes, exactly (JsonResponse defaults).
        assert response.content == RESOLVER_404, response.content
    else:
        # Harness Django (DEBUG=True) renders the HTML technical 404 for an
        # unmatched path, so only the status and content type are comparable.
        assert "text/html" in ctype, ctype


SPELLINGS = ["uppercase", "simple", "braced", "urn"]


def _spellings(row_id):
    """Spellings ``Uuid::parse`` accepts but Django's ``<uuid:>`` does not."""
    return {
        "uppercase": row_id.upper(),
        "simple": row_id.replace("-", ""),
        "braced": "{" + row_id + "}",
        "urn": "urn:uuid:" + row_id,
    }


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_scheduler_get_noncanonical_404s(org, spelling):
    created = _create(org)
    seg = _spellings(created["id"])[spelling]
    assert_resolver_404(org.request("GET", sched_detail(org, seg), "admin"))
    # Resolution precedes auth: anonymous callers 404 too, not 401/403.
    assert_resolver_404(org.request("GET", sched_detail(org, seg), None))


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_scheduler_patch_noncanonical_404s_and_writes_nothing(org, spelling):
    created = _create(org, slug="keep", name="Keep")
    seg = _spellings(created["id"])[spelling]
    r = org.request(
        "PATCH", sched_detail(org, seg), "admin", json={"name": "Mutated"}
    )
    assert_resolver_404(r)
    control = org.request("GET", sched_detail(org, created["id"]), "admin")
    assert control.status_code == 200, control.text
    assert control.json()["name"] == "Keep"


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_scheduler_delete_noncanonical_404s_and_deletes_nothing(org, spelling):
    created = _create(org)
    seg = _spellings(created["id"])[spelling]
    assert_resolver_404(org.request("DELETE", sched_detail(org, seg), "admin"))
    control = org.request("GET", sched_detail(org, created["id"]), "admin")
    assert control.status_code == 200, control.text


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_binding_get_noncanonical_404s(org, spelling):
    _, binding = _installed(org)
    seg = _spellings(binding["id"])[spelling]
    assert_resolver_404(org.request("GET", bind_detail(org, seg), "admin"))
    # Resolution precedes auth: anonymous callers 404 too, not 401/403.
    assert_resolver_404(org.request("GET", bind_detail(org, seg), None))


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_binding_patch_noncanonical_404s_and_writes_nothing(org, spelling):
    _, binding = _installed(org)
    seg = _spellings(binding["id"])[spelling]
    r = org.request(
        "PATCH", bind_detail(org, seg), "admin", json={"enabled": False}
    )
    assert_resolver_404(r)
    control = org.request("GET", bind_detail(org, binding["id"]), "admin")
    assert control.status_code == 200, control.text
    assert control.json()["enabled"] is True


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_binding_delete_noncanonical_404s_and_deletes_nothing(org, spelling):
    _, binding = _installed(org)
    seg = _spellings(binding["id"])[spelling]
    assert_resolver_404(org.request("DELETE", bind_detail(org, seg), "admin"))
    control = org.request("GET", bind_detail(org, binding["id"]), "admin")
    assert control.status_code == 200, control.text


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_binding_runs_noncanonical_404s(org, spelling):
    _, binding = _installed(org)
    seg = _spellings(binding["id"])[spelling]
    assert_resolver_404(
        org.request("GET", f"{bind_list(org)}{seg}/runs/", "admin")
    )
    assert_resolver_404(org.request("GET", f"{bind_list(org)}{seg}/runs/", None))
