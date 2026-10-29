"""``Issue.work_type`` on the app issue surface (PDASHOSS01-234).

``IssueCreateSerializer.validate`` normalizes ``""`` to the inherit state
(NULL) and rejects an unknown key with the list-wrapped registry message;
the app ``IssueSerializer`` / ``IssueDetailSerializer`` render the key right
after ``agent_executor``. Each test creates (and deletes) its own issue so
the shared seed stays pristine.
"""

UNKNOWN_BODY = {"work_type": ["unknown work type; expected one of ['general', 'software']"]}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _after_executor(body):
    keys = list(body)
    return keys[keys.index("agent_executor") + 1]


def test_retrieve_renders_work_type_after_agent_executor(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    body = admin.get(f"{_base(ws, pid)}/issues/{i1}/").json()
    assert _after_executor(body) == "work_type"
    assert body["work_type"] is None


def test_create_patch_put_work_type(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    created = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "Typed", "work_type": "general"}
    )
    assert created.status_code == 201, created.text
    issue_id = created.json()["id"]
    url = f"{_base(ws, pid)}/issues/{issue_id}/"
    try:
        assert admin.get(url).json()["work_type"] == "general"
        # PATCH: "" normalizes back to the inherit state.
        resp = admin.patch(url, json={"work_type": ""})
        assert resp.status_code == 204, resp.text
        assert admin.get(url).json()["work_type"] is None
        resp = admin.patch(url, json={"work_type": "software"})
        assert resp.status_code == 204, resp.text
        assert admin.get(url).json()["work_type"] == "software"
        resp = admin.patch(url, json={"work_type": None})
        assert resp.status_code == 204, resp.text
        assert admin.get(url).json()["work_type"] is None
        # PUT renders the create serializer: the key sits after
        # agent_executor and carries the normalized value.
        resp = admin.put(url, json={"name": "Typed v2", "work_type": "general"})
        assert resp.status_code == 200, resp.text
        assert _after_executor(resp.json()) == "work_type"
        assert resp.json()["work_type"] == "general"
        resp = admin.put(url, json={"name": "Typed v3", "work_type": ""})
        assert resp.status_code == 200, resp.text
        assert resp.json()["work_type"] is None
    finally:
        assert admin.delete(url).status_code == 204


def test_unknown_work_type_400s(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "Bad", "work_type": "carpentry"}
    )
    assert resp.status_code == 400
    assert resp.json() == UNKNOWN_BODY
    created = admin.post(f"{_base(ws, pid)}/issues/", json={"name": "Typed patch"})
    assert created.status_code == 201, created.text
    url = f"{_base(ws, pid)}/issues/{created.json()['id']}/"
    try:
        resp = admin.patch(url, json={"work_type": "carpentry"})
        assert resp.status_code == 400
        assert resp.json() == UNKNOWN_BODY
        resp = admin.patch(url, json={"work_type": "x" * 33})
        assert resp.status_code == 400
        assert resp.json() == {
            "work_type": ["Ensure this field has no more than 32 characters."]
        }
        assert admin.get(url).json()["work_type"] is None
    finally:
        assert admin.delete(url).status_code == 204
