"""DRF body negotiation on the loop write paths (PIDASHCONV-1222).

The loop handlers read ``request.data``, so they inherit DRF's
``Request._parse``: an empty body (Content-Length 0) is ``{}`` without
parser selection, a missing or unknown Content-Type is the 415
``UnsupportedMediaType`` detail, and form media types parse as forms. The
JSON-only readers used to skip that negotiation, turning empty bodies into
JSON parse errors and applying writes Django rejects.

Covers the two write surfaces named by the issue: the user auto-pm toggle
and the admin job PATCH (plus the admin create path, which shares the
reader).
"""

import time

from . import seed_loop as seed_l

SETTINGS = "/api/users/me/auto-pm/"
JOBS = "/api/instances/loop/jobs/"


def detail(job_id: str) -> str:
    return f"{JOBS}{job_id}/"


def _without_content_type(client, method: str, url: str, body: bytes):
    request = client.build_request(method, url, content=body)
    assert "content-type" not in request.headers
    return client.send(request)


def test_empty_body_is_empty_data_not_json_error(org):
    job = seed_l.create_loop_job(org.conn, slug=f"eb-{org.tag}", public_name="EB")
    before = org.conn.execute(
        "SELECT updated_at FROM loop_jobs WHERE id=%s", (job["id"],)
    ).fetchone()[0]
    time.sleep(0.002)

    r = org.adm("PATCH", detail(job["id"]), "admin", content=b"")
    assert r.status_code == 200, r.text
    assert r.json()["enabled"] is True
    # Django's no-op save still bumps the auto_now column.
    after = org.conn.execute(
        "SELECT updated_at FROM loop_jobs WHERE id=%s", (job["id"],)
    ).fetchone()[0]
    assert after > before

    r = org.app("PATCH", SETTINGS, "member", content=b"")
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "invalid_payload"}

    r = org.app("PATCH", f"{SETTINGS}jobs/{job['slug']}/", "member", content=b"")
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "invalid_payload"}

    r = org.adm("POST", JOBS, "admin", content=b"")
    assert r.status_code == 400, r.text
    assert r.json() == {
        "error": "missing_fields",
        "detail": ["name", "prompt", "public_name", "rrule", "slug"],
    }


def test_text_plain_is_unsupported_media_type(org):
    job = seed_l.create_loop_job(org.conn, slug=f"tp-{org.tag}", public_name="TP")
    headers = {"Content-Type": "text/plain"}
    expected = {"detail": 'Unsupported media type "text/plain" in request.'}

    r = org.adm(
        "PATCH", detail(job["id"]), "admin",
        content=b'{"enabled": false}', headers=headers,
    )
    assert r.status_code == 415, r.text
    assert r.json() == expected

    r = org.app(
        "PATCH", SETTINGS, "member",
        content=b'{"enabled": false}', headers=headers,
    )
    assert r.status_code == 415, r.text
    assert r.json() == expected

    r = org.app(
        "PATCH", f"{SETTINGS}jobs/{job['slug']}/", "member",
        content=b'{"enabled": false}', headers=headers,
    )
    assert r.status_code == 415, r.text
    assert r.json() == expected

    # The write was refused, so the toggle is untouched.
    r = org.app("GET", SETTINGS, "member")
    assert r.json()["enabled"] is True


def test_missing_content_type_is_unsupported_media_type(org):
    job = seed_l.create_loop_job(org.conn, slug=f"nc-{org.tag}", public_name="NC")
    expected = {"detail": 'Unsupported media type "" in request.'}

    for role, client, method, url in (
        ("admin", org.client("admin", admin=True), "PATCH", detail(job["id"])),
        ("member", org.client("member"), "PATCH", SETTINGS),
        ("member", org.client("member"), "PATCH", f"{SETTINGS}jobs/{job['slug']}/"),
    ):
        r = _without_content_type(client, method, url, b'{"enabled": false}')
        assert r.status_code == 415, (role, url, r.text)
        assert r.json() == expected

    r = org.app("GET", SETTINGS, "member")
    assert r.json()["enabled"] is True


def test_form_encoded_admin_create_parses_as_form(org):
    # HTML form input reaches the view as strings; the loop guards accept
    # the complete five-field form and create the job.
    form = {
        "slug": f"fm-{org.tag}",
        "name": "Form name",
        "public_name": "Form public",
        "prompt": "Form prompt",
        "rrule": "FREQ=DAILY;BYHOUR=2",
    }
    r = org.adm("POST", JOBS, "admin", data=form)
    assert r.status_code == 201, r.text
    body = r.json()
    assert body["slug"] == form["slug"]
    assert body["name"] == form["name"]
    assert body["public_name"] == form["public_name"]
    assert body["prompt"] == form["prompt"]
    assert body["rrule"] == form["rrule"]
