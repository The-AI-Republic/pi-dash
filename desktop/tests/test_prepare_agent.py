# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Offline regressions for prepare-agent.sh's engine download scratch space."""

import hashlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import textwrap
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/prepare-agent.sh"
TARGET = "x86_64-unknown-linux-gnu"
ASSET = "codex-x86_64-unknown-linux-musl.tar.gz"
ENGINE = "#!/bin/sh\necho engine 0.0.0\n"

GH_STUB = textwrap.dedent(
    """\
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ "$1" == release ]]; then
      while [[ $# -gt 0 ]]; do
        if [[ "$1" == --dir ]]; then cp "$FAKE_ARCHIVE" "$2/"; exit 0; fi
        shift
      done
      exit 1
    fi
    case "$2" in
      */releases/tags/*) printf '%s\\n' "$FAKE_DIGEST" ;;
      *) echo "license text" ;;
    esac
    """
)


def write_executable(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(0o755)


@unittest.skipUnless(
    os.name == "posix" and shutil.which("bash") and shutil.which("shasum"),
    "needs bash and shasum",
)
class PrepareAgentTempDirTests(unittest.TestCase):
    def run_script(self, digest=None):
        """Run a copy of the script against stubbed tools; return (result, leftovers, staged)."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = root / "oss/desktop/scripts"
            scripts.mkdir(parents=True)
            shutil.copy(SCRIPT, scripts / "prepare-agent.sh")
            (root / "oss/LICENSE.txt").write_text("license\n")
            write_executable(root / "oss/target/debug/pidash", "#!/bin/sh\n")

            output = io.BytesIO()
            with tarfile.open(fileobj=output, mode="w:gz") as source:
                info = tarfile.TarInfo(ASSET.removesuffix(".tar.gz"))
                info.size = len(ENGINE)
                info.mode = 0o755
                source.addfile(info, io.BytesIO(ENGINE.encode()))
            archive = root / "upstream" / ASSET
            archive.parent.mkdir()
            archive.write_bytes(output.getvalue())
            if digest is None:
                digest = "sha256:" + hashlib.sha256(output.getvalue()).hexdigest()

            stubs = root / "stubs"
            write_executable(stubs / "rustc", f"#!/bin/sh\necho 'host: {TARGET}'\n")
            write_executable(stubs / "cargo", "#!/bin/sh\n")
            write_executable(stubs / "gh", GH_STUB)

            scratch = root / "tmp"
            scratch.mkdir()
            result = subprocess.run(
                ["bash", str(scripts / "prepare-agent.sh")],
                env={
                    **os.environ,
                    "PATH": f"{stubs}{os.pathsep}{os.environ['PATH']}",
                    "TMPDIR": str(scratch),
                    "FAKE_ARCHIVE": str(archive),
                    "FAKE_DIGEST": digest,
                },
                capture_output=True,
                text=True,
                check=False,
            )
            leftovers = sorted(str(p.relative_to(scratch)) for p in scratch.rglob("*"))
            staged = (root / "oss/desktop/src-tauri/bin/pidash-agent-engine").exists()
            return result, leftovers, staged

    def test_download_dir_is_removed_after_staging(self):
        result, leftovers, staged = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(staged)
        self.assertEqual(leftovers, [])

    def test_download_dir_is_removed_on_checksum_mismatch(self):
        result, leftovers, staged = self.run_script(digest="sha256:" + "0" * 64)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr)
        self.assertFalse(staged)
        self.assertEqual(leftovers, [])

    def test_download_dir_is_removed_on_missing_checksum(self):
        result, leftovers, staged = self.run_script(digest="")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing upstream checksum", result.stderr)
        self.assertFalse(staged)
        self.assertEqual(leftovers, [])


if __name__ == "__main__":
    unittest.main()
