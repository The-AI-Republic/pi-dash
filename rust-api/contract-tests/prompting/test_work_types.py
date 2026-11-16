"""Work-type axis on the prompting endpoints (PDASHOSS01-234).

Routes (apps/api/pi_dash/prompting/urls.py):
  GET  /api/workspaces/<slug>/prompt-sections?kind=&scope=&work_type=
  GET  /api/workspaces/<slug>/prompts/<kind>/compiled?scope=&work_type=
  POST /api/workspaces/<slug>/prompts/<kind>/preview  {issue_id, work_type?}

The stage recipe carries named slots filled by the work type: ``software``
plugs today's git/PR guidance in, ``general`` keeps every stage prompt free
of it. An absent ``work_type`` means the default (``software``) on the
list/compiled endpoints and the issue's effective work type on preview
(issue override → project default → ``general``).
"""

import re

import pytest

UNKNOWN_BODY = {
    "error": "unknown work type 'bogus'",
    "work_types": ["general", "software"],
}

# Code/git/PR wording the general work type must never carry (the Django
# neutrality test's word list, case-insensitive part).
GIT_WORDS = re.compile(
    r"\b(git|branch(es|ed|ing)?|commit(s|ted|ting)?|push(es|ed|ing)?|"
    r"rebase[sd]?|checkout)\b|pull request|merge request|code review",
    re.IGNORECASE,
)


def _list_url(tenant, kind="coding-task", work_type=None):
    url = (f"/api/workspaces/{tenant['workspace']['slug']}/prompt-sections"
           f"?kind={kind}")
    return url + (f"&work_type={work_type}" if work_type is not None else "")


def _compiled_url(tenant, kind="coding-task", work_type=None):
    url = (f"/api/workspaces/{tenant['workspace']['slug']}"
           f"/prompts/{kind}/compiled")
    return url + (f"?work_type={work_type}" if work_type is not None else "")


def _preview_url(tenant, kind):
    return (f"/api/workspaces/{tenant['workspace']['slug']}"
            f"/prompts/{kind}/preview")


def _set(seed, table, column, value, row_id):
    with seed.conn.cursor() as cur:
        cur.execute(f"UPDATE {table} SET {column} = %s WHERE id = %s",
                    (value, row_id))


@pytest.mark.parametrize(
    "kind,slot_keys",
    [
        ("coding-task", ["software.context", "software.execute"]),
        ("review", ["software.context", "software.review"]),
        ("test", ["software.context", "software.test"]),
    ],
)
def test_list_default_work_type_is_software(api, tenant_a, auth_a, kind,
                                            slot_keys):
    r = api.get(_list_url(tenant_a, kind), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["work_type"] == "software"
    keys = [s["key"] for s in body["sections"]]
    assert [k for k in keys if "." in k] == slot_keys


@pytest.mark.parametrize(
    "kind,slot_keys",
    [
        ("coding-task", ["general.execute"]),
        ("review", ["general.review"]),
        ("test", ["general.test"]),
    ],
)
def test_list_general_fills_slots_without_context(api, tenant_a, auth_a,
                                                  kind, slot_keys):
    r = api.get(_list_url(tenant_a, kind, "general"), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["work_type"] == "general"
    keys = [s["key"] for s in body["sections"]]
    # `general` has no context slot: the entry is dropped, not left empty.
    assert [k for k in keys if "." in k] == slot_keys
    section = next(s for s in body["sections"] if s["key"] == slot_keys[0])
    assert section["source"] == "default"
    assert section["customizable"] == "overridable"


def test_list_empty_work_type_is_default(api, tenant_a, auth_a):
    r = api.get(_list_url(tenant_a, work_type=""), headers=auth_a)
    assert r.status_code == 200, r.text
    assert r.json()["work_type"] == "software"


def test_list_scheduler_ignores_work_type(api, tenant_a, auth_a):
    r = api.get(_list_url(tenant_a, "scheduler", "general"), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["work_type"] == "general"
    assert [s["key"] for s in body["sections"]] == [
        "scheduler-intro", "session-framing", "pidash-cli",
        "scheduler-task", "guardrails", "scheduler-ending",
    ]


def test_list_unknown_work_type_400(api, tenant_a, auth_a):
    r = api.get(_list_url(tenant_a, work_type="bogus"), headers=auth_a)
    assert r.status_code == 400, r.text
    assert r.json() == UNKNOWN_BODY


def test_list_unknown_kind_checked_before_work_type(api, tenant_a, auth_a):
    r = api.get(_list_url(tenant_a, kind="nope", work_type="bogus"),
                headers=auth_a)
    assert r.status_code == 400, r.text
    assert r.json()["error"] == "unknown kind 'nope'"


def test_compiled_work_type(api, tenant_a, auth_a):
    r = api.get(_compiled_url(tenant_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert list(body)[:3] == ["kind", "scope", "work_type"]
    assert body["work_type"] == "software"
    assert 'git checkout "$BASE"' in body["template_body"]
    r = api.get(_compiled_url(tenant_a, work_type="general"), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["work_type"] == "general"
    assert 'git checkout "$BASE"' not in body["template_body"]


def test_compiled_unknown_work_type_400(api, tenant_a, auth_a):
    r = api.get(_compiled_url(tenant_a, work_type="bogus"), headers=auth_a)
    assert r.status_code == 400, r.text
    assert r.json() == UNKNOWN_BODY


@pytest.mark.parametrize("kind", ["coding-task", "review", "test"])
def test_preview_general_has_no_git_wording(api, tenant_a, issue_a, write_a,
                                            kind):
    r = api.post(_preview_url(tenant_a, kind), headers=write_a,
                 json={"issue_id": str(issue_a), "work_type": "general"})
    assert r.status_code == 200, r.text
    body = r.json()
    assert list(body) == ["kind", "work_type", "prompt"]
    assert body["work_type"] == "general"
    hits = sorted({m.group(0).lower() for m in GIT_WORDS.finditer(body["prompt"])})
    assert hits == [], hits


def test_preview_software_keeps_coding_guidance(api, tenant_a, issue_a,
                                                write_a):
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_a), "work_type": "software"})
    assert r.status_code == 200, r.text
    prompt = r.json()["prompt"]
    assert 'git checkout "$BASE"' in prompt
    assert "pidash issue attach-review" in prompt


def test_preview_defaults_to_the_issues_effective_work_type(
        api, seed, tenant_a, project_a, issue_a, write_a):
    url = _preview_url(tenant_a, "coding-task")
    payload = {"issue_id": str(issue_a)}
    _set(seed, "projects", "default_work_type", "general", project_a)
    _set(seed, "issues", "work_type", None, issue_a)
    r = api.post(url, headers=write_a, json=payload)
    assert r.status_code == 200, r.text
    assert r.json()["work_type"] == "general"
    # The issue override wins over the project default.
    _set(seed, "issues", "work_type", "software", issue_a)
    r = api.post(url, headers=write_a, json=payload)
    assert r.status_code == 200, r.text
    assert r.json()["work_type"] == "software"
    # An unknown stored key falls through to the project default.
    _set(seed, "issues", "work_type", "retired-type", issue_a)
    r = api.post(url, headers=write_a, json=payload)
    assert r.status_code == 200, r.text
    assert r.json()["work_type"] == "general"
    # Blank project default with no override: the `general` safety net.
    _set(seed, "issues", "work_type", None, issue_a)
    _set(seed, "projects", "default_work_type", "", project_a)
    r = api.post(url, headers=write_a, json=payload)
    assert r.status_code == 200, r.text
    assert r.json()["work_type"] == "general"
    _set(seed, "projects", "default_work_type", "software", project_a)


def test_preview_explicit_falsy_work_type_is_the_default(
        api, seed, tenant_a, project_a, issue_a, write_a):
    """A present-but-falsy ``work_type`` is ``raw or DEFAULT`` (software),
    not the issue's effective work type."""
    _set(seed, "projects", "default_work_type", "general", project_a)
    try:
        r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                     json={"issue_id": str(issue_a), "work_type": ""})
        assert r.status_code == 200, r.text
        assert r.json()["work_type"] == "software"
        r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                     json={"issue_id": str(issue_a), "work_type": None})
        assert r.status_code == 200, r.text
        assert r.json()["work_type"] == "general"
    finally:
        _set(seed, "projects", "default_work_type", "software", project_a)


def test_preview_unknown_work_type_400(api, tenant_a, issue_a, write_a):
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_a), "work_type": "bogus"})
    assert r.status_code == 400, r.text
    assert r.json() == UNKNOWN_BODY


def test_preview_issue_lookup_precedes_work_type_check(api, tenant_a,
                                                       write_a):
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": "00000000-0000-0000-0000-000000000000",
                       "work_type": "bogus"})
    assert r.status_code == 404, r.text


def test_preview_draft_must_belong_to_the_expanded_recipe(api, tenant_a,
                                                          issue_a, write_a):
    url = _preview_url(tenant_a, "coding-task")
    r = api.post(url, headers=write_a,
                 json={"issue_id": str(issue_a), "work_type": "general",
                       "section_key": "software.execute", "body": "draft"})
    assert r.status_code == 400, r.text
    assert r.json() == {
        "error": "section 'software.execute' is not part of the 'coding-task' prompt"
    }
    r = api.post(url, headers=write_a,
                 json={"issue_id": str(issue_a), "work_type": "general",
                       "section_key": "general.execute",
                       "body": "Draft general guidance for {{ issue.identifier }}."})
    assert r.status_code == 200, r.text
    assert "Draft general guidance for" in r.json()["prompt"]


def test_preview_scheduler_reports_default_work_type(api, tenant_a,
                                                     binding_a, write_a):
    r = api.post(_preview_url(tenant_a, "scheduler"), headers=write_a,
                 json={"binding_id": str(binding_a)})
    assert r.status_code == 200, r.text
    assert r.json()["work_type"] == "software"
