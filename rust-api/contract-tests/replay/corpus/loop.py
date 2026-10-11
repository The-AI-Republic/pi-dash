# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: loop (auto-pm) write flows (PIDASHCONV-823).

Built from the ``loop`` contract suite: test_user_settings.py (master
pause upsert, invalid payloads), test_user_job_toggle.py (per-job
upsert, unknown/disabled 404s, invalid payloads, master+job compose),
test_admin_jobs.py (console create with defaults and every validator
branch, patch semantics, soft-delete freeing the slug) and
test_permissions.py (anon 401s, non-admin 403s, app-cookie-is-anonymous
on /api/instances/).

The console writes authenticate as ``admin:<user>`` (the forged
sessions row as the admin-session-id cookie). The seed reuses the
domain's own ``seed_loop.create_loop_job`` factory.
"""

from ..session import Request, Session

SETTINGS = "/api/users/me/auto-pm/"
JOBS = "/api/instances/loop/jobs/"
_MISSING = "00000000-0000-0000-0000-000000000000"


def _job_url(slug: str) -> str:
    return f"{SETTINGS}jobs/{slug}/"


def _detail(job_id: str) -> str:
    return f"{JOBS}{job_id}/"


def _payload(slug: str, **kw) -> dict:
    payload = {
        "slug": slug,
        "name": "Admin name",
        "public_name": "Public name",
        "prompt": "Do the thing.",
        "rrule": "FREQ=DAILY;BYHOUR=2",
    }
    payload.update(kw)
    return payload


_SEED_JOBS = (
    # (slug, public_name, kwargs)
    ("rp-tj", "TJ", {}),
    ("rp-dj", "DJ", {"enabled": False}),
    ("rp-ij", "IJ", {}),
    ("rp-cj", "CJ", {}),
    ("rp-mp", "MP", {}),
    ("rp-ip", "IP", {}),
    ("rp-pu", "PU", {"is_builtin": True}),
    ("rp-p1", "P", {}),
    ("rp-p2", "P", {}),
    ("rp-pe", "P", {}),
    ("rp-dl", "DL", {}),
    ("rp-dup", "D", {}),
)


def _seed_extras(conn) -> None:
    from loop import seed_loop as seed_l

    for slug, public_name, kw in _SEED_JOBS:
        seed_l.create_loop_job(conn, slug=slug, public_name=public_name, **kw)
    conn.commit()


def _setup(world, config) -> dict:
    import psycopg

    with psycopg.connect(config.django_db) as conn, conn.cursor() as cur:
        out = {}
        for slug, _, _ in _SEED_JOBS:
            cur.execute("SELECT id FROM loop_jobs WHERE slug = %s", (slug,))
            out[slug.replace("-", "_")] = str(cur.fetchone()[0])
    return out


SESSIONS = (
    Session(
        name="loop_lifecycle",
        domain="loop",
        source="loop/test_user_settings.py, test_user_job_toggle.py, "
        "test_admin_jobs.py, test_permissions.py",
        auth_users=("admin", "member", "guest", "outsider", "admin:admin", "admin:member"),
        seed=_seed_extras,
        setup=_setup,
        setup_keys=tuple(slug.replace("-", "_") for slug, _, _ in _SEED_JOBS),
        # loop_jobs.dtstart defaults to now() at create (admin_views
        # :121-122); the corpus never sends it explicitly. Deliberately
        # session-scoped, not global: scheduler-table dtstarts (builtin
        # seeds, PIDASHCONV-886) still compare raw.
        timestamp_columns={"loop_jobs": ["dtstart"]},
        requests=(
            # -- master pause: off, on (upsert, no duplicate) --
            Request(
                "PATCH", SETTINGS,
                route="/api/users/me/auto-pm/",
                json={"enabled": False},
                auth="member", expect_status=200,
            ),
            Request(
                "PATCH", SETTINGS,
                route="/api/users/me/auto-pm/",
                json={"enabled": True},
                auth="member", expect_status=200,
            ),
            # -- master invalid payloads --
            Request(
                "PATCH", SETTINGS,
                route="/api/users/me/auto-pm/",
                json={"enabled": "yes"},
                auth="member", expect_status=400,
            ),
            Request(
                "PATCH", SETTINGS,
                route="/api/users/me/auto-pm/",
                json={"enabled": 1},
                auth="member", expect_status=400,
            ),
            Request(
                "PATCH", SETTINGS,
                route="/api/users/me/auto-pm/",
                json={"enabled": None},
                auth="member", expect_status=400,
            ),
            Request(
                "PATCH", SETTINGS,
                route="/api/users/me/auto-pm/",
                json={"foo": True},
                auth="member", expect_status=400,
            ),
            Request(
                "PATCH", SETTINGS,
                route="/api/users/me/auto-pm/",
                json={"enabled": True, "extra": False},
                auth="member", expect_status=400,
            ),
            # -- per-job toggle: off, on (same row), 404s --
            Request(
                "PATCH", _job_url("rp-tj"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"enabled": False},
                auth="member", expect_status=200,
            ),
            Request(
                "PATCH", _job_url("rp-tj"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"enabled": True},
                auth="member", expect_status=200,
            ),
            Request(
                "PATCH", _job_url("rp-nope"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"enabled": False},
                auth="member", expect_status=404,
            ),
            Request(
                "PATCH", _job_url("rp-dj"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"enabled": False},
                auth="member", expect_status=404,
            ),
            # -- per-job invalid payloads --
            Request(
                "PATCH", _job_url("rp-ij"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"enabled": "yes"},
                auth="member", expect_status=400,
            ),
            Request(
                "PATCH", _job_url("rp-ij"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"enabled": 1},
                auth="member", expect_status=400,
            ),
            Request(
                "PATCH", _job_url("rp-ij"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"other": False},
                auth="member", expect_status=400,
            ),
            Request(
                "PATCH", _job_url("rp-ij"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={},
                auth="member", expect_status=400,
            ),
            # -- master+job compose, then restore --
            Request(
                "PATCH", SETTINGS,
                route="/api/users/me/auto-pm/",
                json={"enabled": False},
                auth="member", expect_status=200,
            ),
            Request(
                "PATCH", _job_url("rp-cj"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"enabled": False},
                auth="member", expect_status=200,
            ),
            Request(
                "PATCH", SETTINGS,
                route="/api/users/me/auto-pm/",
                json={"enabled": True},
                auth="member", expect_status=200,
            ),
            # -- user surface denials (no role gate; anon refused) --
            Request(
                "PATCH", SETTINGS,
                route="/api/users/me/auto-pm/",
                json={"enabled": False},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "PATCH", _job_url("rp-tj"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"enabled": False},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "PATCH", _job_url("rp-tj"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"enabled": False},
                auth="guest", expect_status=200,
            ),
            Request(
                "PATCH", _job_url("rp-tj"),
                route="/api/users/me/auto-pm/jobs/{}/",
                json={"enabled": True},
                auth="outsider", expect_status=200,
            ),
            # -- console create: defaults + every validator branch --
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-cd"),
                auth="admin:admin", capture={"job_new": "id"},
                expect_status=201,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-fast", rrule="FREQ=MINUTELY"),
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("Bad Slug rp"),
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-mr", min_role=7),
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-rr", rrule="FREQ=DAILY;BYHOUR=xx"),
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-er", rrule=""),
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json={"slug": "rp-mf"},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-dup"),
                auth="admin:admin", expect_status=409,
            ),
            # int() coercion quirks: numeric string and float echo raw
            # in the create response; garbage 500s through the validator.
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-ms", min_role="15"),
                auth="admin:admin", capture={"job_ms": "id"},
                expect_status=201,
            ),
            Request(
                "GET", _detail("{job_ms}"),
                route="/api/instances/loop/jobs/{}/",
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-mg", min_role="abc"),
                auth="admin:admin", expect_status=500,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-mfl", min_role=15.5),
                auth="admin:admin", capture={"job_fl": "id"},
                expect_status=201,
            ),
            # -- console patch: unknown keys dropped, builtin kept --
            Request(
                "PATCH", _detail("{rp_pu}"),
                route="/api/instances/loop/jobs/{}/",
                json={"is_builtin": False, "enabled": False, "bogus": 1},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", _detail("{rp_p1}"),
                route="/api/instances/loop/jobs/{}/",
                json={"slug": "rp-p1"},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", _detail("{rp_p1}"),
                route="/api/instances/loop/jobs/{}/",
                json={"slug": "rp-p2"},
                auth="admin:admin", expect_status=409,
            ),
            Request(
                "PATCH", _detail("{rp_pe}"),
                route="/api/instances/loop/jobs/{}/",
                json={"enabled": "yes"},
                auth="admin:admin", expect_status=400,
            ),
            # -- console delete: soft, frees the slug; twice is 404 --
            Request(
                "DELETE", _detail("{rp_dl}"),
                route="/api/instances/loop/jobs/{}/",
                auth="admin:admin", expect_status=204,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-dl"),
                auth="admin:admin", capture={"job_dl2": "id"},
                expect_status=201,
            ),
            Request(
                "DELETE", _detail("{rp_dl}"),
                route="/api/instances/loop/jobs/{}/",
                auth="admin:admin", expect_status=404,
            ),
            Request(
                "DELETE", _detail(_MISSING),
                route="/api/instances/loop/jobs/{}/",
                auth="admin:admin", expect_status=404,
            ),
            # -- console denials: non-admin 403, app cookie 401, anon 401 --
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-zz"),
                auth="admin:member", expect_status=403,
            ),
            Request(
                "PATCH", _detail("{rp_pu}"),
                route="/api/instances/loop/jobs/{}/",
                json={"enabled": False},
                auth="admin:member", expect_status=403,
            ),
            Request(
                "DELETE", _detail("{rp_pu}"),
                route="/api/instances/loop/jobs/{}/",
                auth="admin:member", expect_status=403,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-zz"),
                auth="admin", expect_status=401,
            ),
            Request(
                "PATCH", _detail("{rp_pu}"),
                route="/api/instances/loop/jobs/{}/",
                json={"enabled": False},
                auth="admin", expect_status=401,
            ),
            Request(
                "DELETE", _detail("{rp_pu}"),
                route="/api/instances/loop/jobs/{}/",
                auth="admin", expect_status=401,
            ),
            Request(
                "POST", JOBS,
                route="/api/instances/loop/jobs/",
                json=_payload("rp-zz"),
                auth="anonymous", expect_status=401,
            ),
            Request(
                "PATCH", _detail("{rp_pu}"),
                route="/api/instances/loop/jobs/{}/",
                json={"enabled": False},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "DELETE", _detail("{rp_pu}"),
                route="/api/instances/loop/jobs/{}/",
                auth="anonymous", expect_status=401,
            ),
        ),
    ),
)
