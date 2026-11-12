"""Per-job toggle: ``PATCH`` ``/api/users/me/auto-pm/jobs/<slug>/``.

Toggling is an upsert on the user's own preference row. A slug with no
*enabled* job behind it — unknown or admin-disabled — is 404.
"""

from . import seed_loop as seed_l
from .test_user_settings import JOB_KEYS

SETTINGS = "/api/users/me/auto-pm/"


def job_url(slug: str) -> str:
    return f"{SETTINGS}jobs/{slug}/"


def test_toggle_shape_and_upsert(org):
    seed_l.create_loop_job(org.conn, slug=f"tj-{org.tag}", public_name="TJ")
    r = org.app("PATCH", job_url(f"tj-{org.tag}"), "member", json={"enabled": False})
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body.keys()) == {"enabled", "jobs"}
    job = next(j for j in body["jobs"] if j["slug"] == f"tj-{org.tag}")
    assert set(job.keys()) == JOB_KEYS
    assert job["enabled"] is False
    # Toggling back flips the same row instead of inserting another.
    r = org.app("PATCH", job_url(f"tj-{org.tag}"), "member", json={"enabled": True})
    job = next(j for j in r.json()["jobs"] if j["slug"] == f"tj-{org.tag}")
    assert job["enabled"] is True
    n = org.conn.execute(
        """SELECT count(*) FROM loop_user_preferences
        WHERE user_id=%s AND job_id=(SELECT id FROM loop_jobs WHERE slug=%s)
        AND deleted_at IS NULL""",
        (org.member["id"], f"tj-{org.tag}"),
    ).fetchone()[0]
    assert n == 1


def test_unknown_job_404(org):
    r = org.app("PATCH", job_url(f"nope-{org.tag}"), "member", json={"enabled": False})
    assert r.status_code == 404
    assert r.json() == {"error": "not_found"}


def test_disabled_job_404(org):
    # The lookup filters ``enabled=True``: an admin-disabled slug is
    # indistinguishable from an unknown one.
    seed_l.create_loop_job(
        org.conn, slug=f"dj-{org.tag}", public_name="DJ", enabled=False
    )
    r = org.app("PATCH", job_url(f"dj-{org.tag}"), "member", json={"enabled": False})
    assert r.status_code == 404
    assert r.json() == {"error": "not_found"}


def test_invalid_payloads_rejected(org):
    seed_l.create_loop_job(org.conn, slug=f"ij-{org.tag}", public_name="IJ")
    for payload in ({"enabled": "yes"}, {"enabled": 1}, {"other": False}, {}):
        r = org.app("PATCH", job_url(f"ij-{org.tag}"), "member", json=payload)
        assert r.status_code == 400, payload
        assert r.json() == {"error": "invalid_payload"}, payload


def test_master_pause_does_not_hide_job_row(org):
    # The master switch and the per-job switch compose: pausing everything
    # keeps the job visible with its own effective state.
    seed_l.create_loop_job(org.conn, slug=f"cj-{org.tag}", public_name="CJ")
    assert org.app("PATCH", SETTINGS, "member", json={"enabled": False}).status_code == 200
    r = org.app("PATCH", job_url(f"cj-{org.tag}"), "member", json={"enabled": False})
    assert r.status_code == 200
    body = r.json()
    assert body["enabled"] is False
    job = next(j for j in body["jobs"] if j["slug"] == f"cj-{org.tag}")
    assert job["enabled"] is False
