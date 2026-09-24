"""Instance-admin job catalog: list / create / detail / patch / delete.

``GET`` + ``POST`` ``/api/instances/loop/jobs/`` and ``GET`` + ``PATCH`` +
``DELETE`` ``/api/instances/loop/jobs/<uuid>/``. Unlike the user surface, the
admin surface exposes the full job (prompt, ``min_role``, RRULE); unknown
write keys are ignored, so ``is_builtin`` can never be flipped by API.
"""

from . import seed_loop as seed_l

JOBS = "/api/instances/loop/jobs/"

JOB_KEYS = {
    "id", "slug", "name", "public_name", "public_description", "prompt",
    "min_role", "enabled", "is_builtin", "dtstart", "rrule", "tzid",
    "created_at", "updated_at",
}


def detail(job_id: str) -> str:
    return f"{JOBS}{job_id}/"


def valid_payload(slug: str, **kw) -> dict:
    payload = {
        "slug": slug,
        "name": "Admin name",
        "public_name": "Public name",
        "prompt": "Do the thing.",
        "rrule": "FREQ=DAILY;BYHOUR=2",
    }
    payload.update(kw)
    return payload


def test_list_shape_and_order(org):
    first = seed_l.create_loop_job(org.conn, slug=f"la-{org.tag}", public_name="LA")
    second = seed_l.create_loop_job(org.conn, slug=f"lb-{org.tag}", public_name="LB")
    r = org.adm("GET", JOBS, "admin")
    assert r.status_code == 200, r.text
    rows = {j["slug"]: j for j in r.json()}
    assert first["slug"] in rows and second["slug"] in rows
    for job in rows.values():
        assert set(job.keys()) == JOB_KEYS
    # Newest first.
    ids = [j["id"] for j in r.json()]
    assert ids.index(second["id"]) < ids.index(first["id"])
    # The admin surface exposes the prompt (the user surface never does).
    assert rows[first["slug"]]["prompt"] == "Do the thing."


def test_list_includes_builtin_seed_row(org):
    # Migration 0149 seeds exactly one builtin job; the admin list shows it
    # (even while disabled) with its full shape.
    r = org.adm("GET", JOBS, "admin")
    rows = [j for j in r.json() if j["slug"] == "auto-close-merged"]
    assert len(rows) == 1
    assert set(rows[0].keys()) == JOB_KEYS
    assert rows[0]["is_builtin"] is True


def test_create_defaults(org):
    r = org.adm("POST", JOBS, "admin", json=valid_payload(f"cd-{org.tag}"))
    assert r.status_code == 201, r.text
    body = r.json()
    assert set(body.keys()) == JOB_KEYS
    assert body["public_description"] == ""
    assert body["min_role"] == 15
    assert body["enabled"] is True
    assert body["is_builtin"] is False
    assert body["tzid"] == "UTC"
    assert body["dtstart"] is not None
    row = org.conn.execute(
        "SELECT is_builtin, enabled FROM loop_jobs WHERE slug=%s", (f"cd-{org.tag}",)
    ).fetchone()
    assert row == (False, True)


def test_create_rejects_subhourly_rrule(org):
    r = org.adm(
        "POST", JOBS, "admin",
        json=valid_payload(f"fast-{org.tag}", rrule="FREQ=MINUTELY"),
    )
    assert r.status_code == 400
    assert r.json() == {"error": "rrule_too_frequent"}


def test_create_rejects_bad_slug(org):
    r = org.adm(
        "POST", JOBS, "admin", json=valid_payload(f"Bad Slug {org.tag}")
    )
    assert r.status_code == 400
    assert r.json() == {"error": "invalid_slug"}


def test_create_rejects_bad_min_role(org):
    r = org.adm(
        "POST", JOBS, "admin", json=valid_payload(f"mr-{org.tag}", min_role=7)
    )
    assert r.status_code == 400
    assert r.json() == {"error": "invalid_min_role"}


def test_create_rejects_bad_rrule(org):
    r = org.adm(
        "POST", JOBS, "admin",
        json=valid_payload(f"rr-{org.tag}", rrule="FREQ=DAILY;BYHOUR=xx"),
    )
    assert r.status_code == 400
    body = r.json()
    assert body["error"] == "invalid_rrule"
    assert body["detail"] == "invalid RRULE: invalid 'BYHOUR': XX"


def test_create_rejects_empty_rrule(org):
    r = org.adm(
        "POST", JOBS, "admin", json=valid_payload(f"er-{org.tag}", rrule="")
    )
    assert r.status_code == 400
    assert r.json() == {"error": "invalid_rrule", "detail": "rrule is required"}


def test_create_missing_fields(org):
    r = org.adm("POST", JOBS, "admin", json={"slug": f"mf-{org.tag}"})
    assert r.status_code == 400
    assert r.json() == {
        "error": "missing_fields",
        "detail": ["name", "prompt", "public_name", "rrule"],
    }


def test_create_duplicate_slug_409(org):
    seed_l.create_loop_job(org.conn, slug=f"dup-{org.tag}", public_name="D")
    r = org.adm("POST", JOBS, "admin", json=valid_payload(f"dup-{org.tag}"))
    assert r.status_code == 409
    assert r.json() == {"error": "slug_taken"}


def test_create_min_role_string_coerced(org):
    # ``int()`` validation accepts a numeric string and the raw value is
    # echoed back in the create response...
    r = org.adm(
        "POST", JOBS, "admin",
        json=valid_payload(f"ms-{org.tag}", min_role="15"),
    )
    assert r.status_code == 201, r.text
    assert r.json()["min_role"] == "15"
    # ...while a later read returns the stored integer.
    job_id = r.json()["id"]
    assert org.adm("GET", detail(job_id), "admin").json()["min_role"] == 15


def test_create_min_role_garbage_500(org):
    # ``int("abc")`` raises through the validator into the generic 500 handler.
    r = org.adm(
        "POST", JOBS, "admin",
        json=valid_payload(f"mg-{org.tag}", min_role="abc"),
    )
    assert r.status_code == 500
    assert r.json() == {"error": "Something went wrong please try again later"}


def test_create_min_role_float_echoed(org):
    # ``int(15.5)`` passes validation and the raw float is echoed back.
    r = org.adm(
        "POST", JOBS, "admin",
        json=valid_payload(f"mfl-{org.tag}", min_role=15.5),
    )
    assert r.status_code == 201, r.text
    assert r.json()["min_role"] == 15.5


def test_detail_shape_with_stats(org):
    job = seed_l.create_loop_job(org.conn, slug=f"ds-{org.tag}", public_name="DS")
    r = org.adm("GET", detail(job["id"]), "admin")
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body.keys()) == JOB_KEYS | {"stats"}
    assert body["stats"] == {
        "target_count": 0, "completed": 0, "failed": 0, "skipped": 0,
    }


def test_detail_stats_rollup(org):
    job = seed_l.create_loop_job(org.conn, slug=f"sr-{org.tag}", public_name="SR")
    thread = seed_l.create_thread(
        org.conn, workspace_id=org.workspace["id"], user_id=org.member["id"]
    )
    recent_ok = seed_l.create_turn(
        org.conn, thread_id=thread["id"], status="completed",
        usage_total_tokens=10, completed_at=seed_l.hours_ago(1),
    )
    recent_failed = seed_l.create_turn(
        org.conn, thread_id=thread["id"], status="failed",
        error_code="provider_unreachable", completed_at=seed_l.hours_ago(2),
    )
    old_ok = seed_l.create_turn(
        org.conn, thread_id=thread["id"], status="completed",
        completed_at=seed_l.hours_ago(25),
    )
    seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.workspace["id"],
        user_id=org.member["id"], last_run_id=recent_ok["id"],
    )
    seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.workspace["id"],
        user_id=org.admin["id"], last_run_id=recent_failed["id"],
    )
    seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.workspace["id"],
        user_id=org.guest["id"], last_run_id=old_ok["id"],
    )
    seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.workspace["id"],
        user_id=org.outsider["id"], last_skipped_at=seed_l.hours_ago(3),
        last_skip_reason="llm_config_missing",
    )
    seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.workspace["id"],
        user_id=org.other_user["id"], last_skipped_at=seed_l.hours_ago(30),
        last_skip_reason="llm_config_missing",
    )
    stats = org.adm("GET", detail(job["id"]), "admin").json()["stats"]
    # The 24h rollup counts only recent runs: the 25h-old completion and the
    # 30h-old skip fall outside the window.
    assert stats == {
        "target_count": 5, "completed": 1, "failed": 1, "skipped": 1,
    }


def test_detail_unknown_404(org):
    import uuid as _uuid

    r = org.adm("GET", detail(str(_uuid.uuid4())), "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "not_found"}


def test_patch_updates_and_ignores_unknown_keys(org):
    job = seed_l.create_loop_job(
        org.conn, slug=f"pu-{org.tag}", public_name="PU", is_builtin=True
    )
    r = org.adm(
        "PATCH", detail(job["id"]), "admin",
        json={"is_builtin": False, "enabled": False, "bogus": 1},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body.keys()) == JOB_KEYS
    # ``is_builtin`` is immutable through the API; unknown keys are dropped.
    assert body["is_builtin"] is True
    assert body["enabled"] is False
    row = org.conn.execute(
        "SELECT is_builtin, enabled FROM loop_jobs WHERE id=%s", (job["id"],)
    ).fetchone()
    assert row == (True, False)


def test_patch_same_slug_ok_clash_409(org):
    first = seed_l.create_loop_job(org.conn, slug=f"p1-{org.tag}", public_name="P")
    seed_l.create_loop_job(org.conn, slug=f"p2-{org.tag}", public_name="P")
    r = org.adm(
        "PATCH", detail(first["id"]), "admin", json={"slug": f"p1-{org.tag}"}
    )
    assert r.status_code == 200
    r = org.adm(
        "PATCH", detail(first["id"]), "admin", json={"slug": f"p2-{org.tag}"}
    )
    assert r.status_code == 409
    assert r.json() == {"error": "slug_taken"}


def test_patch_enabled_type_error_body(org):
    # The admin write path does not validate ``enabled`` itself: a non-bool
    # reaches the model layer and comes back with a different error body than
    # the user surface's ``invalid_payload``.
    job = seed_l.create_loop_job(org.conn, slug=f"pe-{org.tag}", public_name="P")
    r = org.adm("PATCH", detail(job["id"]), "admin", json={"enabled": "yes"})
    assert r.status_code == 400
    assert r.json() == {"error": "Please provide valid detail"}


def test_delete_soft_deletes_and_frees_slug(org):
    job = seed_l.create_loop_job(org.conn, slug=f"dl-{org.tag}", public_name="DL")
    assert org.adm("DELETE", detail(job["id"]), "admin").status_code == 204
    r = org.adm("GET", detail(job["id"]), "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "not_found"}
    row = org.conn.execute(
        "SELECT deleted_at FROM loop_jobs WHERE id=%s", (job["id"],)
    ).fetchone()
    assert row[0] is not None
    # The conditional unique lets a new active row reuse the slug; deleting
    # again is a 404, not a second delete.
    r = org.adm("POST", JOBS, "admin", json=valid_payload(f"dl-{org.tag}"))
    assert r.status_code == 201, r.text
    assert r.json()["id"] != job["id"]
    assert org.adm("DELETE", detail(job["id"]), "admin").status_code == 404
