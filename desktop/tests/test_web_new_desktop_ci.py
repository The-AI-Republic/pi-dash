# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""CI must build the desktop shell against the web_new bundle (PIDESKAPP-205).

web-new.yml builds the web_new SPA with Vite and check:no-tauri, but never
runs desktop/scripts/dev-prep.sh with PIDASH_DESKTOP_WEB=web_new and never
compiles desktop/src-tauri in release against that dist/ — so a break in the
desktop bundle or the shell release guards merges green (PIDESKAPP-184/185
are the breaks this gap allowed). pull-request-test-desktop.yml runs cargo
clippy/test in debug against the committed placeholder dist/ and does not
fire on frontend changes at all.

This reads the workflows as text (no YAML dependency: the CI image's Python
has no PyYAML guaranteed), matching test_ci_path_filters.py.
"""

from pathlib import Path
import re
import unittest

REPO = Path(__file__).resolve().parents[2]
WORKFLOWS = REPO / ".github/workflows"

# A `paths:` key and its list items, comment lines included.
PATHS_BLOCK = re.compile(r"^ +paths:\n((?: +(?:- \"[^\"\n]+\"|#.*)\n)+)", re.M)


def paths_filters(text: str) -> list:
    return [
        re.findall(r'^ +- "([^"]+)"$', block, re.M)
        for block in PATHS_BLOCK.findall(text)
    ]


def covers(globs: list, pattern: str) -> bool:
    return pattern in globs


class WebNewDesktopCiTests(unittest.TestCase):
    def setUp(self):
        self.text = (WORKFLOWS / "pull-request-test-desktop.yml").read_text()
        self.filters = paths_filters(self.text)

    def test_parser_sees_the_workflow(self):
        # Guard the text parsing itself: an empty result would make the
        # coverage assertions below pass vacuously.
        self.assertEqual(len(self.filters), 2, "pull_request + push")

    def test_paths_cover_the_web_new_frontend(self):
        # A PR that only touches the frontend must still trigger the
        # desktop shell job: the shell bundles that frontend.
        for globs in self.filters:
            for pattern in ("apps/web_new/**", "packages/kit/**", "packages/api-client/**"):
                with self.subTest(globs=globs, pattern=pattern):
                    self.assertTrue(
                        covers(globs, pattern),
                        f"pull-request-test-desktop.yml paths {globs} "
                        f"does not include {pattern}",
                    )

    def test_a_job_builds_the_desktop_bundle_with_web_new(self):
        # The flagged dev-prep build is what a developer runs locally; CI
        # must run the same command on a clean checkout.
        self.assertIn("PIDASH_DESKTOP_WEB=web_new", self.text)
        self.assertIn("dev-prep.sh", self.text)

    def test_a_job_compiles_the_shell_in_release(self):
        # Debug clippy/test never run build.rs's release guards, so they
        # cannot catch a release-only panic against the web_new dist/.
        self.assertRegex(self.text, r"cargo build --release|cargo tauri build")


if __name__ == "__main__":
    unittest.main()
