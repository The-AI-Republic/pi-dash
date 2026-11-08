# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""CI must build the desktop shell against the web_new bundle (PIDESKAPP-205).

web-new.yml builds the web_new SPA with Vite and check:no-tauri, but never
runs desktop/scripts/dev-prep.sh with PIDASH_DESKTOP_WEB=web_new and never
compiles desktop/src-tauri in release against that dist/ — so a break in the
desktop bundle or the shell release guards merges green (PIDESKAPP-184/185
are the breaks this gap allowed). pull-request-test-desktop.yml ran cargo
clippy/test in debug against the committed placeholder dist/ and did not
fire on frontend changes at all. The desktop parity specs
(apps/web_new/e2e/parity/desktop/*, runners/desktop-runtime.spec.ts) were
not run by any workflow either.

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

    def test_a_job_runs_the_desktop_parity_specs(self):
        # The desktop rows assert the bundled desktop behavior (tauri-mock
        # driver) against a seeded stack. The job must bring the stack up
        # and run every desktop spec file through run-parity.mjs on the
        # `new` project, in two serve phases: RUN rows assert web-observable
        # behavior and need the web build (on the desktop build the app
        # renders the desktop-unavailable card instead of the sign-in form,
        # so their entry flow never starts), DESK rows need the desktop
        # build.
        self.assertIn("parity-up.sh", self.text)
        # parity-env.sh omits the container names, but the server helpers
        # shell into the API container by name: without this derivation the
        # specs exec into the default parity19-api and fail.
        self.assertIn("PARITY_API_CONTAINER", self.text)
        self.assertIn("PARITY_NEW_DIST=web", self.text)
        self.assertIn("PARITY_NEW_DIST=desktop", self.text)
        self.assertIn("pnpm build:web", self.text)
        self.assertIn("parity:serve-new", self.text)
        # Via the pnpm script: run-parity.mjs spawns the bare `playwright`
        # binary, which is only on PATH in a pnpm script's environment.
        self.assertIn("test:parity:new --", self.text)
        for spec in (
            "e2e/parity/desktop/shell/shell.spec.ts",
            "e2e/parity/desktop/sign-in/sign-in.spec.ts",
            "e2e/parity/desktop/updater/updater.spec.ts",
            "e2e/parity/desktop/agent-runtime/agent-runtime.spec.ts",
            "e2e/parity/runners/desktop-runtime.spec.ts",
        ):
            with self.subTest(spec=spec):
                self.assertIn(spec, self.text)
        # Browsers are required for Playwright to start at all.
        self.assertIn("playwright install", self.text)
        # The scratch stack must be torn down even when specs fail.
        self.assertIn("parity-down.sh", self.text)

    def test_desktop_build_bakes_the_stack_origin_not_a_placeholder(self):
        # DESK rows assert shareable links and opener hand-offs resolve
        # against the seeded stack's oracle URL, so dev-prep must bake
        # that URL (derived from PARITY_NS) rather than a placeholder.
        self.assertIn("parity-env.sh", self.text)
        self.assertIn("VITE_WEB_BASE_URL=\"$PARITY_ORACLE_URL\"", self.text)

    def test_release_build_derives_the_origin_like_dev_prep(self):
        # The release compile must use the same namespaced oracle URL
        # dev-prep baked in; a hardcoded port would drift from PARITY_NS.
        self.assertNotIn("localhost:13005", self.text)
        self.assertIn("PARITY_ORACLE_URL", self.text)

    def test_failure_artifacts_are_uploaded(self):
        # A red DESK leg must leave diagnosable evidence: Playwright
        # error-context/traces and the serve logs upload on failure.
        self.assertIn("upload-artifact", self.text)
        self.assertIn("if: failure()", self.text)
        self.assertIn("test-results/", self.text)
        self.assertIn("parity-serve-desktop.log", self.text)

    def test_stale_parity_legs_are_excluded_explicitly(self):
        # DESK-002/DESK-025/RUN-044 are stale against current web_new
        # behavior (NEWFRONT-1056). They are excluded by code, not silently
        # skipped: the exclusion names the legs.
        self.assertIn("--grep-invert", self.text)
        self.assertIn("DESK-002", self.text)
        self.assertIn("DESK-025", self.text)
        self.assertIn("RUN-044", self.text)
        self.assertIn("NEWFRONT-1056", self.text)

    def test_desk009_exclusion_is_explicit(self):
        # DESK-009 loses the on-machine turn across reload after a
        # clear+auto-create in CI (deterministic in runs 42472453472
        # and 42482026426; passes locally). It stays excluded by name
        # until NEWFRONT-1057 lands the spec/app fix.
        self.assertIn("DESK-009", self.text)
        self.assertIn("NEWFRONT-1057", self.text)


if __name__ == "__main__":
    unittest.main()
