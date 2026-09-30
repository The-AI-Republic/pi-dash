# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Flagged desktop build of the new frontend (NEWFRONT-18, F-07).

`dev-prep.sh` gains one additive branch: with PIDASH_DESKTOP_WEB=web_new it
builds apps/web_new with PIDASH_TARGET=desktop and copies
apps/web_new/dist/desktop into desktop/src-tauri/dist/. Without the flag the
legacy overlay-merged apps/web path must run exactly as before.

These run the real script against a synthetic checkout: the script derives
its own `OSS_DIR` from its location, so copying it into a temp tree
exercises the shipped routing while stub neighbours (prepare-agent.sh,
merge-web-tree.sh, pnpm, corepack) stand in for the heavy steps. The stubs
record what the script asked for, and materialize a fake bundle the script's
own guards then validate.
"""

from pathlib import Path
import os
import shutil
import stat
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/dev-prep.sh"
API_BASE = "http://127.0.0.1:8131"

FAKE_PNPM = """\
#!/usr/bin/env bash
set -euo pipefail
echo "pnpm $*" >> "$FAKE_LOG"
if [[ "${1:-}" == "install" ]]; then
    exit 0
fi
if [[ "$*" == *"--filter web_new build:desktop"* ]]; then
    out="apps/web_new/dist/desktop"
    mkdir -p "$out/assets"
    if [[ "${FAKE_WEB_NEW_DESKTOP_MARKER:-1}" == "1" ]]; then
        echo "// desktop_api_request (desktop platform)" > "$out/assets/app.js"
    else
        echo "// plain web bundle, no desktop code" > "$out/assets/app.js"
    fi
    # Mirror the real web_new shell: a small index.html bootstrapping
    # assets/ (FAKE_WEB_NEW_SHELL=broken omits the reference entirely).
    if [[ "${FAKE_WEB_NEW_SHELL:-good}" == "good" ]]; then
        shell_ref='<script src="assets/app.js"></script>'
    else
        shell_ref='<!-- no bundle wired up -->'
    fi
    {
        echo '<!doctype html><html><head><title>web_new desktop</title></head>'
        echo "<body><div id=root></div>$shell_ref</body></html>"
    } > "$out/index.html"
    exit 0
fi
if [[ "${1:-}" == "exec" ]]; then
    # pnpm exec turbo run build --filter=web --force
    dev="${PIDASH_DESKTOP_DEV_TREE:?PIDASH_DESKTOP_DEV_TREE is required by the fake}"
    out="$dev/apps/web/build/client"
    mkdir -p "$out/assets"
    api="${VITE_API_BASE_URL:-http://localhost:8000}"
    {
        echo '<!doctype html><html><head><title>legacy web</title></head>'
        echo "<body>$api<div id=root></div>"
        printf 'y%.0s' $(seq 1 5000)
        echo '</body></html>'
    } > "$out/index.html"
    echo "// $api" > "$out/assets/app.js"
    exit 0
fi
echo "fake pnpm: unexpected arguments: $*" >&2
exit 99
"""

STUB_PREPARE_AGENT = """\
#!/usr/bin/env bash
set -euo pipefail
touch "$STUB_PREPARE_MARKER"
"""

STUB_MERGE_WEB_TREE = """\
#!/usr/bin/env bash
set -euo pipefail
echo "$@" > "$STUB_MERGE_ARGS"
mkdir -p "$1"
"""


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


class WebNewDesktopTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="web-new-desktop-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)

        # A synthetic OSS checkout. The script resolves OSS_DIR as the
        # grandparent of its own location, so this copy is rooted here.
        scripts = self.tmp / "desktop/scripts"
        scripts.mkdir(parents=True)
        shutil.copy2(SCRIPT, scripts / "dev-prep.sh")
        write(scripts / "prepare-agent.sh", STUB_PREPARE_AGENT)
        write(scripts / "merge-web-tree.sh", STUB_MERGE_WEB_TREE)

        # Stub binaries shadow the real toolchain for the heavy steps.
        self.fakebin = self.tmp / "fakebin"
        self.fakebin.mkdir()
        write(self.fakebin / "pnpm", FAKE_PNPM)
        write(self.fakebin / "corepack", "#!/usr/bin/env bash\nexit 0\n")
        for name in ("pnpm", "corepack"):
            tool = self.fakebin / name
            tool.chmod(tool.stat().st_mode | stat.S_IEXEC)

        self.dev_tree = self.tmp / "devtree"
        self.merge_args = self.tmp / "merge-args.txt"
        self.prepare_marker = self.tmp / "prepare-called"
        self.fake_log = self.tmp / "fake.log"
        self.dist = self.tmp / "desktop/src-tauri/dist"

    def run_dev_prep(self, extra_env=None):
        env = {
            "PATH": f"{self.fakebin}:/usr/bin:/bin",
            "PIDASH_DESKTOP_DEV_TREE": str(self.dev_tree),
            "VITE_API_BASE_URL": API_BASE,
            "STUB_MERGE_ARGS": str(self.merge_args),
            "STUB_PREPARE_MARKER": str(self.prepare_marker),
            "FAKE_LOG": str(self.fake_log),
            "FAKE_WEB_NEW_DESKTOP_MARKER": "1",
            "FAKE_WEB_NEW_SHELL": "good",
        }
        env.update(extra_env or {})
        return subprocess.run(
            ["bash", str(self.tmp / "desktop/scripts/dev-prep.sh")],
            capture_output=True,
            text=True,
            env=env,
        )

    def bake_info(self):
        return (self.dist / "bake-info.txt").read_text()

    def test_web_new_flag_builds_web_new_into_dist(self):
        write(self.tmp / "apps/web_new/package.json", '{"name":"web_new"}\n')
        # Deliberately no apps/web: the flagged path must not need the
        # legacy tree.
        result = self.run_dev_prep({"PIDASH_DESKTOP_WEB": "web_new"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("frontend=web_new", result.stdout)

        index = self.dist / "index.html"
        self.assertTrue(index.is_file())
        self.assertIn("assets/", index.read_text())
        self.assertTrue(any((self.dist / "assets").iterdir()))
        self.assertIn("desktop_api_request", (self.dist / "assets/app.js").read_text())
        self.assertIn("frontend=web_new", self.bake_info())
        self.assertIn(API_BASE, self.bake_info())

        # The overlay path never ran: no merge, no merged tree.
        self.assertFalse(self.merge_args.exists())
        self.assertFalse(self.dev_tree.exists())
        # The bundled runner setup still runs on the flagged path.
        self.assertTrue(self.prepare_marker.exists())

    def test_default_path_still_merges_the_overlay_tree(self):
        write(self.tmp / "apps/web/package.json", '{"name":"web"}\n')
        result = self.run_dev_prep()
        self.assertEqual(result.returncode, 0, result.stderr)

        # Same legacy assembly as before the flag existed.
        self.assertTrue(self.merge_args.is_file())
        self.assertEqual(self.merge_args.read_text().strip(), str(self.dev_tree))
        self.assertTrue(self.prepare_marker.exists())
        self.assertIn("turbo run build --filter=web", self.fake_log.read_text())
        self.assertIn(API_BASE, (self.dist / "index.html").read_text())
        self.assertNotIn("frontend=", self.bake_info())

    def test_unrecognized_flag_value_keeps_the_default_path(self):
        write(self.tmp / "apps/web/package.json", '{"name":"web"}\n')
        result = self.run_dev_prep({"PIDASH_DESKTOP_WEB": "legacy"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.merge_args.is_file())
        self.assertNotIn("frontend=", self.bake_info())

    def test_web_new_flag_rejects_a_non_desktop_bundle(self):
        write(self.tmp / "apps/web_new/package.json", '{"name":"web_new"}\n')
        result = self.run_dev_prep(
            {"PIDASH_DESKTOP_WEB": "web_new", "FAKE_WEB_NEW_DESKTOP_MARKER": "0"}
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no desktop platform marker", result.stderr)
        # The swap never ran: no half-populated dist/ left behind.
        self.assertFalse((self.dist / "index.html").exists())

    def test_web_new_flag_rejects_a_shell_without_assets(self):
        write(self.tmp / "apps/web_new/package.json", '{"name":"web_new"}\n')
        result = self.run_dev_prep(
            {"PIDASH_DESKTOP_WEB": "web_new", "FAKE_WEB_NEW_SHELL": "broken"}
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not reference its assets", result.stderr)
        self.assertFalse((self.dist / "index.html").exists())

    def test_web_new_flag_needs_apps_web_new(self):
        result = self.run_dev_prep({"PIDASH_DESKTOP_WEB": "web_new"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("apps/web_new is missing", result.stderr)


if __name__ == "__main__":
    unittest.main()
