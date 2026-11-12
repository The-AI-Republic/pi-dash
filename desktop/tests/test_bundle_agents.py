# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Offline release packaging regressions for every advertised desktop target."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/bundle_agents.py"
SPEC = importlib.util.spec_from_file_location("bundle_agents", SCRIPT)
bundle = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bundle)


def archive_bytes(archive, members):
    output = io.BytesIO()
    if archive.endswith(".zip"):
        with zipfile.ZipFile(output, "w") as source:
            for name, value in members.items():
                source.writestr(name, value)
    else:
        with tarfile.open(fileobj=output, mode="w:gz") as source:
            for name, value in members.items():
                info = tarfile.TarInfo(name)
                info.size = len(value)
                source.addfile(info, io.BytesIO(value))
    return output.getvalue()


class BundleAgentTests(unittest.TestCase):
    def test_exact_linux_and_windows_asset_names(self):
        self.assertEqual(
            bundle.assets("x86_64-unknown-linux-gnu"),
            [
                (
                    "The-AI-Republic/pi-dash",
                    "pidash-x86_64-unknown-linux-gnu.tar.xz",
                    "pidash",
                    "pidash",
                )
            ],
        )
        self.assertEqual(
            bundle.assets("x86_64-pc-windows-msvc"),
            [
                (
                    "The-AI-Republic/pi-dash",
                    "pidash-x86_64-pc-windows-msvc.zip",
                    "pidash.exe",
                    "pidash.exe",
                )
            ],
        )

    def test_no_target_stages_an_engine(self):
        # The engine ships in the CLI release; no desktop target may carry it.
        for target in ("universal-apple-darwin", "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"):
            for architecture in bundle.targets(target):
                for repo, _, _, destination in bundle.assets(architecture):
                    self.assertNotEqual(repo, "openai/codex")
                    self.assertNotIn("agent-engine", destination)

    def test_all_targets_produce_exact_native_filenames_and_mac_slices(self):
        for target in (
            "universal-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
        ):
            with (
                self.subTest(target=target),
                tempfile.TemporaryDirectory() as directory,
            ):
                downloads = []
                lipo_calls = []

                def download(repo, tag, archive, pinned=None):
                    downloads.append((repo, tag, archive))
                    self.assertIsNone(pinned)
                    for architecture in bundle.targets(target):
                        for expected_repo, expected_archive, member, _ in bundle.assets(
                            architecture
                        ):
                            if (repo, archive) == (expected_repo, expected_archive):
                                return archive_bytes(
                                    archive, {f"package/{member}": architecture.encode()}
                                )
                    raise AssertionError(archive)

                def lipo(command, check):
                    lipo_calls.append(command)
                    self.assertTrue(check)
                    if command[1] == "-create":
                        self.assertEqual(len(command[2:-2]), 2)
                        payload = b"|".join(Path(p).read_bytes() for p in command[2:-2])
                        self.assertIn(b"aarch64-apple-darwin", payload)
                        self.assertIn(b"x86_64-apple-darwin", payload)
                        Path(command[-1]).write_bytes(payload)
                    else:
                        self.assertTrue(Path(command[1]).is_file())
                        self.assertEqual(command[2:], ["-verify_arch", "arm64", "x86_64"])

                with (
                    patch.object(bundle, "download", side_effect=download),
                    patch.object(bundle.subprocess, "run", side_effect=lipo),
                ):
                    bundle.stage(target, "pidash-test", directory)
                suffix = ".exe" if "windows" in target else ""
                self.assertEqual(
                    {p.name for p in Path(directory).iterdir()},
                    {f"pidash{suffix}"},
                )
                self.assertEqual(len(downloads), 2 if target.startswith("universal") else 1)
                self.assertEqual(len(lipo_calls), 2 if target.startswith("universal") else 0)

    def test_missing_or_ambiguous_executables_are_refused(self):
        for archive in ("test.tar.gz", "test.zip"):
            for members in ({"pidash-helper": b"wrong"}, {"a/pidash": b"one", "b/pidash": b"two"}):
                with self.assertRaises(ValueError):
                    bundle.executable_bytes(archive_bytes(archive, members), archive, "pidash")

    def test_checksum_is_verified_before_using_download(self):
        payload = b"fake archive bytes"
        digest = f"sha256:{hashlib.sha256(payload).hexdigest()}"

        def download(command, check):
            Path(command[-1], "agent.tar.gz").write_bytes(payload)

        for advertised, valid in ((digest, True), ("sha256:wrong", False), (None, False)):
            metadata = json.dumps({"assets": [{"name": "agent.tar.gz", "digest": advertised}]})
            with (
                patch.object(bundle.subprocess, "check_output", return_value=metadata),
                patch.object(bundle.subprocess, "run", side_effect=download),
            ):
                if valid:
                    self.assertEqual(bundle.download("repo/agent", "v1", "agent.tar.gz"), payload)
                else:
                    with self.assertRaises(ValueError):
                        bundle.download("repo/agent", "v1", "agent.tar.gz")

    def test_a_published_digest_that_differs_from_the_pin_is_refused(self):
        payload = b"fake archive bytes"
        digest = f"sha256:{hashlib.sha256(payload).hexdigest()}"
        metadata = json.dumps({"assets": [{"name": "agent.tar.gz", "digest": digest}]})

        def fetch(command, check):
            Path(command[-1], "agent.tar.gz").write_bytes(payload)

        with (
            patch.object(bundle.subprocess, "check_output", return_value=metadata),
            patch.object(bundle.subprocess, "run", side_effect=fetch),
        ):
            self.assertEqual(
                bundle.download("repo/agent", "v1", "agent.tar.gz", pinned=digest), payload
            )
            with self.assertRaises(ValueError):
                bundle.download("repo/agent", "v1", "agent.tar.gz", pinned="sha256:" + "0" * 64)


if __name__ == "__main__":
    unittest.main()
