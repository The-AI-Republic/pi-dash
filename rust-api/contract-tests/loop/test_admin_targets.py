"""Run cursors: ``GET`` ``/api/instances/loop/jobs/<uuid>/targets/``.

One page of at most 50 rows, newest first, with ``skip_reason`` /
``workspace`` / ``status`` filters. Unknown filter values are ignored (except
an unknown workspace slug, which matches nothing); a bad page falls back to 1.
"""

from _harness import seed

from . import seed_loop as seed_l

JOBS = "/api/instances/loop/jobs/"

ROW_KEYS = {
    "id", "workspace_slug", "user_email", "next_run_at", "last_skipped_at",
    "last_skip_reason", "last_run",
}
RUN_KEYS = {"status", "error_code", "model_used", "total_tokens", "completed_at"}


def targets(job_id: str) -> str:
    return f"{JOBS}{job_id}/targets/"


def seed_row(org, job, user, **kw):
    seed_l.configure_llm(org.conn, user_id=user["id"])
    return seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.workspace["id"],
        user_id=user["id"], next_run_at=seed_l.hours_ago(1), **kw
    )


def test_row_shapes_with_and_without_run(org):
    job = seed_l.create_loop_job(org.conn, slug=f"tr-{org.tag}", public_name="TR")
    thread = seed_l.create_thread(
        org.conn, workspace_id=org.workspace["id"], user_id=org.member["id"]
    )
    turn = seed_l.create_turn(
        org.conn, thread_id=thread["id"], status="completed",
        usage_total_tokens=42, model_used="gpt-test",
        completed_at=seed_l.hours_ago(1),
    )
    seed_row(
        org, job, org.member, thread_id=thread["id"], last_run_id=turn["id"]
    )
    seed_row(
        org, job, org.admin, last_skipped_at=seed_l.hours_ago(2),
        last_skip_reason="llm_config_missing",
    )
    r = org.adm("GET", targets(job["id"]), "admin")
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body.keys()) == {"page", "results"}
    assert body["page"] == 1
    assert len(body["results"]) == 2
    for row in body["results"]:
        assert set(row.keys()) == ROW_KEYS
    by_email = {t["user_email"]: t for t in body["results"]}
    ran = by_email[org.member["email"]]
    assert ran["workspace_slug"] == org.workspace["slug"]
    assert ran["next_run_at"] is not None
    assert ran["last_skipped_at"] is None
    assert ran["last_skip_reason"] == ""
    assert set(ran["last_run"].keys()) == RUN_KEYS
    assert ran["last_run"]["status"] == "completed"
    assert ran["last_run"]["error_code"] == ""
    assert ran["last_run"]["model_used"] == "gpt-test"
    assert ran["last_run"]["total_tokens"] == 42
    assert ran["last_run"]["completed_at"] is not None
    skipped = by_email[org.admin["email"]]
    assert skipped["last_run"] is None
    assert skipped["last_skip_reason"] == "llm_config_missing"
    assert skipped["last_skipped_at"] is not None


def test_empty_targets(org):
    job = seed_l.create_loop_job(org.conn, slug=f"te-{org.tag}", public_name="TE")
    assert org.adm("GET", targets(job["id"]), "admin").json() == {
        "page": 1, "results": [],
    }


def test_unknown_job_404(org):
    import uuid as _uuid

    r = org.adm("GET", targets(str(_uuid.uuid4())), "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "not_found"}


def test_filters(org):
    job = seed_l.create_loop_job(org.conn, slug=f"tf-{org.tag}", public_name="TF")
    seed_row(org, job, org.member)
    other = seed_l.create_target(
        org.conn, job_id=job["id"], workspace_id=org.other_ws["id"],
        user_id=org.other_user["id"], next_run_at=seed_l.hours_ago(1),
        last_skipped_at=seed_l.hours_ago(1), last_skip_reason="user_disabled",
    )
    assert other["id"] is not None

    r = org.adm(
        "GET", targets(job["id"]), "admin", params={"skip_reason": "user_disabled"}
    )
    assert [t["user_email"] for t in r.json()["results"]] == [org.other_user["email"]]

    r = org.adm(
        "GET", targets(job["id"]), "admin",
        params={"workspace": org.workspace["slug"]},
    )
    assert [t["user_email"] for t in r.json()["results"]] == [org.member["email"]]

    r = org.adm("GET", targets(job["id"]), "admin", params={"workspace": "nope"})
    assert r.json() == {"page": 1, "results": []}

    # Unknown filter values are ignored, not rejected.
    r = org.adm(
        "GET", targets(job["id"]), "admin", params={"skip_reason": "bogus"}
    )
    assert len(r.json()["results"]) == 2
    r = org.adm("GET", targets(job["id"]), "admin", params={"status": "bogus"})
    assert len(r.json()["results"]) == 2


def test_status_filter_needs_a_run(org):
    job = seed_l.create_loop_job(org.conn, slug=f"ts-{org.tag}", public_name="TS")
    thread = seed_l.create_thread(
        org.conn, workspace_id=org.workspace["id"], user_id=org.member["id"]
    )
    turn = seed_l.create_turn(org.conn, thread_id=thread["id"], status="failed")
    seed_row(org, job, org.member, thread_id=thread["id"], last_run_id=turn["id"])
    seed_row(org, job, org.guest)
    r = org.adm("GET", targets(job["id"]), "admin", params={"status": "failed"})
    assert [t["user_email"] for t in r.json()["results"]] == [org.member["email"]]
    r = org.adm("GET", targets(job["id"]), "admin", params={"status": "completed"})
    assert r.json()["results"] == []


def test_pagination(org):
    job = seed_l.create_loop_job(org.conn, slug=f"pg-{org.tag}", public_name="PG")
    for i in range(51):
        email = f"loop-page-{org.tag}-{i}@ct.example.com"
        user = seed.create_user(
            org.conn, email=email, username=email,
            password_field="pbkdf2_sha256$600000$aaaaaaaaaaaaaaaaaaaaaa$bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb=",
        )
        seed.add_workspace_member(
            org.conn, workspace_id=org.workspace["id"], user_id=user["id"], role=5,
        )
        seed_l.create_target(
            org.conn, job_id=job["id"], workspace_id=org.workspace["id"],
            user_id=user["id"], next_run_at=seed_l.hours_ago(1),
        )
    page1 = org.adm("GET", targets(job["id"]), "admin").json()
    assert page1["page"] == 1
    assert len(page1["results"]) == 50
    page2 = org.adm("GET", targets(job["id"]), "admin", params={"page": 2}).json()
    assert page2["page"] == 2
    assert len(page2["results"]) == 1
    # A bad page falls back to 1 instead of erroring.
    bad = org.adm("GET", targets(job["id"]), "admin", params={"page": "xx"}).json()
    assert bad["page"] == 1
    assert len(bad["results"]) == 50
