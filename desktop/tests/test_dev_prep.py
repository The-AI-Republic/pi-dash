# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Which origins `dev-prep.sh` bakes into the bundled desktop SPA.

`dev-prep.sh` is what `cargo tauri dev` and a local `cargo tauri build` run
before compiling, and the `VITE_*` values it exports to the web build are the
only place the bundle learns its API and public web origins. The web origin is
what shareable links and the desktop browser hand-off resolve against, and the
runtime refuses to invent one — so a default build that bakes nothing ships an
app whose "Copy link" cannot work and whose screens cannot leave the window.

These run the real script against a synthetic checkout (it derives its own
paths from its location) with the expensive collaborators stubbed out: the
runner/engine prep and `pnpm`, whose stand-in records the environment it was
given and emits a bundle that embeds those origins the way the real build does.
"""

from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/dev-prep.sh"

FAKE_PNPM = """#!/usr/bin/env bash
set -euo pipefail
echo "pnpm $*" >> "$DEV_PREP_TEST_LOG/calls"
if [[ "${1:-}" == "install" ]]; then
    exit 0
fi
if [[ "$*" == *"--filter @pidash/kit build"* ]]; then
    printf '%s' "${VITE_WEB_BASE_URL-<absent>}" > "$DEV_PREP_TEST_LOG/kit-web-base"
    exit 0
fi
if [[ "$*" == *"--filter @pidash/api-client build"* ]]; then
    exit 0
fi
if [[ "$*" == *"--filter web_new build:desktop"* ]]; then
    printf '%s' "${VITE_WEB_BASE_URL-<absent>}" > "$DEV_PREP_TEST_LOG/web-base"
    printf '%s' "${VITE_API_BASE_URL-<absent>}" > "$DEV_PREP_TEST_LOG/api-base"
    printf '%s' "${PIDASH_TARGET-<absent>}" > "$DEV_PREP_TEST_LOG/target"
    client="apps/web_new/dist/desktop"
    mkdir -p "$client/assets"
    echo '<!doctype html><html><body><div id=root></div><script src="assets/app.js"></script></body></html>' \
        > "$client/index.html"
    bake_web="${VITE_WEB_BASE_URL-}"
    [[ -n "${DEV_PREP_TEST_DROP_WEB_BASE:-}" ]] && bake_web=""
    printf 'const api="%s";const web="%s";const t="desktop_api_request";\\n' \\
        "${VITE_API_BASE_URL-}" "$bake_web" > "$client/assets/app.js"
    exit 0
fi
echo "fake pnpm: unexpected arguments: $*" >&2
exit 99
"""


def write(path: Path, text: str, mode: int = 0o644) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(mode)


class DevPrepBakedOriginTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="dev-prep-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)

        # A synthetic OSS checkout: dev-prep.sh resolves DESKTOP_DIR / OSS_DIR
        # from where it lives, so this copy is rooted at self.tmp.
        scripts = self.tmp / "desktop/scripts"
        scripts.mkdir(parents=True)
        self.script = scripts / "dev-prep.sh"
        shutil.copy2(SCRIPT, self.script)
        write(self.tmp / "apps/web_new/package.json", '{"name":"web_new"}\n')

        write(scripts / "prepare-agent.sh", "#!/usr/bin/env bash\n")

        self.bin = self.tmp / "bin"
        write(self.bin / "pnpm", FAKE_PNPM, 0o755)
        # dev-prep.sh runs `corepack enable` when it finds one; keep the test
        # from touching the machine's real shims.
        write(self.bin / "corepack", "#!/usr/bin/env bash\n", 0o755)

        self.log = self.tmp / "log"
        self.log.mkdir()
        self.dist = self.tmp / "desktop/src-tauri/dist"

    def prep(self, check=True, **env):
        return subprocess.run(
            ["bash", str(self.script)],
            capture_output=True,
            text=True,
            env={
                "PATH": f"{self.bin}{os.pathsep}/usr/bin:/bin:/usr/local/bin",
                "DEV_PREP_TEST_LOG": str(self.log),
                **env,
            },
            check=check,
        )

    def baked_web_base(self):
        return (self.log / "web-base").read_text()

    def test_default_build_bakes_the_bundled_dev_sign_in_origin(self):
        # No VITE_WEB_BASE_URL and no PI_DASH_URL: the README's default
        # `cargo tauri dev`. main.rs falls back to http://localhost:8000 for
        # PI_DASH_URL in that build, and the web origin has to follow it rather
        # than come out empty.
        self.prep()
        self.assertEqual(self.baked_web_base(), "http://localhost:8000")
        self.assertIn("web_base=http://localhost:8000", (self.dist / "bake-info.txt").read_text())

    def test_web_base_follows_pi_dash_url(self):
        self.prep(PI_DASH_URL="https://pidash.example.com")
        self.assertEqual(self.baked_web_base(), "https://pidash.example.com")

    def test_explicit_web_base_wins_over_pi_dash_url(self):
        self.prep(PI_DASH_URL="https://api.example.com", VITE_WEB_BASE_URL="https://app.example.com")
        self.assertEqual(self.baked_web_base(), "https://app.example.com")

    def test_an_empty_web_base_export_does_not_blank_the_default(self):
        # An exported-but-empty variable (a CI matrix cell, a sourced .env
        # template) is as unusable as an unset one.
        self.prep(VITE_WEB_BASE_URL="", PI_DASH_URL="")
        self.assertEqual(self.baked_web_base(), "http://localhost:8000")

    def test_a_bundle_missing_the_web_base_is_rejected(self):
        # Same guard the API base already has: a build that did not pick the
        # value up must not replace dist/.
        result = self.prep(
            check=False,
            VITE_WEB_BASE_URL="https://app.example.com",
            DEV_PREP_TEST_DROP_WEB_BASE="1",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("VITE_WEB_BASE_URL=https://app.example.com", result.stderr)
        self.assertFalse((self.dist / "bake-info.txt").exists())

    def test_the_web_new_desktop_target_is_what_gets_built(self):
        # Exactly one frontend, built for the desktop platform: the web build
        # would still render, with no native transport and no hand-off.
        self.prep(VITE_WEB_BASE_URL="https://app.example.com")
        self.assertEqual((self.log / "target").read_text(), "desktop")
        calls = (self.log / "calls").read_text()
        self.assertIn("--filter web_new build:desktop", calls)
        self.assertNotIn("turbo", calls)

    def test_the_kit_package_is_built_before_the_app(self):
        # web_new imports @pidash/kit's built stylesheet, which the source
        # alias cannot stand in for; a clean checkout fails its very first
        # desktop build without this step (PIDESKAPP-185).
        self.prep()
        calls = (self.log / "calls").read_text().splitlines()
        kit = next(i for i, line in enumerate(calls) if "--filter @pidash/kit build" in line)
        app = next(i for i, line in enumerate(calls) if "--filter web_new build:desktop" in line)
        self.assertLess(kit, app)
        install = next(i for i, line in enumerate(calls) if line.startswith("pnpm install"))
        self.assertLess(install, kit)
        # Both workspace deps (PIDESKAPP-185): the API client feeds the SPA too.
        api_client = next(i for i, line in enumerate(calls) if "--filter @pidash/api-client build" in line)
        self.assertLess(api_client, app)
        self.assertLess(install, api_client)

    def test_bake_info_records_the_frontend_and_both_origins(self):
        self.prep(VITE_API_BASE_URL="http://127.0.0.1:8131", VITE_WEB_BASE_URL="https://app.example.com")
        info = (self.dist / "bake-info.txt").read_text()
        self.assertIn("frontend=web_new", info)
        self.assertIn("api_base=http://127.0.0.1:8131", info)
        self.assertIn("web_base=https://app.example.com", info)


if __name__ == "__main__":
    unittest.main()
