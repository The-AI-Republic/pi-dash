# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The desktop app's default frontend (NEWFRONT-92, M-01).

`cargo tauri dev` and `cargo tauri build` run `dev-prep.sh` with no flags
and get apps/web_new built for the desktop target. There is no other path:
the retired overlay-merged apps/web tree, its tree merger and its test
runner are gone, and an environment that still asks for that tree is
refused loudly rather than silently given web_new.

These run the real script against a synthetic checkout: the script derives
its own `OSS_DIR` from its location, so copying it into a temp tree
exercises the shipped routing while stub neighbours (prepare-agent.sh,
pnpm, corepack) stand in for the heavy steps. The stubs record what the
script asked for, and materialize a fake bundle the script's own guards
then validate.
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
WEB_BASE = "https://app.example.test"

FAKE_PNPM = """\
#!/usr/bin/env bash
set -euo pipefail
echo "pnpm $*" >> "$FAKE_LOG"
if [[ "${1:-}" == "install" ]]; then
    exit 0
fi
if [[ "${1:-}" == "--filter" && ( "${2:-}" == "@pidash/kit" || "${2:-}" == "@pidash/api-client" ) ]]; then
    exit 0
fi
if [[ "$*" == *"--filter web_new build:desktop"* ]]; then
    out="apps/web_new/dist/desktop"
    mkdir -p "$out/assets"
    api="${VITE_API_BASE_URL:-}"
    web="${VITE_WEB_BASE_URL:-}"
    if [[ "${FAKE_WEB_NEW_DESKTOP_MARKER:-1}" == "1" ]]; then
        printf 'const t="desktop_api_request";const a="%s";const w="%s";\\n' "$api" "$web" \
            > "$out/assets/app.js"
    else
        printf 'const a="%s";const w="%s";\\n' "$api" "$web" > "$out/assets/app.js"
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
echo "fake pnpm: unexpected arguments: $*" >&2
exit 99
"""

STUB_PREPARE_AGENT = """\
#!/usr/bin/env bash
set -euo pipefail
touch "$STUB_PREPARE_MARKER"
"""

VERIFY_HOOK = """\
#!/usr/bin/env bash
set -euo pipefail
echo "$1" > "$STUB_VERIFY_ARGS"
[[ "$(basename "$1")" == "desktop" ]]
"""

REJECTING_HOOK = """\
#!/usr/bin/env bash
set -euo pipefail
echo "edition refuses this bundle" >&2
exit 1
"""


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


class DesktopFrontendTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="desktop-frontend-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)

        # A synthetic OSS checkout. The script resolves OSS_DIR as the
        # grandparent of its own location, so this copy is rooted here.
        scripts = self.tmp / "desktop/scripts"
        scripts.mkdir(parents=True)
        shutil.copy2(SCRIPT, scripts / "dev-prep.sh")
        write(scripts / "prepare-agent.sh", STUB_PREPARE_AGENT)
        write(self.tmp / "apps/web_new/package.json", '{"name":"web_new"}\n')
        # Deliberately no apps/web: the default path must not need the
        # retired tree.
        write(self.tmp / "apps/web/package.json", '{"name":"web"}\n')
        (self.tmp / "apps/web/package.json").unlink()

        # Stub binaries shadow the real toolchain for the heavy steps.
        self.fakebin = self.tmp / "fakebin"
        self.fakebin.mkdir()
        write(self.fakebin / "pnpm", FAKE_PNPM)
        write(self.fakebin / "corepack", "#!/usr/bin/env bash\nexit 0\n")
        for name in ("pnpm", "corepack"):
            tool = self.fakebin / name
            tool.chmod(tool.stat().st_mode | stat.S_IEXEC)

        self.prepare_marker = self.tmp / "prepare-called"
        self.fake_log = self.tmp / "fake.log"
        self.dist = self.tmp / "desktop/src-tauri/dist"

    def run_dev_prep(self, extra_env=None):
        env = {
            "PATH": f"{self.fakebin}:/usr/bin:/bin",
            "VITE_API_BASE_URL": API_BASE,
            "VITE_WEB_BASE_URL": WEB_BASE,
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

    def test_the_default_build_is_web_new(self):
        result = self.run_dev_prep()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("frontend=web_new", result.stdout)

        index = self.dist / "index.html"
        self.assertTrue(index.is_file())
        self.assertIn("assets/", index.read_text())
        self.assertTrue(any((self.dist / "assets").iterdir()))
        self.assertIn("desktop_api_request", (self.dist / "assets/app.js").read_text())
        self.assertIn("frontend=web_new", self.bake_info())
        self.assertIn(API_BASE, self.bake_info())
        self.assertIn(WEB_BASE, self.bake_info())

        # The bundled runner setup still runs.
        self.assertTrue(self.prepare_marker.exists())
        # One frontend, built for the desktop target, workspace deps first.
        log = self.fake_log.read_text()
        # Workspace deps build first: @pidash/kit's dist/kit.css (imported by
        # apps/web_new/src/styles/app.css) and @pidash/api-client must be built
        # before web_new, or Vite fails on a clean checkout (PIDESKAPP-185).
        kit_index = log.find("pnpm --filter @pidash/kit build")
        api_client_index = log.find("pnpm --filter @pidash/api-client build")
        desktop_index = log.find("pnpm --filter web_new build:desktop")
        self.assertGreaterEqual(kit_index, 0, log)
        self.assertGreaterEqual(api_client_index, 0, log)
        self.assertLess(kit_index, desktop_index, log)
        self.assertLess(api_client_index, desktop_index, log)
        self.assertNotIn("turbo", log)

    def test_legacy_frontend_values_are_refused(self):
        for value in ("legacy", "web", "turbo"):
            result = self.run_dev_prep({"PIDASH_DESKTOP_WEB": value})
            self.assertNotEqual(result.returncode, 0, value)
            self.assertIn("PIDASH_DESKTOP_WEB", result.stderr)
            self.assertIn("no longer supported", result.stderr)
            self.assertFalse((self.dist / "index.html").exists(), value)

    def test_the_retired_value_web_new_still_selects_web_new(self):
        result = self.run_dev_prep({"PIDASH_DESKTOP_WEB": "web_new"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("frontend=web_new", self.bake_info())

    def test_a_bundle_without_the_desktop_marker_is_refused(self):
        result = self.run_dev_prep({"FAKE_WEB_NEW_DESKTOP_MARKER": "0"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no desktop platform marker", result.stderr)
        # The swap never ran: no half-populated dist/ left behind.
        self.assertFalse((self.dist / "index.html").exists())

    def test_a_shell_without_its_assets_is_refused(self):
        result = self.run_dev_prep({"FAKE_WEB_NEW_SHELL": "broken"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not reference its assets", result.stderr)
        self.assertFalse((self.dist / "index.html").exists())

    def test_a_checkout_without_web_new_is_refused(self):
        shutil.rmtree(self.tmp / "apps/web_new")
        result = self.run_dev_prep()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("apps/web_new", result.stderr)

    def test_the_verify_hook_sees_the_built_bundle(self):
        hook = self.tmp / "verify.sh"
        write(hook, VERIFY_HOOK)
        args = self.tmp / "verify-args.txt"
        result = self.run_dev_prep({"PIDASH_DESKTOP_VERIFY_HOOK": str(hook), "STUB_VERIFY_ARGS": str(args)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(args.read_text().strip(), str(self.tmp / "apps/web_new/dist/desktop"))

    def test_a_verify_hook_can_reject_the_bundle(self):
        hook = self.tmp / "verify.sh"
        write(hook, REJECTING_HOOK)
        result = self.run_dev_prep({"PIDASH_DESKTOP_VERIFY_HOOK": str(hook)})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("verify hook rejected", result.stderr)
        self.assertFalse((self.dist / "index.html").exists())

    def test_a_missing_verify_hook_is_refused_before_building(self):
        result = self.run_dev_prep({"PIDASH_DESKTOP_VERIFY_HOOK": str(self.tmp / "nope.sh")})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("is not a file", result.stderr)
        self.assertFalse(self.fake_log.exists())

    def test_skip_reuses_the_existing_bundle(self):
        write(self.dist / "index.html", "<html>previous</html>")
        write(self.dist / "bake-info.txt", "frontend=web_new\napi_base=http://old\n")
        result = self.run_dev_prep({"PIDASH_SKIP_DEV_PREP": "1"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("frontend=web_new", result.stdout)
        self.assertEqual((self.dist / "index.html").read_text(), "<html>previous</html>")
        self.assertFalse(self.fake_log.exists())

    def test_hot_reload_builds_nothing(self):
        result = self.run_dev_prep({"PIDASH_DESKTOP_HOT_RELOAD": "1"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.fake_log.exists())
        self.assertFalse((self.dist / "index.html").exists())


if __name__ == "__main__":
    unittest.main()
