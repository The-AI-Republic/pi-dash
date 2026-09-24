"""Static contract parity: task options and beat schedule from Django source.

Parses the Django task modules with ``ast`` — the suite never imports Django.
Each group pins its tasks' wire names, decorator options (ack/retry/ETA
semantics), fan-out call sites, and the beat entries it owns. Any drift in
the Python source fails loudly here, which is exactly what the Rust port must
track.

Expected option values are Python source fragments, compared by normalized
AST so formatting differences do not matter.
"""

import ast
from pathlib import Path

from . import config


def source_root() -> Path:
    override = __import__("os").environ.get(config.PI_DASH_SOURCE_DIR)
    if override:
        return Path(override)
    # rust-api/contract-tests/_harness -> repo root -> apps/api
    return Path(__file__).resolve().parent.parent.parent.parent / "apps" / "api"


def _norm(fragment: str) -> str:
    return ast.dump(ast.parse(fragment, mode="eval").body)


def _decorator_options(source: str, func_name: str) -> dict:
    """Return {keyword: normalized-ast} for the @shared_task on func_name."""
    module = ast.parse(source)
    for node in module.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == func_name:
            for dec in node.decorator_list:
                if isinstance(dec, ast.Name) and dec.id == "shared_task":
                    return {}
                if isinstance(dec, ast.Call) and getattr(dec.func, "id", "") == "shared_task":
                    return {kw.arg: ast.dump(kw.value) for kw in dec.keywords}
    raise AssertionError(f"@{func_name}: no @shared_task decorator found")


def assert_task_options(module_relpath: str, func_name: str, expected: dict) -> None:
    """Ack/retry parity: the decorator carries exactly these options.

    ``expected`` maps keyword -> source fragment, e.g.
    ``{"max_retries": "5", "autoretry_for": "(requests.RequestException,)"}``.
    An omitted keyword means "must be absent" is NOT implied — only listed
    keywords are checked; use ``assert_no_option`` for absence.
    """
    path = source_root() / module_relpath
    assert path.exists(), f"Django source not found: {path}"
    actual = _decorator_options(path.read_text(), func_name)
    for keyword, fragment in expected.items():
        assert keyword in actual, (
            f"{module_relpath}::{func_name}: missing decorator option {keyword!r} "
            f"(actual options: {sorted(actual)})"
        )
        assert actual[keyword] == _norm(fragment), (
            f"{module_relpath}::{func_name}: option {keyword!r} drifted "
            f"(expected {fragment!r})"
        )


def assert_no_option(module_relpath: str, func_name: str, keyword: str) -> None:
    """Assert a decorator keyword is absent (e.g. no acks_late override)."""
    path = source_root() / module_relpath
    actual = _decorator_options(path.read_text(), func_name)
    assert keyword not in actual, (
        f"{module_relpath}::{func_name}: unexpected decorator option {keyword!r} "
        f"= {actual[keyword]}"
    )


def assert_calls_delay(module_relpath: str, func_name: str, callee: str) -> None:
    """Fan-out parity: func_name's body calls ``<callee>.delay(...)``."""
    path = source_root() / module_relpath
    module = ast.parse(path.read_text())
    for node in module.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == func_name:
            for child in ast.walk(node):
                if (
                    isinstance(child, ast.Call)
                    and isinstance(child.func, ast.Attribute)
                    and child.func.attr == "delay"
                    and isinstance(child.func.value, ast.Name)
                    and child.func.value.id == callee
                ):
                    return
    raise AssertionError(f"{module_relpath}::{func_name}: no {callee}.delay(...) call found")


def assert_delay_kwarg(
    module_relpath: str, func_name: str, callee: str, kwarg: str
) -> None:
    """ETA parity: a ``<callee>.delay(..., <kwarg>=...)`` call site exists."""
    path = source_root() / module_relpath
    module = ast.parse(path.read_text())
    for node in module.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == func_name:
            for child in ast.walk(node):
                if (
                    isinstance(child, ast.Call)
                    and isinstance(child.func, ast.Attribute)
                    and child.func.attr == "delay"
                    and isinstance(child.func.value, ast.Name)
                    and child.func.value.id == callee
                    and any(kw.arg == kwarg for kw in child.keywords)
                ):
                    return
    raise AssertionError(
        f"{module_relpath}::{func_name}: no {callee}.delay(..., {kwarg}=...) call found"
    )


def load_beat_schedule() -> dict:
    """Return {entry_name: {"task": ..., "schedule": <normalized ast>}}.

    Reads the literal ``app.conf.beat_schedule`` dict in celery.py. Entries
    added dynamically (settings-backed cloud/runner sweeps) are not literals
    and are out of scope for the D-07..D-10 oracle.
    """
    path = source_root() / "pi_dash" / "celery.py"
    assert path.exists(), f"Django source not found: {path}"
    module = ast.parse(path.read_text())
    for node in ast.walk(module):
        if (
            isinstance(node, ast.Assign)
            and len(node.targets) == 1
            and isinstance(node.targets[0], ast.Attribute)
            and node.targets[0].attr == "beat_schedule"
            and isinstance(node.value, ast.Dict)
        ):
            entries = {}
            for key, value in zip(node.value.keys, node.value.values):
                name = ast.literal_eval(key)
                task = schedule = None
                for k, v in zip(value.keys, value.values):
                    field = ast.literal_eval(k)
                    if field == "task":
                        task = ast.literal_eval(v)
                    elif field == "schedule":
                        schedule = ast.dump(v)
                entries[name] = {"task": task, "schedule": schedule}
            return entries
    raise AssertionError("pi_dash/celery.py: literal beat_schedule dict not found")


def assert_beat_entry(entry_name: str, task: str, schedule_fragment: str) -> None:
    """Beat-firing parity: the entry exists with this task and cadence."""
    entries = load_beat_schedule()
    assert entry_name in entries, (
        f"beat entry {entry_name!r} missing "
        f"(known entries: {sorted(entries)})"
    )
    actual = entries[entry_name]
    assert actual["task"] == task, (
        f"beat entry {entry_name!r}: task is {actual['task']!r}, expected {task!r}"
    )
    assert actual["schedule"] == _norm(schedule_fragment), (
        f"beat entry {entry_name!r}: schedule drifted (expected {schedule_fragment!r})"
    )
