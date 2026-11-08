"""Seeders supply the dedicated-runner NOT NULL columns (NEWPIDASH-144).

Migrations db/0173 (``projects.dedicated_runner_lost_action``) and
runner/0039 (``agent_run.pin_kind``) are NOT NULL with Python-only
defaults. Raw-SQL seeders must supply them or the suite fails at seed
time with ``NotNullViolation``. Needs no backend.
"""

from pathlib import Path

from _harness import db

_ROOT = Path(__file__).resolve().parents[2]


def _py_files():
    return sorted(_ROOT.rglob("*.py"))


def _texts():
    return {p: p.read_text() for p in _py_files()}


def test_insert_row_defaults_mirror_django_defaults():
    assert db._TABLE_DEFAULTS["projects"]["dedicated_runner_lost_action"] == "fail_to_backlog"
    assert db._TABLE_DEFAULTS["agent_run"]["pin_kind"] == "locality"


def test_projects_inserts_supply_dedicated_runner_lost_action():
    # PROJECT_COLUMNS is defined once in app_issues/conftest.py and
    # imported by test_epics/test_issue_bulk/test_issue_types; those
    # importers carry no inline column and are covered by the shared
    # definition (checked here via the conftest text).
    conftest = (_ROOT / "app_issues" / "conftest.py").read_text()
    assert "dedicated_runner_lost_action" in conftest
    offenders = []
    for path, text in _texts().items():
        if "insert into projects" not in text.lower():
            continue
        if "dedicated_runner_lost_action" in text:
            continue
        # Importers of the shared PROJECT_COLUMNS/_project_row are covered
        # by the conftest definition verified above.
        if "PROJECT_COLUMNS" in text and ".conftest" in text:
            continue
        if "PROJECT_COLUMNS" in text and "app_issues.conftest" in text:
            continue
        offenders.append(str(path.relative_to(_ROOT)))
    assert not offenders, f"projects INSERTs missing column: {offenders}"


def test_agent_run_inserts_supply_pin_kind():
    offenders = []
    for path, text in _texts().items():
        if "insert into agent_run" not in text.lower():
            continue
        if "pin_kind" not in text:
            offenders.append(str(path.relative_to(_ROOT)))
    assert not offenders, f"agent_run INSERTs missing column: {offenders}"
