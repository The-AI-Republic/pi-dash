"""User settings surface: ``GET`` + ``PATCH`` ``/api/users/me/auto-pm/``.

The payload is a whitelist (``slug``, ``name`` = public name, ``description``
= public description, ``interval_label``, ``enabled``): the prompt, the admin
name and ``min_role`` must never leak. Jobs are instance-global, ordered by
``public_name``; absence of a preference row means enabled.
"""

from . import seed_loop as seed_l

SETTINGS = "/api/users/me/auto-pm/"

JOB_KEYS = {"slug", "name", "description", "interval_label", "enabled"}


def get(org, role="member"):
    return org.app("GET", SETTINGS, role)


def test_get_shape_whitelists_keys(org):
    seed_l.create_loop_job(
        org.conn, slug=f"wl-{org.tag}", public_name="Whitelist",
        name="Admin-Only Name", prompt="TOPSECRET PROMPT", min_role=20,
    )
    r = get(org)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body.keys()) == {"enabled", "jobs"}
    assert body["enabled"] is True
    job = next(j for j in body["jobs"] if j["slug"] == f"wl-{org.tag}")
    assert set(job.keys()) == JOB_KEYS
    assert job["name"] == "Whitelist"
    assert "TOPSECRET" not in r.text
    assert "min_role" not in job


def test_disabled_jobs_excluded(org):
    seed_l.create_loop_job(org.conn, slug=f"on-{org.tag}", public_name="On")
    seed_l.create_loop_job(
        org.conn, slug=f"off-{org.tag}", public_name="Off", enabled=False
    )
    slugs = {j["slug"] for j in get(org).json()["jobs"]}
    assert f"on-{org.tag}" in slugs
    assert f"off-{org.tag}" not in slugs


def test_jobs_ordered_by_public_name(org):
    seed_l.create_loop_job(org.conn, slug=f"z-{org.tag}", public_name="Zebra")
    seed_l.create_loop_job(org.conn, slug=f"m-{org.tag}", public_name="Mango")
    seed_l.create_loop_job(org.conn, slug=f"a-{org.tag}", public_name="Apple")
    names = [
        j["name"] for j in get(org).json()["jobs"] if j["slug"].endswith(org.tag)
    ]
    assert names == sorted(names)
    assert names == ["Apple", "Mango", "Zebra"]


def test_interval_labels(org):
    seed_l.create_loop_job(
        org.conn, slug=f"h-{org.tag}", public_name="H", rrule="FREQ=HOURLY"
    )
    seed_l.create_loop_job(
        org.conn, slug=f"d-{org.tag}", public_name="D", rrule="FREQ=DAILY;BYHOUR=3"
    )
    seed_l.create_loop_job(
        org.conn, slug=f"w-{org.tag}", public_name="W", rrule="FREQ=WEEKLY;BYDAY=MO"
    )
    seed_l.create_loop_job(
        org.conn, slug=f"m-{org.tag}", public_name="M", rrule="FREQ=MINUTELY"
    )
    seed_l.create_loop_job(
        org.conn, slug=f"g-{org.tag}", public_name="G", rrule="FREQ=YEARLY;BYMONTH=13"
    )
    seed_l.create_loop_job(org.conn, slug=f"x-{org.tag}", public_name="X", rrule="GARBAGE")
    labels = {j["slug"]: j["interval_label"] for j in get(org).json()["jobs"]}
    assert labels[f"h-{org.tag}"] == "hourly"
    assert labels[f"d-{org.tag}"] == "daily"
    assert labels[f"w-{org.tag}"] == "weekly"
    # The API never validates on the read path: even a MINUTELY job (which the
    # admin write path rejects) renders its label here.
    assert labels[f"m-{org.tag}"] == "every few minutes"
    assert labels[f"g-{org.tag}"] == "yearly"
    assert labels[f"x-{org.tag}"] == "periodically"


def test_master_pause_toggle(org):
    seed_l.create_loop_job(org.conn, slug=f"mp-{org.tag}", public_name="MP")
    r = org.app("PATCH", SETTINGS, "member", json={"enabled": False})
    assert r.status_code == 200, r.text
    assert r.json()["enabled"] is False
    row = org.conn.execute(
        "SELECT enabled FROM loop_user_preferences WHERE user_id=%s AND job_id IS NULL"
        " AND deleted_at IS NULL",
        (org.member["id"],),
    ).fetchone()
    assert row[0] is False
    # Toggling back upserts instead of duplicating.
    r = org.app("PATCH", SETTINGS, "member", json={"enabled": True})
    assert r.json()["enabled"] is True
    n = org.conn.execute(
        "SELECT count(*) FROM loop_user_preferences WHERE user_id=%s AND job_id IS NULL"
        " AND deleted_at IS NULL",
        (org.member["id"],),
    ).fetchone()[0]
    assert n == 1


def test_explicit_master_opt_in_still_enabled(org):
    seed_l.set_pref(org.conn, user_id=org.member["id"], job_id=None, enabled=True)
    assert get(org).json()["enabled"] is True


def test_invalid_payloads_rejected(org):
    seed_l.create_loop_job(org.conn, slug=f"ip-{org.tag}", public_name="IP")
    for payload in (
        {"enabled": "yes"},
        {"enabled": 1},
        {"enabled": None},
        {"foo": True},
        {"enabled": True, "extra": False},
        {},
        [True],
    ):
        r = org.app("PATCH", SETTINGS, "member", json=payload)
        assert r.status_code == 400, payload
        assert r.json() == {"error": "invalid_payload"}, payload
