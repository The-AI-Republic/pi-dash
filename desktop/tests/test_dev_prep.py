# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Which origins `dev-prep.sh` bakes into the bundled desktop SPA.

`dev-prep.sh` is what `cargo tauri dev` and a local `cargo tauri build` run
before compiling, and the `VITE_*` values it exports to the web build are the
only place the bundle learns its API and public web origins. The web origin is
what `desktop-overlay`'s `desktopWebUrl()` builds every shareable link from,
and it throws on an empty one — so a default build that bakes nothing ships an
app whose "Copy link" cannot work.

These run the real script against a synthetic checkout (it derives its own
paths from its location) with the expensive collaborators stubbed out: the
runner/engine prep, the tree merge, and `pnpm`, whose stand-in records the
environment it was given and emits a bundle that embeds those origins the way
the real build does.
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
[[ "$*" == "exec turbo run build"* ]] || exit 0
printf '%s' "${VITE_WEB_BASE_URL-<absent>}" > "$DEV_PREP_TEST_LOG/web-base"
printf '%s' "${VITE_API_BASE_URL-<absent>}" > "$DEV_PREP_TEST_LOG/api-base"
client="apps/web/build/client"
mkdir -p "$client/assets"
head -c 8192 /dev/zero | tr '\\0' ' ' > "$client/index.html"
bake_web="${VITE_WEB_BASE_URL-}"
[[ -n "${DEV_PREP_TEST_DROP_WEB_BASE:-}" ]] && bake_web=""
printf 'const api="%s";const web="%s";\\n' "${VITE_API_BASE_URL-}" "$bake_web" > "$client/assets/app.js"
"""


def write(path: Path, text: str, mode: int = 0o644) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(mode)


class DevPrepBakedOriginTests(unittest.TestCase):
    def setUp(self):
        if not shutil.which("rsync"):
            self.skipTest("rsync is required by dev-prep.sh")
        self.tmp = Path(tempfile.mkdtemp(prefix="dev-prep-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)

        # A synthetic OSS checkout: dev-prep.sh resolves DESKTOP_DIR / OSS_DIR
        # from where it lives, so this copy is rooted at self.tmp.
        scripts = self.tmp / "desktop/scripts"
        scripts.mkdir(parents=True)
        self.script = scripts / "dev-prep.sh"
        shutil.copy2(SCRIPT, self.script)
        write(self.tmp / "apps/web/package.json", '{"name":"web"}\n')

        write(scripts / "prepare-agent.sh", "#!/usr/bin/env bash\n")
        write(scripts / "merge-web-tree.sh", '#!/usr/bin/env bash\nmkdir -p "$1"\n')

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


if __name__ == "__main__":
    unittest.main()
