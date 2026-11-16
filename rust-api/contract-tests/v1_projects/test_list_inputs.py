"""Contract tests: v1_projects list/array field inputs (PIDASHCONV-1214).

The project write views spread ``{**request.data}``, so a form/multipart
body hands the serializer ``{key: [values]}`` -- every field a list -- and
a JSON body may carry an explicit array. Each DRF field type then answers
with *its own* list-input error, which the Rust coercers must reproduce
byte for byte:

- ``ChoiceField`` (``timezone``, ``default_agent_executor``) does
  ``str(data)``: a list renders as Python ``repr`` (``['UTC']``) and the
  miss echoes that quoted repr.
- ``PrimaryKeyRelatedField`` over a UUID pk (``project_lead``,
  ``default_assignee``, ``default_state``, ``estimate``) hands the list to
  ``UUIDField.to_python``, whose miss renders the value inside curly quotes
  (``“['']” is not a valid UUID.``).
- ``BooleanField`` / ``IntegerField`` already answer their scalar type
  error for a list (no divergence -- pinned here so the sweep is covered).
- ``JSONField`` (``icon_prop``) accepts any JSON value, list included.

Every test runs against Django and against the Rust server with identical
expectations, and each error body is asserted exactly (byte for byte).
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http  # noqa: E402

KEY = "projects"

FORM = {"Content-Type": "application/x-www-form-urlencoded"}


def base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects"


def detail(seed):
    return f"{base(seed)}/{seed['project']['id']}/"


def _form_patch(seed, body):
    with http.client(seed["keys"][KEY]) as c:
        return c.patch(detail(seed), content=body, headers=FORM)


def _json_patch(seed, payload):
    with http.client(seed["keys"][KEY]) as c:
        return c.patch(detail(seed), json=payload)


# --- ChoiceField: str(list) -> Python list repr in the message ---------------


def test_timezone_form_value(seed):
    r = _form_patch(seed, "timezone=UTC")
    assert r.status_code == 400, r.text
    assert r.json() == {"timezone": ["\"['UTC']\" is not a valid choice."]}


def test_timezone_json_array(seed):
    r = _json_patch(seed, {"timezone": ["UTC"]})
    assert r.status_code == 400, r.text
    assert r.json() == {"timezone": ["\"['UTC']\" is not a valid choice."]}


def test_default_agent_executor_form_value(seed):
    r = _form_patch(seed, "default_agent_executor=local_runner")
    assert r.status_code == 400, r.text
    assert r.json() == {
        "default_agent_executor": ["\"['local_runner']\" is not a valid choice."]
    }


def test_default_agent_executor_json_array(seed):
    r = _json_patch(seed, {"default_agent_executor": ["local_runner"]})
    assert r.status_code == 400, r.text
    assert r.json() == {
        "default_agent_executor": ["\"['local_runner']\" is not a valid choice."]
    }


def test_default_agent_executor_apostrophe_uses_cpython_quotes(seed):
    # CPython repr picks double quotes for a string containing a single
    # quote (and no double quote): str(["it's"]) is `["it's"]`.
    r = _form_patch(seed, "default_agent_executor=it's")
    assert r.status_code == 400, r.text
    assert r.json() == {
        "default_agent_executor": ["\"[\"it's\"]\" is not a valid choice."]
    }


# --- PrimaryKeyRelatedField over a UUID pk: curly-quoted repr ----------------


def test_project_lead_form_value(seed):
    r = _form_patch(seed, "project_lead=")
    assert r.status_code == 400, r.text
    assert r.json() == {"project_lead": ["\u201c['']\u201d is not a valid UUID."]}


def test_project_lead_json_array(seed):
    r = _json_patch(seed, {"project_lead": [""]})
    assert r.status_code == 400, r.text
    assert r.json() == {"project_lead": ["\u201c['']\u201d is not a valid UUID."]}


def test_project_lead_apostrophe_uses_cpython_quotes(seed):
    r = _json_patch(seed, {"project_lead": ["it's"]})
    assert r.status_code == 400, r.text
    assert r.json() == {
        "project_lead": ["\u201c[\"it's\"]\u201d is not a valid UUID."]
    }


def test_default_assignee_form_value(seed):
    r = _form_patch(seed, "default_assignee=x")
    assert r.status_code == 400, r.text
    assert r.json() == {
        "default_assignee": ["\u201c['x']\u201d is not a valid UUID."]
    }


def test_default_assignee_json_array(seed):
    r = _json_patch(seed, {"default_assignee": ["x"]})
    assert r.status_code == 400, r.text
    assert r.json() == {
        "default_assignee": ["\u201c['x']\u201d is not a valid UUID."]
    }


def test_default_state_form_value(seed):
    r = _form_patch(seed, "default_state=")
    assert r.status_code == 400, r.text
    assert r.json() == {"default_state": ["\u201c['']\u201d is not a valid UUID."]}


def test_default_state_json_array(seed):
    r = _json_patch(seed, {"default_state": [""]})
    assert r.status_code == 400, r.text
    assert r.json() == {"default_state": ["\u201c['']\u201d is not a valid UUID."]}


def test_estimate_form_value(seed):
    r = _form_patch(seed, "estimate=")
    assert r.status_code == 400, r.text
    assert r.json() == {"estimate": ["\u201c['']\u201d is not a valid UUID."]}


def test_estimate_json_array(seed):
    r = _json_patch(seed, {"estimate": [""]})
    assert r.status_code == 400, r.text
    assert r.json() == {"estimate": ["\u201c['']\u201d is not a valid UUID."]}


# --- BooleanField / IntegerField / JSONField sweep ---------------------------


def test_boolean_form_value(seed):
    r = _form_patch(seed, "is_issue_type_enabled=true")
    assert r.status_code == 400, r.text
    assert r.json() == {"is_issue_type_enabled": ["Must be a valid boolean."]}


def test_boolean_json_array(seed):
    r = _json_patch(seed, {"is_issue_type_enabled": [True]})
    assert r.status_code == 400, r.text
    assert r.json() == {"is_issue_type_enabled": ["Must be a valid boolean."]}


def test_integer_form_value(seed):
    r = _form_patch(seed, "archive_in=1")
    assert r.status_code == 400, r.text
    assert r.json() == {"archive_in": ["A valid integer is required."]}


def test_integer_json_array(seed):
    r = _json_patch(seed, {"archive_in": [1]})
    assert r.status_code == 400, r.text
    assert r.json() == {"archive_in": ["A valid integer is required."]}


def test_jsonfield_json_array_accepted(seed):
    # JSONField takes any JSON value: a list is stored as-is, not an error.
    r = _json_patch(seed, {"icon_prop": [{"a": 1}]})
    assert r.status_code == 200, r.text
    assert r.json()["icon_prop"] == [{"a": 1}]


# --- create path carries the same list-input errors --------------------------


def test_timezone_form_value_on_create(seed):
    # A form body makes *every* field a list, so name/identifier answer their
    # own "Not a valid string." in field order before timezone's choice miss.
    with http.client(seed["keys"][KEY]) as c:
        r = c.post(
            base(seed) + "/",
            content="name=F&identifier=F1&timezone=UTC",
            headers=FORM,
        )
    assert r.status_code == 400, r.text
    assert r.json() == {
        "name": ["Not a valid string."],
        "identifier": ["Not a valid string."],
        "timezone": ["\"['UTC']\" is not a valid choice."],
    }


def test_project_lead_json_array_on_create(seed):
    with http.client(seed["keys"][KEY]) as c:
        r = c.post(
            base(seed) + "/",
            json={"name": "CT1214", "identifier": "CT1214", "project_lead": [""]},
        )
    assert r.status_code == 400, r.text
    assert r.json() == {"project_lead": ["\u201c['']\u201d is not a valid UUID."]}
