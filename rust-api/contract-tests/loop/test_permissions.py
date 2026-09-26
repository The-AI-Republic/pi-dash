"""Denied-permission cases for every loop surface.

The user surface carries no role gate (``IsAuthenticated`` only): the denied
case there is an unauthenticated caller (401). The admin surface additionally
requires ``InstanceAdminPermission``: a validly-authenticated non-admin gets
403, and a caller presenting the *app* cookie on an ``/api/instances/`` route
is anonymous there (401) because that path reads the ``admin-session-id``
cookie instead.

``test_instance_admin_gate_enforced`` is the deliberate-removal tripwire:
removing the view's ``permission_classes`` flips it from 403 to 200 and the
suite goes red. That removal is demonstrated once (locally, reverted) before
the PR.
"""

import uuid

from . import seed_loop as seed_l
from .test_admin_jobs import JOBS, detail, valid_payload
from .test_admin_targets import targets as targets_url
from .test_user_job_toggle import job_url
from .test_user_settings import SETTINGS

ANON = {"detail": "Authentication credentials were not provided."}
DENIED = {"detail": "You do not have permission to perform this action."}


def test_anonymous_denied_on_user_routes(org):
    seed_l.create_loop_job(org.conn, slug=f"ad-{org.tag}", public_name="AD")
    cases = [
        ("GET", SETTINGS, None),
        ("PATCH", SETTINGS, {"enabled": False}),
        ("PATCH", job_url(f"ad-{org.tag}"), {"enabled": False}),
    ]
    for method, path, payload in cases:
        r = org.app(method, path, None, json=payload)
        assert r.status_code == 401, path
        assert r.json() == ANON, path


def test_anonymous_denied_on_admin_routes(org):
    job = seed_l.create_loop_job(org.conn, slug=f"ad-{org.tag}", public_name="AD")
    cases = [
        ("GET", JOBS, None),
        ("POST", JOBS, valid_payload(f"zz-{org.tag}")),
        ("GET", detail(job["id"]), None),
        ("PATCH", detail(job["id"]), {"enabled": False}),
        ("DELETE", detail(job["id"]), None),
        ("GET", targets_url(job["id"]), None),
    ]
    for method, path, payload in cases:
        r = org.adm(method, path, None, json=payload)
        assert r.status_code == 401, path
        assert r.json() == ANON, path
    # The job still exists: none of the denied writes landed.
    assert org.conn.execute(
        "SELECT count(*) FROM loop_jobs WHERE id=%s AND deleted_at IS NULL", (job["id"],)
    ).fetchone()[0] == 1


def test_app_cookie_is_anonymous_on_admin_routes(org):
    # ``/api/instances/`` reads the ``admin-session-id`` cookie: a caller
    # presenting only the app session is unauthenticated here.
    job = seed_l.create_loop_job(org.conn, slug=f"ac-{org.tag}", public_name="AC")
    r = org.app("GET", JOBS, "admin")
    assert r.status_code == 401
    assert r.json() == ANON
    r = org.app("GET", detail(job["id"]), "admin")
    assert r.status_code == 401


def test_non_admin_denied_on_every_admin_route(org):
    job = seed_l.create_loop_job(org.conn, slug=f"nd-{org.tag}", public_name="ND")
    cases = [
        ("GET", JOBS, None),
        ("POST", JOBS, valid_payload(f"zz-{org.tag}")),
        ("GET", detail(job["id"]), None),
        ("PATCH", detail(job["id"]), {"enabled": False}),
        ("DELETE", detail(job["id"]), None),
        ("GET", targets_url(job["id"]), None),
    ]
    for role in ("member", "guest", "outsider", "other"):
        for method, path, payload in cases:
            r = org.adm(method, path, role, json=payload)
            assert r.status_code == 403, (role, method, path)
            assert r.json() == DENIED, (role, method, path)
    # None of the denied writes landed.
    assert org.conn.execute(
        "SELECT enabled FROM loop_jobs WHERE id=%s", (job["id"],)
    ).fetchone()[0] is True


def test_instance_admin_gate_enforced(org):
    # Tripwire for the deliberate-permission-removal check: with
    # ``InstanceAdminPermission`` in place a member is refused; removing the
    # gate (one-line local patch, reverted) turns this 403 into a 200.
    r = org.adm("GET", JOBS, "member")
    assert r.status_code == 403
    assert r.json() == DENIED


def test_user_routes_have_no_role_gate(org):
    # Preferences are the user's own: guest and outsider alike get 200.
    seed_l.create_loop_job(org.conn, slug=f"ng-{org.tag}", public_name="NG")
    for role in ("admin", "member", "guest", "outsider"):
        assert org.app("GET", SETTINGS, role).status_code == 200, role
        r = org.app("PATCH", job_url(f"ng-{org.tag}"), role, json={"enabled": False})
        assert r.status_code == 200, role


def test_stale_instance_admin_denied(org):
    # The permission checks the *first* Instance row (newest): an admin row on
    # any other instance is not enough.
    import datetime

    iid = seed_l.new_id()
    now = datetime.datetime.now(datetime.timezone.utc).isoformat()
    org.conn.execute(
        """INSERT INTO instances (id, instance_name, instance_id, current_version,
            edition, domain, last_checked_at, is_telemetry_enabled,
            is_support_required, is_setup_done, is_signup_screen_visited,
            is_verified, is_test, is_current_version_deprecated,
            created_at, updated_at)
        VALUES (%s,'decoy',%s,'1.0.0','PI_DASH_COMMUNITY','',%s,
            true,true,false,false,false,false,false,%s,%s)""",
        (iid, f"decoy-{org.tag}", now, now, now),
    )
    seed_l.make_instance_admin(
        org.conn, instance_id=iid, user_id=org.other_user["id"]
    )
    # The decoy must stay older than the suite instance so it never becomes
    # ``first()`` itself: equal ``created_at`` values would leave the order
    # undefined, so the suite row is stamped strictly newer.
    import datetime as _dt

    newer = (datetime.datetime.now(datetime.timezone.utc)
             + datetime.timedelta(seconds=5)).isoformat()
    org.conn.execute(
        "UPDATE instances SET created_at=%s, updated_at=%s WHERE id=%s",
        (newer, newer, org.instance["id"]),
    )
    r = org.adm("GET", JOBS, "other")
    assert r.status_code == 403
    assert r.json() == DENIED
