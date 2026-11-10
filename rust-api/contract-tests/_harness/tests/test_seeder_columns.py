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


def _split_sql_list(body):
    """Split a SQL list on top-level commas, respecting single-quoted literals."""
    parts = []
    current = []
    in_quote = False
    i = 0
    while i < len(body):
        ch = body[i]
        if ch == "'":
            if in_quote and i + 1 < len(body) and body[i + 1] == "'":
                current.append("''")
                i += 2
                continue
            in_quote = not in_quote
            current.append(ch)
        elif ch == "," and not in_quote:
            parts.append("".join(current).strip())
            current = []
        else:
            current.append(ch)
        i += 1
    parts.append("".join(current).strip())
    return [p for p in parts if p]


def test_dispatch_agent_run_insert_arity():
    # NEWPIDASH-163: the pin_kind column was added without its value,
    # so the INSERT carried 36 target columns for 35 expressions and
    # every dispatch seed died with "INSERT has more target columns
    # than expressions". agent_metadata/usage are JSONField columns
    # (Django default dict) and must stay '{}', not ''.
    import re

    text = (_ROOT / "dispatch" / "conftest.py").read_text()
    m = re.search(
        r"INSERT INTO agent_run\s*\((.*?)\)\s*VALUES\s*\((.*?)\)", text, re.S
    )
    assert m, "agent_run INSERT not found in dispatch/conftest.py"
    cols = [c.strip() for c in m.group(1).split(",")]
    vals = _split_sql_list(m.group(2))
    assert len(cols) == len(vals), (
        f"agent_run INSERT arity: {len(cols)} columns vs {len(vals)} expressions"
    )
    by_col = dict(zip(cols, vals))
    assert by_col["pin_kind"] == "'locality'"
    assert by_col["agent_metadata"] == "'{}'"
    assert by_col["usage"] == "'{}'"
    assert by_col["phase_kind"] == "''"


def test_agent_run_inserts_supply_pin_kind():
    offenders = []
    for path, text in _texts().items():
        if "insert into agent_run" not in text.lower():
            continue
        if "pin_kind" not in text:
            offenders.append(str(path.relative_to(_ROOT)))
    assert not offenders, f"agent_run INSERTs missing column: {offenders}"
