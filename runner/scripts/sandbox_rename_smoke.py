# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression guard: the *renamed* engine still enforces its sandbox.

`stage_engine.py` ships the upstream Codex binary renamed to
`pidash-agent-engine` in the CLI release's `engine/<version>/` tree. Codex
re-execs helper copies of itself to build the sandbox that walls in the
commands an agent runs, dispatching on `argv[0]` / `current_exe()`. If a
renamed binary fails to self-locate, commands could run *without* the sandbox
and nothing would say so — a silent safety-feature failure, not a crash
(see design.md §24.8, PDASHOSS01-162).

This script copies the engine under the name it ships as, then runs the probe
from PDASHOSS01-162 against it: a no-sandbox control write must succeed, and the
same write attempted through `pidash-agent-engine sandbox` must be refused. A
future `ENGINE_VERSION` bump that breaks `arg0` dispatch makes this fail
the build instead of shipping an un-sandboxed engine.

No model, session, or credential is used. The engine binary must already be
staged (pass `--engine`), or `--stage` will stage the pinned upstream release
for the current host through `stage_engine.py` itself (needs `gh` + a token;
used in CI).

Two traps this method exists to avoid (from the ticket):
  1. CODEX_HOME must not live under the system temp dir — Codex refuses to
     create its helper binaries there and *skips the exact code path* under
     test. This script keeps its scratch tree out of `tempfile.gettempdir()`.
  2. Checking only "the probe file is absent" yields a false pass if the engine
     never ran the command (usage error, refused helpers). We assert the inner
     shell actually executed (a start marker in its output) AND that the write
     did not happen — and always keep a passing control.
"""

import argparse
import importlib.util
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent

# Reuse the exact staging the CLI release uses, so this guard and the shipped
# engine can never disagree about which asset the engine comes from.
_SPEC = importlib.util.spec_from_file_location(
    "stage_engine", HERE / "stage_engine.py"
)
stage_engine = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(stage_engine)

START = "PROBE_START"
WROTE = "WROTE"


def resolve_engine_tag():
    """The engine tag the CLI release ships, from `runner/ENGINE_VERSION`."""
    return stage_engine.read_engine_tag()


def host_target():
    """CLI release target string for the machine running this script."""
    machine = platform.machine().lower()
    if sys.platform == "darwin":
        return "aarch64-apple-darwin" if machine in ("arm64", "aarch64") else "x86_64-apple-darwin"
    if sys.platform.startswith("linux"):
        if machine in ("x86_64", "amd64"):
            return "x86_64-unknown-linux-gnu"
        if machine in ("aarch64", "arm64"):
            return "aarch64-unknown-linux-gnu"
        raise SystemExit(f"Unsupported Linux arch for this guard: {machine}")
    raise SystemExit(f"Unsupported platform for this guard: {sys.platform}")


def stage_binary(tag, into):
    """Stage the pinned upstream engine for this host via `stage_engine.py`."""
    target = host_target()
    pins = stage_engine.read_pins(tag)
    directory = stage_engine.stage(target, into, tag, pins)
    _, _, shipped = stage_engine.upstream_asset(target)
    return directory / shipped


def run_probe(engine_named, home, outside, label):
    """Run `sh -c 'echo START; touch <outside>/<label> ...'` under the engine sandbox."""
    probe = outside / f"probe_{label}"
    if probe.exists():
        probe.unlink()
    inner = f"echo {START}; touch '{probe}' && echo {WROTE} || echo BLOCKED"
    # No -c / -C flags (trap 2): they make --permission-profile required and the
    # engine would exit on a usage error without ever running `inner`.
    result = subprocess.run(
        [str(engine_named), "sandbox", "sh", "-c", inner],
        cwd=str(outside.parent / "work"),
        env={"CODEX_HOME": str(home), "PATH": "/usr/bin:/bin:/usr/sbin:/sbin"},
        capture_output=True,
        text=True,
    )
    return result, probe


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", type=Path, help="Path to an already-staged engine binary.")
    parser.add_argument(
        "--stage",
        action="store_true",
        help="Stage the pinned upstream engine for this host via stage_engine.py (needs gh + token).",
    )
    parser.add_argument(
        "--engine-tag",
        default=None,
        help="Upstream release tag to stage; defaults to runner/ENGINE_VERSION.",
    )
    args = parser.parse_args()
    engine_tag = args.engine_tag or resolve_engine_tag()

    # Scratch tree kept OUT of the system temp dir (trap 1). CODEX_HOME under
    # $TMPDIR makes the engine refuse its helper binaries and skip the sandbox.
    root = Path.home() / ".cache" / "pidash-sandbox-rename-smoke"
    if root.exists():
        shutil.rmtree(root)
    system_temp = Path(tempfile.gettempdir()).resolve()
    if system_temp in root.resolve().parents or root.resolve() == system_temp:
        raise SystemExit(f"Scratch dir {root} is under the system temp dir; would defeat the test.")
    work = root / "work"
    outside = root / "outside"
    home = root / "home"
    for directory in (work, outside, home):
        directory.mkdir(parents=True, exist_ok=True)

    if args.engine:
        source = args.engine
    elif args.stage:
        print(f"staging engine {engine_tag} for {host_target()} ...")
        source = stage_binary(engine_tag, root)
    else:
        raise SystemExit("Provide --engine <path> or --stage to stage the pinned engine.")
    if not Path(source).exists():
        raise SystemExit(f"Engine binary not found: {source}")

    # The crux: run under the name the engine ships as, so arg0/current_exe
    # self-location is exercised exactly as in the CLI release.
    suffix = ".exe" if sys.platform.startswith("win") else ""
    engine_named = root / f"pidash-agent-engine{suffix}"
    shutil.copy2(source, engine_named)
    engine_named.chmod(0o755)

    version = subprocess.run(
        [str(engine_named), "--version"], capture_output=True, text=True
    ).stdout.strip()

    failures = []

    # Control: the same write, with NO sandbox, MUST succeed — otherwise a
    # "block" proves nothing (it could be permissions, a full disk, etc.).
    control = outside / "control"
    if control.exists():
        control.unlink()
    control.write_text("ok")
    if not control.exists():
        raise SystemExit("Control write failed; the target is not writable — probe is invalid.")

    # Test: the write attempted through the sandbox MUST be refused.
    result, probe = run_probe(engine_named, home, outside, "fs")
    combined = result.stdout + result.stderr
    if START not in combined:
        failures.append(
            "engine never ran the probe command (usage error / refused helpers?) — "
            f"the sandbox code path was NOT exercised.\n--- output ---\n{combined}"
        )
    elif WROTE in combined or probe.exists():
        failures.append(
            "out-of-bounds write was NOT blocked: the renamed engine ran the command "
            f"WITHOUT the sandbox.\n--- output ---\n{combined}"
        )

    print(f"engine: {version}  (as {engine_named.name})")
    print(f"control (no sandbox): wrote {control} -> OK")
    print("sandbox probe output:")
    for line in combined.strip().splitlines():
        print(f"    {line}")

    if failures:
        print("\nSANDBOX RENAME SMOKE: FAIL", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("\nSANDBOX RENAME SMOKE: PASS — renamed engine blocked the out-of-bounds write.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
