# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Which cargo profile `prepare-agent.sh` builds the bundled runner with.

`dev-prep.sh` is wired to both `beforeDevCommand` and `beforeBuildCommand`, so
the same script stages `src-tauri/bin/pidash` for `cargo tauri dev` and for a
local `cargo tauri build`. Whatever lands in `bin/` is sealed into the bundle
and is the daemon the shipped app spawns for every agent task, so a release
bundle must not pick up the unoptimized debug runner just because nobody set
`PIDASH_RUNNER_PROFILE`.

The Tauri CLI tells its before* hooks which kind of build they serve: every
hook gets `TAURI_ENV_PLATFORM`, and `TAURI_ENV_DEBUG=true` only for `tauri dev`
and `tauri build --debug`. tauri-cli 2.10 leaves `TAURI_ENV_DEBUG` unset for a
release build rather than setting it to `false`, so both spellings are covered.
The default profile follows that signal.

These run the real script against a synthetic checkout with `cargo`, `rustc`
and the agent engine stubbed out: the script derives its own paths from its
location, so nothing is compiled or downloaded.
"""

from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/prepare-agent.sh"

ENGINE_TAG = "rust-v0.0.0-test"

RUSTC_STUB = """#!/usr/bin/env bash
echo "host: x86_64-unknown-linux-gnu"
"""

# Records the requested profile and leaves a "binary" where cargo would, tagged
# with the directory it was built into so the test can tell which one was staged.
CARGO_STUB = """#!/usr/bin/env bash
set -euo pipefail
manifest="" profile=""
while (( $# )); do
  case "$1" in
    --manifest-path) manifest="$2"; shift ;;
    --profile) profile="$2"; shift ;;
  esac
  shift
done
root="$(dirname "$manifest")"
printf '%s\\n' "$profile" >> "$root/cargo-profiles.log"
out="$profile"
if [[ "$profile" == dev ]]; then out=debug; fi
mkdir -p "$root/target/$out"
printf 'pidash built into target/%s\\n' "$out" > "$root/target/$out/pidash"
"""

ENGINE_STUB = """#!/usr/bin/env bash
echo "stub-engine 0.0.0"
"""


def write(path: Path, text: str, executable: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    if executable:
        path.chmod(0o755)


@unittest.skipIf(sys.platform == "win32", "stubs are POSIX shell scripts")
class PrepareAgentProfileTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="prepare-agent-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)

        # A synthetic OSS checkout. The script resolves the desktop dir as the
        # parent of the directory holding it, and the OSS root above that.
        self.script = self.tmp / "desktop/scripts/prepare-agent.sh"
        self.script.parent.mkdir(parents=True)
        shutil.copy2(SCRIPT, self.script)
        write(self.tmp / "Cargo.toml", "[workspace]\n")
        write(self.tmp / "LICENSE.txt", "license\n")

        write(self.tmp / "stubs/rustc", RUSTC_STUB, executable=True)
        write(self.tmp / "stubs/cargo", CARGO_STUB, executable=True)

        # An already-staged engine at the requested tag, so the script never
        # reaches for `gh`.
        self.bin = self.tmp / "desktop/src-tauri/bin"
        write(self.bin / "engine-version.txt", ENGINE_TAG + "\n")
        write(self.bin / "pidash-agent-engine", ENGINE_STUB, executable=True)

    # The environment the Tauri CLI hands a before* hook.
    RELEASE_BUILD = {"TAURI_ENV_PLATFORM": "linux"}
    DEBUG_BUILD = {"TAURI_ENV_PLATFORM": "linux", "TAURI_ENV_DEBUG": "true"}

    def prepare(self, **env):
        return subprocess.run(
            ["bash", str(self.script)],
            capture_output=True,
            text=True,
            env={
                "PATH": f"{self.tmp / 'stubs'}:/usr/bin:/bin:/usr/local/bin",
                "CODEX_BUNDLE_VERSION": ENGINE_TAG,
                **env,
            },
            check=True,
        )

    def built_profiles(self):
        return (self.tmp / "cargo-profiles.log").read_text().split()

    def staged_runner(self):
        return (self.bin / "pidash").read_text().strip()

    def test_release_tauri_build_stages_a_release_runner(self):
        self.prepare(**self.RELEASE_BUILD)

        self.assertEqual(self.built_profiles(), ["release"])
        self.assertEqual(self.staged_runner(), "pidash built into target/release")

    def test_release_tauri_build_with_debug_spelled_false_stages_a_release_runner(self):
        self.prepare(**self.RELEASE_BUILD, TAURI_ENV_DEBUG="false")

        self.assertEqual(self.built_profiles(), ["release"])
        self.assertEqual(self.staged_runner(), "pidash built into target/release")

    def test_tauri_dev_stages_a_dev_runner(self):
        self.prepare(**self.DEBUG_BUILD)

        self.assertEqual(self.built_profiles(), ["dev"])
        self.assertEqual(self.staged_runner(), "pidash built into target/debug")

    def test_outside_tauri_stages_a_dev_runner(self):
        self.prepare()

        self.assertEqual(self.built_profiles(), ["dev"])
        self.assertEqual(self.staged_runner(), "pidash built into target/debug")

    def test_explicit_profile_wins_in_tauri_dev(self):
        self.prepare(**self.DEBUG_BUILD, PIDASH_RUNNER_PROFILE="release")

        self.assertEqual(self.built_profiles(), ["release"])
        self.assertEqual(self.staged_runner(), "pidash built into target/release")

    def test_explicit_dev_profile_in_a_release_build_is_honoured_loudly(self):
        result = self.prepare(**self.RELEASE_BUILD, PIDASH_RUNNER_PROFILE="dev")

        self.assertEqual(self.built_profiles(), ["dev"])
        self.assertEqual(self.staged_runner(), "pidash built into target/debug")
        self.assertIn("WARNING", result.stderr)
        self.assertIn("PIDASH_RUNNER_PROFILE=dev", result.stderr)

    def test_release_build_without_an_override_does_not_warn(self):
        result = self.prepare(**self.RELEASE_BUILD)

        self.assertNotIn("WARNING", result.stderr)


if __name__ == "__main__":
    unittest.main()
