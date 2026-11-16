# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Which path `prepare-agent.sh` takes: download a released CLI by default.

By default (no PIDASH_BUNDLE_CLI_FROM_SOURCE) every desktop build bundles the
latest released `pidash-v*` binary for the target platform via
`bundle_agents.py` — no `cargo build` of the CLI. The build must never ship a
dirty checkout. Building from source is only an explicit opt-in
(PIDASH_BUNDLE_CLI_FROM_SOURCE=1) for CLI development, and it warns loudly.

These run the real script against a synthetic checkout with `cargo`, `rustc`,
`python3` (and `lipo`) stubbed out: the script derives its own paths from its
location, so nothing is compiled and no network is touched.
"""

from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/prepare-agent.sh"

RUSTC_STUB = """#!/usr/bin/env bash
echo "host: x86_64-unknown-linux-gnu"
"""

RUSTC_STUB_ARM = """#!/usr/bin/env bash
echo "host: aarch64-apple-darwin"
"""

# From-source cargo stub: respects --target and CARGO_TARGET_DIR, records profiles.
CARGO_STUB = """#!/usr/bin/env bash
set -euo pipefail
manifest="" profile="" target=""
while (( $# )); do
  case "$1" in
    --manifest-path) manifest="$2"; shift ;;
    --profile) profile="$2"; shift ;;
    --target) target="$2"; shift ;;
  esac
  shift
done
# If no target was requested, behave like the old stub (host build for profile)
root="$(dirname "$manifest")"
# Record the requested profile for the default-profile tests
printf '%s\\n' "$profile" >> "$root/cargo-profiles.log"
# Also record the target when given
if [[ -n "$target" ]]; then
  printf '%s\\n' "$target" >> "$root/cargo-targets.log"
fi
out="$profile"
if [[ "$profile" == "dev" ]]; then out=debug; fi
target_dir="${CARGO_TARGET_DIR:-$root/target}"
if [[ -n "$target" ]]; then
  mkdir -p "$target_dir/$target/$out"
  printf 'pidash built into %s/%s\\n' "$target" "$out" > "$target_dir/$target/$out/pidash"
else
  mkdir -p "$target_dir/$out"
  printf 'pidash built into target/%s\\n' "$out" > "$target_dir/$out/pidash"
fi
"""

PYTHON_STUB = """#!/usr/bin/env bash
# Stub for `python3 bundle_agents.py --target X --output Y [--runner-tag TAG]`.
# Records the call and writes a fake released binary + version file, the way
# real bundle_agents.py would.
set -euo pipefail
target="" output="" tag=""
while (( $# )); do
  case "$1" in
    --target) target="$2"; shift ;;
    --output) output="$2"; shift ;;
    --runner-tag) tag="$2"; shift ;;
  esac
  shift
done
# Output is the bin dir (e.g. /tmp/.../desktop/src-tauri/bin)
mkdir -p "$output"
echo "pidash released $tag for $target" > "$output/pidash"
chmod 755 "$output/pidash"
# version file: strip pidash-v prefix if present
ver="$tag"
if [[ -n "$ver" ]]; then
  ver="${ver#pidash-v}"
  ver="${ver#v}"
else
  ver="0.1.30"
fi
printf '%s\\n' "$ver" > "$output/pidash.version"
# Record for assertions
root="$(dirname "$output")/.."  # not reliable; use env var if set
if [[ -n "${PREPARE_AGENT_TEST_LOG:-}" ]]; then
  printf 'target=%s tag=%s\\n' "$target" "$tag" >> "${PREPARE_AGENT_TEST_LOG}/bundle-calls.log"
fi
"""

LIPO_STUB = """#!/usr/bin/env bash
# Stub lipo: -create A B -output OUT  → concatenate
set -euo pipefail
if [[ "$1" == "-create" ]]; then
  shift
  out=""
  inputs=()
  while (( $# )); do
    if [[ "$1" == "-output" ]]; then
      out="$2"
      shift 2
      break
    fi
    inputs+=("$1")
    shift
  done
  # just copy first input, mark as universal
  cat "${inputs[0]}" > "$out"
  echo " universal" >> "$out"
  chmod 755 "$out"
else
  # -verify_arch
  exit 0
fi
"""


def write(path: Path, text: str, executable: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    if executable:
        path.chmod(0o755)


class BasePrepareAgentTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="prepare-agent-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.script = self.tmp / "desktop/scripts/prepare-agent.sh"
        self.script.parent.mkdir(parents=True)
        shutil.copy2(SCRIPT, self.script)
        write(self.tmp / "Cargo.toml", "[workspace]\n")
        write(self.tmp / "LICENSE.txt", "license\n")
        # Minimal runner Cargo.toml for from-source version extraction
        write(self.tmp / "runner/Cargo.toml", '[package]\nname = "pidash"\nversion = "0.1.30"\n')
        # bundle_agents.py exists (real one) — default path invokes it via python3
        # stub; we need the file to exist so the script's check passes.
        write(self.tmp / "desktop/scripts/bundle_agents.py", "# stub placeholder\n")

        write(self.tmp / "stubs/rustc", RUSTC_STUB, executable=True)
        write(self.tmp / "stubs/cargo", CARGO_STUB, executable=True)
        write(self.tmp / "stubs/python3", PYTHON_STUB, executable=True)
        write(self.tmp / "stubs/lipo", LIPO_STUB, executable=True)

        self.bin = self.tmp / "desktop/src-tauri/bin"
        self.licences = self.tmp / "desktop/src-tauri/licenses"
        self.log = self.tmp / "log"
        self.log.mkdir()

    def prepare(self, check=True, **env):
        base_env = {
            "PATH": f"{self.tmp / 'stubs'}:/usr/bin:/bin:/usr/local/bin",
            "PREPARE_AGENT_TEST_LOG": str(self.log),
        }
        base_env.update(env)
        return subprocess.run(
            ["bash", str(self.script)],
            capture_output=True,
            text=True,
            env=base_env,
            check=check,
        )

    def cargo_profiles(self):
        p = self.tmp / "cargo-profiles.log"
        return p.read_text().split() if p.exists() else []

    def bundle_calls(self):
        p = self.log / "bundle-calls.log"
        return p.read_text().strip() if p.exists() else ""

    # The environment the Tauri CLI hands a before* hook.
    RELEASE_BUILD = {"TAURI_ENV_PLATFORM": "linux"}
    DEBUG_BUILD = {"TAURI_ENV_PLATFORM": "linux", "TAURI_ENV_DEBUG": "true"}


@unittest.skipIf(sys.platform == "win32", "stubs are POSIX shell scripts")
class PrepareAgentDefaultTests(BasePrepareAgentTests):
    """Default path: download a released CLI, never `cargo build`."""

    def test_default_downloads_without_cargo(self):
        result = self.prepare(**self.RELEASE_BUILD)
        self.assertEqual(self.cargo_profiles(), [], "default must not run cargo")
        self.assertTrue((self.bin / "pidash").exists())
        self.assertIn("pidash released", (self.bin / "pidash").read_text())
        self.assertEqual((self.bin / "pidash.version").read_text().strip(), "0.1.30")
        self.assertEqual({p.name for p in self.licences.iterdir()}, {"pidash-LICENSE.txt"})
        self.assertIn("Downloading released CLI", result.stdout + result.stderr)

    def test_default_uses_pinned_version_when_set(self):
        self.prepare(**self.RELEASE_BUILD, PIDASH_BUNDLE_VERSION="pidash-v0.1.29")
        calls = self.bundle_calls()
        self.assertIn("tag=pidash-v0.1.29", calls)
        self.assertEqual((self.bin / "pidash.version").read_text().strip(), "0.1.29")

    def test_default_resolves_target_from_tauri_env(self):
        # TAURI_ENV_TARGET_TRIPLE wins over the host triple.
        self.prepare(TAURI_ENV_TARGET_TRIPLE="aarch64-apple-darwin")
        calls = self.bundle_calls()
        self.assertIn("target=universal-apple-darwin", calls)

    def test_macos_single_arch_normalizes_to_universal(self):
        self.prepare(TAURI_ENV_TARGET_TRIPLE="x86_64-apple-darwin")
        calls = self.bundle_calls()
        self.assertIn("target=universal-apple-darwin", calls)
        # version file still written
        self.assertTrue((self.bin / "pidash.version").exists())

    def test_linux_target_passed_through(self):
        self.prepare(TAURI_ENV_TARGET_TRIPLE="aarch64-unknown-linux-gnu")
        calls = self.bundle_calls()
        self.assertIn("target=aarch64-unknown-linux-gnu", calls)

    def test_default_falls_back_to_host_triple(self):
        # No TAURI_ENV_TARGET_TRIPLE → host triple from rustc (x86_64-unknown-linux-gnu)
        self.prepare()
        calls = self.bundle_calls()
        self.assertIn("target=x86_64-unknown-linux-gnu", calls)

    def test_only_cli_and_version_and_licence_are_staged(self):
        self.prepare(**self.RELEASE_BUILD)
        names = {p.name for p in self.bin.iterdir()}
        # Default download stages the binary plus the version file (no engine).
        self.assertIn("pidash", names)
        self.assertIn("pidash.version", names)
        self.assertNotIn("pidash-agent-engine", names)
        self.assertNotIn("engine-version.txt", names)


@unittest.skipIf(sys.platform == "win32", "stubs are POSIX shell scripts")
class PrepareAgentFromSourceTests(BasePrepareAgentTests):
    """Opt-in from-source path: cargo build with target and warning."""

    FROM_SOURCE = {"PIDASH_BUNDLE_CLI_FROM_SOURCE": "1"}

    def prepare_from_source(self, **env):
        merged = {**self.FROM_SOURCE, **env}
        return self.prepare(**merged)

    def test_from_source_builds_with_cargo_and_warns(self):
        result = self.prepare_from_source(**self.RELEASE_BUILD)
        self.assertEqual(self.cargo_profiles(), ["release"])
        self.assertIn("pidash built into", (self.bin / "pidash").read_text())
        self.assertIn("WARNING", result.stderr)
        self.assertIn("PIDASH_BUNDLE_CLI_FROM_SOURCE=1", result.stderr)
        # version file from runner/Cargo.toml
        self.assertEqual((self.bin / "pidash.version").read_text().strip(), "0.1.30")

    def test_from_source_passes_target_to_cargo(self):
        self.prepare_from_source(TAURI_ENV_TARGET_TRIPLE="aarch64-unknown-linux-gnu")
        targets = (self.tmp / "cargo-targets.log").read_text().split() if (self.tmp / "cargo-targets.log").exists() else []
        self.assertIn("aarch64-unknown-linux-gnu", targets)

    def test_from_source_respects_cargo_target_dir(self):
        custom = self.tmp / "custom-target"
        # Use a release build so the profile is release; pass both triple and platform
        self.prepare_from_source(TAURI_ENV_TARGET_TRIPLE="x86_64-unknown-linux-gnu", TAURI_ENV_PLATFORM="linux", CARGO_TARGET_DIR=str(custom))
        # The stub wrote into custom-target/x86_64-unknown-linux-gnu/release/pidash
        self.assertTrue((custom / "x86_64-unknown-linux-gnu" / "release" / "pidash").exists())
        # and the script copied from there into bin/
        self.assertIn("pidash built into", (self.bin / "pidash").read_text())

    def test_profile_default_follows_tauri_build_signal(self):
        # Release build → release profile
        self.prepare_from_source(**self.RELEASE_BUILD)
        self.assertEqual(self.cargo_profiles(), ["release"])
        # Tauri dev → dev (debug dir) profile
        # Clean logs
        (self.tmp / "cargo-profiles.log").write_text("")
        if (self.tmp / "cargo-targets.log").exists():
            (self.tmp / "cargo-targets.log").write_text("")
        self.prepare_from_source(**self.DEBUG_BUILD)
        self.assertEqual(self.cargo_profiles(), ["dev"])

    def test_universal_from_source_lipos_both_slices(self):
        self.prepare_from_source(TAURI_ENV_TARGET_TRIPLE="aarch64-apple-darwin")
        # universal target triggers two cargo builds
        self.assertGreaterEqual(len(self.cargo_profiles()), 2)
        self.assertIn("universal", (self.bin / "pidash").read_text())


if __name__ == "__main__":
    unittest.main()
