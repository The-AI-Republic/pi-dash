# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Job parity: Celery tasks + beat schedule vs the Rust worker (PIDASHCONV-820).

Dumps every Celery task name Django registers plus the full
``beat_schedule`` (literal entries and the settings-backed entries from
``pi_dash/celery.py``), loads the Rust inventory from ``pidash-api jobs
--json``, and diffs task ownership and schedule cadences. CI
(``.github/workflows/rust-api-parity.yml``) fails when the live diff
contains anything not classified in ``EXPECTED.md``.

Tooling, not a pytest suite: Django/Celery are imported lazily inside
:func:`dump_django_jobs` only, so importing this module never touches
them.
"""

from __future__ import annotations

import argparse
import ast
import json
import os
import subprocess
import sys
from datetime import timedelta

# The four beat entries whose cadence comes from Django settings
# (celery.py _register_settings_backed_beat_entries), with the setting
# each reads. Recorded in the dump meta so the Rust env-overridable
# defaults can be compared against the Django values.
SETTINGS_BACKED_INTERVALS = (
    ("cloud-agent-scan-queued-runs", "CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS"),
    ("cloud-agent-sweep-stale-runs", "CLOUD_AGENT_SWEEP_INTERVAL_SECONDS"),
    ("managed-runner-expire-waiting-runs", "MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS"),
    ("agent-run-reconcile-terminal-effects", "AGENT_RUN_TERMINAL_RECONCILE_INTERVAL_SECONDS"),
)


def dump_django_jobs(api_dir: str, settings_module: str) -> dict:
    """Dump Celery task names + beat schedule as JSON-serialisable data.

    ``{"meta": {...}, "tasks": [{"name", "builtin", "registered",
    "defined", "called", "scheduled"}], "beat": [{"name", "task",
    "cadence", "settings_backed"}]}``. ``cadence`` is a normalised
    string (``crontab(minute=.. hour=.. ...)`` with all five fields, or
    ``interval(<seconds>s)``). ``called`` is set when the task function
    is invoked via a Celery API (static scan); ``scheduled`` when beat
    fires it.
    """
    sys.path.insert(0, os.path.abspath(api_dir))
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", settings_module)
    import django  # noqa: E402  -- lazy: importing this module stays clean

    django.setup()
    import importlib  # noqa: E402
    from django.conf import settings  # noqa: E402
    from pi_dash.celery import app  # noqa: E402

    # Worker boot imports every CELERY_IMPORTS module so their
    # @shared_task decorators register; do the same here.
    for module in settings.CELERY_IMPORTS:
        importlib.import_module(module)
    app.finalize()  # fires on_after_finalize: the settings-backed entries
    registered = set(app.tasks.keys())
    package_dir = os.path.join(os.path.abspath(api_dir), "pi_dash")
    defined = scan_task_definitions(package_dir)
    called_funcs = scan_task_calls(
        package_dir, {spec["func"] for spec in defined.values()}
    )
    scheduled_tasks = {
        entry["task"] for entry in app.conf.beat_schedule.values()
    }
    tasks = []
    for name in sorted(registered | set(defined.keys())):
        spec = defined.get(name)
        tasks.append(
            {
                "name": name,
                "builtin": name.startswith("celery."),
                "registered": name in registered,
                "defined": spec,
                "called": spec is not None and spec["func"] in called_funcs,
                "scheduled": name in scheduled_tasks,
            }
        )
    beat = []
    for name in sorted(app.conf.beat_schedule.keys()):
        entry = app.conf.beat_schedule[name]
        beat.append(
            {
                "name": name,
                "task": entry["task"],
                "cadence": _format_schedule(entry["schedule"]),
                "settings_backed": name
                in {row[0] for row in SETTINGS_BACKED_INTERVALS},
            }
        )
    meta = {
        "settings": settings_module,
        "django": django.get_version(),
        "task_count": len(tasks),
        "beat_count": len(beat),
        "intervals": {
            setting: getattr(settings, setting, None)
            for _, setting in SETTINGS_BACKED_INTERVALS
        },
    }
    return {"meta": meta, "tasks": tasks, "beat": beat}


def scan_task_definitions(package_dir: str) -> dict[str, dict]:
    """Statically enumerate every Celery task defined under package_dir.

    Walks ``*.py`` (excluding tests), parses with :mod:`ast`, and records
    each ``@shared_task`` / ``@app.task`` / ``@celery_app.task``
    function: ``name -> {"file", "line"}``. The name is the explicit
    ``name=`` kwarg when present, else ``{module}.{function}`` (Celery's
    default). Pure static scan — no Django import.
    """
    found: dict[str, dict] = {}
    for root, dirs, files in os.walk(package_dir):
        dirs[:] = [d for d in dirs if d != "tests" and not d.startswith(".")]
        for filename in sorted(files):
            if not filename.endswith(".py"):
                continue
            filepath = os.path.join(root, filename)
            rel = os.path.relpath(filepath, os.path.dirname(package_dir))
            module = rel[:-3].replace(os.sep, ".")
            try:
                with open(filepath, encoding="utf-8") as handle:
                    tree = ast.parse(handle.read(), filename=filepath)
            except (SyntaxError, UnicodeDecodeError):
                continue
            for node in ast.walk(tree):
                if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    continue
                for decorator in node.decorator_list:
                    name = _task_name_from_decorator(decorator, module, node.name)
                    if name is not None:
                        found[name] = {"file": rel, "line": node.lineno, "func": node.name}
                        break
    return found


def _task_name_from_decorator(decorator: ast.expr, module: str, func: str) -> str | None:
    """Return the Celery task name if this decorator defines one."""
    target: ast.expr = decorator
    keywords: list[ast.keyword] = []
    if isinstance(decorator, ast.Call):
        target = decorator.func
        keywords = decorator.keywords
    is_task = False
    if isinstance(target, ast.Name) and target.id in {"shared_task", "task"}:
        is_task = True
    elif isinstance(target, ast.Attribute) and target.attr == "task":
        is_task = True
    if not is_task:
        return None
    for keyword in keywords:
        if keyword.arg == "name" and isinstance(keyword.value, ast.Constant):
            return str(keyword.value.value)
    return f"{module}.{func}"


def scan_task_calls(package_dir: str, func_names: set[str]) -> set[str]:
    """Find task functions invoked via Celery APIs (static AST scan).

    Returns the subset of ``func_names`` referenced as ``func.delay(``,
    ``func.apply_async(``, ``func.si(`` or ``func.s(`` anywhere under
    ``package_dir`` (excluding tests). Pure static scan, no Django
    import. Direct (eager, in-process) calls are not counted: they need
    no worker on either side.
    """
    called: set[str] = set()
    for root, dirs, files in os.walk(package_dir):
        dirs[:] = [d for d in dirs if d != "tests" and not d.startswith(".")]
        for filename in files:
            if not filename.endswith(".py"):
                continue
            filepath = os.path.join(root, filename)
            try:
                with open(filepath, encoding="utf-8") as handle:
                    tree = ast.parse(handle.read(), filename=filepath)
            except (SyntaxError, UnicodeDecodeError):
                continue
            for node in ast.walk(tree):
                if not isinstance(node, ast.Call):
                    continue
                func = node.func
                if (
                    isinstance(func, ast.Attribute)
                    and func.attr in {"delay", "apply_async", "si", "s"}
                    and isinstance(func.value, ast.Name)
                    and func.value.id in func_names
                ):
                    called.add(func.value.id)
    return called


def _format_schedule(schedule) -> str:
    from celery.schedules import crontab  # noqa: E402  -- lazy, see module doc

    if isinstance(schedule, timedelta):
        return f"interval({int(schedule.total_seconds())}s)"
    if isinstance(schedule, crontab):
        fields = (
            ("minute", schedule._orig_minute),
            ("hour", schedule._orig_hour),
            ("day_of_month", schedule._orig_day_of_month),
            ("month_of_year", schedule._orig_month_of_year),
            ("day_of_week", schedule._orig_day_of_week),
        )
        inner = " ".join(f"{key}={value}" for key, value in fields)
        return f"crontab({inner})"
    return f"unknown({schedule!r})"


def load_rust_jobs(binary: str) -> dict:
    """Run ``pidash-api jobs --json`` and return its parsed inventory."""
    proc = subprocess.run(
        [binary, "jobs", "--json"],
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"{binary} jobs --json failed ({proc.returncode}): {proc.stderr.strip()}"
        )
    return json.loads(proc.stdout)


# Task-name prefix -> wire gap for live registered tasks the Rust worker
# does not own (each gap is one fix issue wiring the register fn).
WIRE_GAPS = (
    ("assistant.", "W-assistant"),
    ("cloud_agent.", "W-cloud-agent"),
    ("pi_dash.bgtasks.agent_ticker.", "W-ticker"),
    ("pi_dash.bgtasks.email_notification_task.", "W-mail"),
    ("pi_dash.bgtasks.notification_task.", "W-mail"),
    ("pi_dash.bgtasks.exporter_expired_task.", "W-export"),
    ("pi_dash.bgtasks.issue_activities_task.", "W-activity"),
    ("pi_dash.bgtasks.issue_automation_task.", "W-automation"),
    ("pi_dash.bgtasks.loop.", "W-loop"),
    ("pi_dash.bgtasks.scheduler.", "W-scheduler"),
    ("pi_dash.license.", "W-license"),
    ("runner.", "W-runner"),
)

# Registered but never published (zero references beyond the
# definition and its own log line, verified by hand): forwarding
# preserves their non-execution, so there is nothing to wire.
DORMANT_TASKS = {
    "runner.sweep_run_message_dedupe",
    "runner.sweep_stale_runners",
    "runner.sweep_idle_sessions",
    "runner.sweep_old_streams",
}

# Defined but never referenced anywhere (dead tasks).
DEAD_TASKS = {
    "pi_dash.bgtasks.analytic_plot_export.export_analytics_to_csv_email",
}

# Beat cadence the Rust entry must equal, rendered deterministically.
def _rust_cadence_string(entry: dict) -> str:
    cadence = entry["cadence"]
    if cadence["type"] == "interval_secs":
        return f"interval:{cadence['secs']}s"
    parts = []
    for key in ("minute", "hour", "day_of_month", "month_of_year", "day_of_week"):
        parts.append(f"{key}{{{','.join(str(v) for v in cadence[key])}}}")
    return "cron:" + " ".join(parts)


def diff_jobs(django: dict, rust: dict) -> dict:
    """Diff the Django jobs dump against the Rust inventory.

    Returns ``{"tasks": [...], "beat": [...]}``. Task rows:
    ``{"task", "registered", "owner", "disposition", "detail", "gap"}``
    with disposition ``OWNED`` | ``PROXIED`` | ``MISSING``. Beat rows:
    ``{"entry", "task", "django_cadence", "rust_cadence", "task_owner",
    "disposition", "detail"}``. ``G0``/``W0`` gaps are unclassified and
    must be resolved by hand before finalizing EXPECTED.md.
    """
    owner = {row["name"]: row["owner"] for row in rust["tasks"]}
    rust_schedule = {entry["name"]: entry for entry in rust["schedule"]}
    task_rows = []
    for task in django["tasks"]:
        name = task["name"]
        own = owner.get(name)
        if own is None:
            task_rows.append(
                {
                    "task": name,
                    "registered": task["registered"],
                    "owner": "-",
                    "disposition": "MISSING",
                    "detail": "absent-from-rust-table",
                    "gap": "W0",
                }
            )
            continue
        task_rows.append(_diff_task(name, task, own))
    beat_rows = []
    for entry in django["beat"]:
        rust_entry = rust_schedule.get(entry["name"])
        if rust_entry is None:
            beat_rows.append(
                {
                    "entry": entry["name"],
                    "task": entry["task"],
                    "django_cadence": entry["cadence"],
                    "rust_cadence": "-",
                    "task_owner": owner.get(entry["task"], "-"),
                    "disposition": "MISSING",
                    "detail": "no-rust-entry",
                }
            )
            continue
        problems = []
        if rust_entry["task"] != entry["task"]:
            problems.append(f"task:{rust_entry['task']}")
        rust_cadence = _rust_cadence_string(rust_entry)
        if not _cadences_equal(entry["cadence"], rust_entry["cadence"]):
            problems.append(f"cadence:{rust_cadence}")
        beat_rows.append(
            {
                "entry": entry["name"],
                "task": entry["task"],
                "django_cadence": entry["cadence"],
                "rust_cadence": rust_cadence,
                "task_owner": owner.get(entry["task"], "-"),
                "disposition": "MISSING" if problems else "OWNED",
                "detail": ";".join(problems) if problems else "ok",
            }
        )
    for name in sorted(set(rust_schedule) - {e["name"] for e in django["beat"]}):
        entry = rust_schedule[name]
        beat_rows.append(
            {
                "entry": name,
                "task": entry["task"],
                "django_cadence": "-",
                "rust_cadence": _rust_cadence_string(entry),
                "task_owner": owner.get(entry["task"], "-"),
                "disposition": "MISSING",
                "detail": "rust-only-entry",
            }
        )
    beat_rows.sort(key=lambda row: row["entry"])
    return {"tasks": task_rows, "beat": beat_rows}


def _diff_task(name: str, task: dict, own: str) -> dict:
    base = {"task": name, "registered": task["registered"], "owner": own}
    if own == "rust":
        if task["registered"]:
            return base | {"disposition": "OWNED", "detail": "owned-registered", "gap": "-"}
        return base | {
            "disposition": "OWNED",
            "detail": "owned-unregistered (superset; django-side PDASHOSS01-292)",
            "gap": "-",
        }
    if task["builtin"]:
        return base | {
            "disposition": "PROXIED",
            "detail": "celery-builtin; no canvas usage",
            "gap": "-",
        }
    if task["registered"]:
        if name in DORMANT_TASKS:
            return base | {
                "disposition": "PROXIED",
                "detail": "dormant (never published; forward preserves non-execution)",
                "gap": "-",
            }
        if not task["called"] and not task["scheduled"]:
            return base | {
                "disposition": "MISSING",
                "detail": "live-registered but no publishers found",
                "gap": "W0",
            }
        for prefix, gap in WIRE_GAPS:
            if name.startswith(prefix):
                return base | {
                    "disposition": "MISSING",
                    "detail": "must-wire (lost without python worker)",
                    "gap": gap,
                }
        return base | {
            "disposition": "MISSING",
            "detail": "must-wire (no gap rule)",
            "gap": "W0",
        }
    if name in DEAD_TASKS:
        return base | {
            "disposition": "PROXIED",
            "detail": "dead (defined, never referenced)",
            "gap": "-",
        }
    if name == "pi_dash.bgtasks.recent_visited_task.recent_visited_task":
        return base | {
            "disposition": "PROXIED",
            "detail": "documented no-op (memory broker, no worker by design)",
            "gap": "-",
        }
    if name == "managed_runner.expire_waiting_runs":
        return base | {
            "disposition": "PROXIED",
            "detail": "cloud-only app; oss beat fires into the void on both sides",
            "gap": "-",
        }
    if name == "pi_dash.bgtasks.event_tracking_task.track_event":
        return base | {
            "disposition": "PROXIED",
            "detail": "django-drops-too (PDASHOSS01-292); rust explicitly unregistered (early-return would swallow forward)",
            "gap": "-",
        }
    if name == "pi_dash.bgtasks.project_invitation_task.project_invitation":
        return base | {
            "disposition": "PROXIED",
            "detail": "task never invoked (invite.py:105 calls .delay on a list; PDASHOSS01-292)",
            "gap": "-",
        }
    if task["called"] or task["scheduled"]:
        return base | {
            "disposition": "PROXIED",
            "detail": "django-drops-too (PDASHOSS01-292)",
            "gap": "-",
        }
    return base | {
        "disposition": "PROXIED",
        "detail": "unregistered-uncalled (review)",
        "gap": "W0",
    }


def _cadences_equal(django_cadence: str, rust_cadence: dict) -> bool:
    """Compare a Django cadence string against Rust match sets."""
    if django_cadence.startswith("interval("):
        if rust_cadence["type"] != "interval_secs":
            return False
        seconds = int(django_cadence[len("interval("):-len("s)")])
        return rust_cadence["secs"] == seconds
    if not django_cadence.startswith("crontab(") or rust_cadence["type"] != "crontab":
        return False
    fields = dict(
        part.split("=", 1) for part in django_cadence[len("crontab("):-1].split(" ")
    )
    bounds = {
        "minute": (0, 59),
        "hour": (0, 23),
        "day_of_month": (1, 31),
        "month_of_year": (1, 12),
        "day_of_week": (0, 6),
    }
    for key, (low, high) in bounds.items():
        if set(rust_cadence[key]) != _expand_cron_field(fields[key], low, high):
            return False
    return True


def _expand_cron_field(spec: str, low: int, high: int) -> set[int]:
    """Expand one crontab field (``*``, ``*/n``, ranges, lists, names)."""
    names = {
        "mon": 1, "tue": 2, "wed": 3, "thu": 4, "fri": 5, "sat": 6, "sun": 7,
        "jan": 1, "feb": 2, "mar": 3, "apr": 4, "may": 5, "jun": 6,
        "jul": 7, "aug": 8, "sep": 9, "oct": 10, "nov": 11, "dec": 12,
    }
    values: set[int] = set()
    for part in spec.split(","):
        part = part.strip().lower()
        step = 1
        if "/" in part:
            part, step_text = part.split("/", 1)
            step = int(step_text)
        if part == "*":
            values.update(range(low, high + 1, step))
        elif "-" in part:
            start_text, end_text = part.split("-", 1)
            start = names.get(start_text, int(start_text) if start_text.lstrip("-").isdigit() else None)
            end = names.get(end_text, int(end_text) if end_text.isdigit() else None)
            if start is None or end is None:
                raise ValueError(f"bad cron range {spec!r}")
            values.update(range(start, end + 1, step))
        else:
            value = names.get(part, int(part) if part.isdigit() else None)
            if value is None:
                raise ValueError(f"bad cron value {spec!r}")
            values.add(value)
    if low == 0 and high == 6 and 7 in values:
        # Sunday is both 0 and 7 in cron; normalize to 0.
        values.discard(7)
        values.add(0)
    return {v for v in values if low <= v <= high}


def load_expected(path: str) -> set[str]:
    """Collect every ``expected-jobs`` fenced block in EXPECTED.md."""
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    expected: set[str] = set()
    in_block = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped == "```expected-jobs":
            in_block = True
            continue
        if in_block and stripped == "```":
            in_block = False
            continue
        if in_block and stripped:
            expected.add(stripped)
    return expected


def dump_django_fresh(api_dir: str, settings_module: str) -> dict:
    """Dump Celery tasks + beat in a fresh interpreter.

    Task ``registered`` state is import-order-dependent (importing the
    URL conf registers task modules as a side effect), so the check
    never dumps in-process: a fresh ``dump-django`` subprocess gives the
    same inventory CI sees.
    """
    proc = subprocess.run(
        [
            sys.executable,
            os.path.abspath(__file__),
            "dump-django",
            "--api-dir",
            api_dir,
            "--settings",
            settings_module,
            "--out",
            "-",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"dump-django failed ({proc.returncode}): {proc.stderr.strip()}")
    return json.loads(proc.stdout)


def _task_signature(row: dict) -> str:
    head = row["gap"] if row["disposition"] == "MISSING" else row["disposition"]
    return f"{head} :: {row['task']} :: {row['detail']}"


def cmd_check(args: argparse.Namespace) -> int:
    """Diff live Django vs live Rust and fail on anything unlisted."""
    django = dump_django_fresh(args.api_dir, args.settings)
    rust = load_rust_jobs(args.binary)
    diff = diff_jobs(django, rust)
    expected = load_expected(args.expected)
    for line in sorted(expected):
        if line.split(" :: ", 1)[0] == "W0":
            print(f"error: {args.expected} lists an unclassified row: {line}")
            return 1
    live = {_task_signature(row) for row in diff["tasks"] if row["disposition"] != "OWNED"}
    failures = 0
    for row in diff["tasks"]:
        if row["disposition"] == "MISSING" and row["gap"] == "W0":
            print(f"unclassified (W0): {row['task']} :: {row['detail']}")
            failures += 1
    for line in sorted(live - expected):
        print(f"unlisted diff: {line}")
        failures += 1
    for line in sorted(expected - live):
        print(f"stale EXPECTED.md entry (fix landed? update the file): {line}")
        failures += 1
    for row in diff["beat"]:
        if row["disposition"] != "OWNED":
            print(f"beat drift: {row['entry']} :: {row['detail']}")
            failures += 1
    owned = sum(1 for row in diff["tasks"] if row["disposition"] == "OWNED")
    proxied = sum(1 for row in diff["tasks"] if row["disposition"] == "PROXIED")
    beat_owned = sum(1 for row in diff["beat"] if row["disposition"] == "OWNED")
    verdict = "all listed" if failures == 0 else f"{failures} problem(s)"
    print(
        f"jobs: {len(diff['tasks'])} tasks ({owned} owned, {proxied} proxied-by-design, "
        f"{len(diff['tasks']) - owned - proxied} missing), "
        f"{beat_owned}/{len(diff['beat'])} beat owned -- {verdict}"
    )
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Django/Rust job parity inventory")
    sub = parser.add_subparsers(dest="command", required=True)

    dump = sub.add_parser("dump-django", help="dump Celery tasks + beat as JSON")
    dump.add_argument("--api-dir", default="apps/api")
    dump.add_argument("--settings", default="pi_dash.settings.test")
    dump.add_argument("--out", default="-")

    check = sub.add_parser("check", help="fail on any diff not listed in EXPECTED.md")
    check.add_argument("--binary", default="pidash-api")
    check.add_argument("--api-dir", default="apps/api")
    check.add_argument("--settings", default="pi_dash.settings.test")
    check.add_argument(
        "--expected",
        default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "EXPECTED.md"),
    )

    args = parser.parse_args(argv)
    if args.command == "dump-django":
        inventory = dump_django_jobs(args.api_dir, args.settings)
        text = json.dumps(inventory, indent=2, sort_keys=False) + "\n"
        if args.out == "-":
            sys.stdout.write(text)
        else:
            with open(args.out, "w", encoding="utf-8") as handle:
                handle.write(text)
        return 0
    if args.command == "check":
        return cmd_check(args)
    raise AssertionError(f"unreachable command {args.command}")  # pragma: no cover


if __name__ == "__main__":
    raise SystemExit(main())
