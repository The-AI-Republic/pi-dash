# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The Rust CI workflows must fire for the crates they build from source.

`pull-request-test-runner.yml` and `pull-request-test-desktop.yml` are gated
by `paths:` filters. Both crates take `pidash-ipc` — the desktop <-> daemon
wire protocol — as a path dependency, and editing a path dependency's source
changes neither lockfile, so a filter that names only the crate's own
directory lets a protocol change merge with no Rust CI at all.

These read the workflows and manifests as text (no YAML/TOML dependency: the
CI image's Python has neither PyYAML nor tomllib guaranteed).
"""

from pathlib import Path
import re
import unittest

REPO = Path(__file__).resolve().parents[2]
WORKFLOWS = REPO / ".github/workflows"

# workflow -> the crate directory its job builds and tests
CRATES = {
    "pull-request-test-runner.yml": "runner",
    "pull-request-test-desktop.yml": "desktop/src-tauri",
}

PATH_DEP = re.compile(r'^[\w-]+\s*=\s*\{[^}\n]*\bpath\s*=\s*"([^"]+)"', re.M)
# A `paths:` key and its list items, comment lines included.
PATHS_BLOCK = re.compile(r'^ +paths:\n((?: +(?:- "[^"\n]+"|#.*)\n)+)', re.M)


def path_dependencies(crate: str, seen=None) -> set:
    """Repo-relative directories of `crate`'s path dependencies, transitively."""
    seen = set() if seen is None else seen
    manifest = (REPO / crate / "Cargo.toml").read_text()
    for rel in PATH_DEP.findall(manifest):
        dep = (REPO / crate / rel).resolve().relative_to(REPO).as_posix()
        if dep not in seen:
            seen.add(dep)
            path_dependencies(dep, seen)
    return seen


def paths_filters(workflow: str) -> list:
    """Every `paths:` list in the workflow (one per trigger), as lists of globs."""
    text = (WORKFLOWS / workflow).read_text()
    return [
        re.findall(r'^ +- "([^"]+)"$', block, re.M)
        for block in PATHS_BLOCK.findall(text)
    ]


def covers(globs: list, directory: str) -> bool:
    """True when some `<dir>/**` glob in the filter contains `directory`."""
    return any(
        glob.endswith("/**") and (directory + "/").startswith(glob[:-2])
        for glob in globs
    )


class RustWorkflowPathFilterTests(unittest.TestCase):
    def test_parsers_see_the_workflows(self):
        # Guard the text parsing itself: an empty result would make the
        # coverage test below pass vacuously.
        for workflow, crate in CRATES.items():
            filters = paths_filters(workflow)
            self.assertEqual(len(filters), 2, f"{workflow}: pull_request + push")
            for globs in filters:
                self.assertTrue(covers(globs, crate), f"{workflow}: {globs}")
        self.assertIn("pidash-ipc", path_dependencies("desktop/src-tauri"))
        self.assertIn("pidash-ipc", path_dependencies("runner"))

    def test_path_dependencies_trigger_the_workflow(self):
        for workflow, crate in CRATES.items():
            for globs in paths_filters(workflow):
                for dep in sorted(path_dependencies(crate)):
                    with self.subTest(workflow=workflow, dependency=dep):
                        self.assertTrue(
                            covers(globs, dep),
                            f"{workflow} builds {crate}, which depends on "
                            f"{dep}/ by path, but its paths filter {globs} "
                            f"does not include {dep}/**",
                        )

    def test_runner_workflow_tests_every_workspace_member(self):
        # The job runs from runner/, where a bare `cargo test` only compiles
        # pidash-ipc as a dependency and never runs its own #[cfg(test)]
        # suite.
        text = (WORKFLOWS / "pull-request-test-runner.yml").read_text()
        commands = re.findall(r"^ +run: (cargo (?:test|clippy) .*)$", text, re.M)
        self.assertEqual(len(commands), 2, commands)
        for command in commands:
            self.assertIn("--workspace", command.split())


if __name__ == "__main__":
    unittest.main()
