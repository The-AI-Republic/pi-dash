"""Resolver-level UUID gating on the instance-admin loop job routes.

Django routes ``jobs/<uuid:pk>/`` and ``jobs/<uuid:pk>/targets/`` through
``UUIDConverter`` (``loop/admin_urls.py:16-17``), whose regex accepts only
the canonical lowercase-hyphenated spelling. Uppercase, 32-hex simple,
braced and ``urn:uuid:`` spellings resolve 404 before auth or the view
runs, so no row is read or written.
"""

from __future__ import annotations

import pytest

from . import seed_loop as seed_l

JOBS = "/api/instances/loop/jobs/"


def _spellings(job_id: str) -> dict[str, str]:
    return {
        "uppercase": job_id.upper(),
        "simple": job_id.replace("-", ""),
        "braced": "{" + job_id + "}",
        "urn": "urn:uuid:" + job_id,
    }


def _assert_resolver_404(res) -> None:
    # Django's resolver catches the URL before any view or permission code;
    # the answer is the HTML 404, not a JSON body or a 401/403/204.
    assert res.status_code == 404, res.text
    assert res.headers["content-type"].startswith("text/html"), res.headers.get(
        "content-type"
    )


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_admin_job_noncanonical_spelling_404s_without_writes(org, spelling):
    """Every non-canonical spelling 404s on detail, patch, delete and targets.

    PIDASHCONV-1225: ``Uuid::parse_str`` accepts all four spellings while
    Django's ``<uuid:pk>`` converter rejects them at resolution time. The
    Rust route must proxy them to Django instead of serving or mutating the
    row.
    """
    job = seed_l.create_loop_job(
        org.conn, slug=f"nc-{org.tag}", public_name="Noncanonical"
    )
    segment = _spellings(job["id"])[spelling]
    detail = f"{JOBS}{segment}/"

    _assert_resolver_404(org.adm("GET", detail, "admin"))
    _assert_resolver_404(
        org.adm("PATCH", detail, "admin", json={"name": "Mutated"})
    )
    _assert_resolver_404(org.adm("GET", f"{detail}targets/", "admin"))
    _assert_resolver_404(org.adm("DELETE", detail, "admin"))

    row = org.conn.execute(
        "SELECT name, deleted_at FROM loop_jobs WHERE id=%s", (job["id"],)
    ).fetchone()
    assert row[0] == "Admin name", spelling
    assert row[1] is None, spelling
