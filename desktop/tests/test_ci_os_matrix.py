# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The desktop crate's PR check must build, lint and test every shipped OS.

`desktop/src-tauri` carries `#[cfg(windows)]` / `#[cfg(target_os = "macos")]`
arms (daemon stop, creation flags, installer command, install paths) that are
not part of the compilation unit on Linux. A job pinned to one Ubuntu runner
lets a Windows-only or macOS-only break merge green; it then surfaces in the
release build, which compiles those arms but never runs their tests.

Read as text (no YAML dependency: the CI image's Python has no PyYAML
guaranteed).
"""

from pathlib import Path
import re
import unittest

REPO = Path(__file__).resolve().parents[2]
WORKFLOW = REPO / ".github/workflows/pull-request-test-desktop.yml"

# One runner per OS family the desktop app ships installers for.
SHIPPED = ("ubuntu", "windows", "macos")

MATRIX_OS = re.compile(r"^ +matrix:\n +os:\n((?: +(?:- \S+|#.*)\n)+)", re.M)
STEP = re.compile(r"^      - (?:name|uses): .*\n(?:(?!      - ).*\n?)*", re.M)


def matrix_runners(text: str) -> list:
    block = MATRIX_OS.search(text)
    return re.findall(r"^ +- (\S+)$", block.group(1), re.M) if block else []


def cargo_steps(text: str) -> dict:
    """`cargo <subcommand> ...` run line -> the step that owns it."""
    steps = {}
    for step in STEP.findall(text):
        for command in re.findall(r"^ +run: (cargo .+)$", step, re.M):
            steps[command] = step
    return steps


class DesktopWorkflowOsMatrixTests(unittest.TestCase):
    def setUp(self):
        self.text = WORKFLOW.read_text()
        self.cargo = cargo_steps(self.text)

    def test_parser_sees_the_cargo_steps(self):
        # An empty result would make the per-step assertions pass vacuously.
        self.assertTrue(any(c.startswith("cargo test") for c in self.cargo))

    def test_job_runs_on_every_shipped_os(self):
        runners = matrix_runners(self.text)
        for family in SHIPPED:
            self.assertTrue(
                any(r.startswith(family + "-") for r in runners),
                f"no {family} runner in the job matrix: {runners}",
            )
        self.assertRegex(self.text, r"(?m)^    runs-on: \$\{\{ matrix\.os \}\}$")

    def test_one_failing_os_does_not_cancel_the_others(self):
        self.assertRegex(self.text, r"(?m)^      fail-fast: false$")

    def test_clippy_and_tests_cover_all_targets_with_warnings_denied(self):
        self.assertIn(
            "cargo clippy --all-targets --locked -- -D warnings", self.cargo
        )
        self.assertIn("cargo test --all-targets --locked", self.cargo)

    def test_cargo_steps_are_not_pinned_to_one_os(self):
        for command, step in self.cargo.items():
            self.assertNotRegex(step, r"(?m)^        if: ", command)


if __name__ == "__main__":
    unittest.main()
