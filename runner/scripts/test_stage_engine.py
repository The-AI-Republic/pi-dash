# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Offline tests for stage_engine.py (no `gh`, no network)."""

import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parent / "stage_engine.py"
SPEC = importlib.util.spec_from_file_location("stage_engine", SCRIPT)
stage_engine = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage_engine)

TAG = "rust-v0.153.4"


def tarball(members):
    """A .tar.gz holding `members` (name -> bytes, or None for a symlink)."""
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w:gz") as archive:
        for name, data in members.items():
            info = tarfile.TarInfo(name)
            if data is None:
                info.type = tarfile.SYMTYPE
                info.linkname = "/etc/passwd"
                archive.addfile(info)
            else:
                info.size = len(data)
                info.mode = 0o755
                archive.addfile(info, io.BytesIO(data))
    return out.getvalue()


def fake_fetch(payload, digest=None):
    digest = digest or "sha256:" + hashlib.sha256(payload).hexdigest()

    def fetch(tag, asset):
        return payload, digest, b"Apache License 2.0\n", b"OpenAI Codex\n"

    return fetch


class AssetTests(unittest.TestCase):
    def test_linux_gnu_ships_the_static_musl_engine(self):
        for target in ("x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"):
            self.assertEqual(
                stage_engine.upstream_asset(target),
                (
                    "codex-x86_64-unknown-linux-musl.tar.gz",
                    "codex-x86_64-unknown-linux-musl",
                    "pidash-agent-engine",
                ),
            )

    def test_windows_asset_keeps_the_exe_suffix(self):
        self.assertEqual(
            stage_engine.upstream_asset("x86_64-pc-windows-msvc"),
            (
                "codex-x86_64-pc-windows-msvc.exe.tar.gz",
                "codex-x86_64-pc-windows-msvc.exe",
                "pidash-agent-engine.exe",
            ),
        )

    def test_every_cli_release_target_is_supported(self):
        dist = (SCRIPT.parents[2] / "dist-workspace.toml").read_text()
        block = dist.split("targets = [", 1)[1].split("]", 1)[0]
        targets = {t.strip().strip('"') for t in block.split(",") if t.strip().strip('"')}
        self.assertTrue(targets)
        self.assertEqual(targets, stage_engine.SUPPORTED_TARGETS)

    def test_unknown_target_is_rejected(self):
        with self.assertRaises(ValueError):
            stage_engine.upstream_asset("riscv64gc-unknown-linux-gnu")


class VersionFileTests(unittest.TestCase):
    def test_repo_version_file(self):
        tag = stage_engine.read_engine_tag()
        self.assertRegex(tag, r"^rust-v\d+\.\d+\.\d+")
        self.assertEqual(stage_engine.version_from_tag(tag), tag[len("rust-v") :])

    def test_malformed_version_file_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ENGINE_VERSION"
            path.write_text("0.153.4\n")
            with self.assertRaises(ValueError):
                stage_engine.read_engine_tag(path)


class StageTests(unittest.TestCase):
    def setUp(self):
        self.out = Path(tempfile.mkdtemp(prefix="stage-engine-"))
        self.addCleanup(lambda: __import__("shutil").rmtree(self.out, True))

    def test_stages_renamed_engine_with_licence_files(self):
        payload = tarball({"codex-x86_64-unknown-linux-musl": b"ELF engine"})
        directory = stage_engine.stage(
            "x86_64-unknown-linux-gnu", self.out, TAG, fetch=fake_fetch(payload)
        )
        self.assertEqual(directory, self.out / "engine/0.153.4")
        self.assertEqual(
            sorted(p.name for p in directory.iterdir()),
            ["LICENSE", "NOTICE", "README.txt", "pidash-agent-engine"],
        )
        self.assertEqual((directory / "pidash-agent-engine").read_bytes(), b"ELF engine")
        self.assertIn("rust-v0.153.4", (directory / "README.txt").read_text())
        self.assertFalse((self.out / "engine.wxi").exists())

    def test_windows_stages_the_msi_version_define(self):
        payload = tarball({"codex-x86_64-pc-windows-msvc.exe": b"MZ engine"})
        directory = stage_engine.stage(
            "x86_64-pc-windows-msvc", self.out, TAG, fetch=fake_fetch(payload)
        )
        self.assertTrue((directory / "pidash-agent-engine.exe").is_file())
        wxi = (self.out / "engine.wxi").read_text()
        self.assertIn('<?define EngineVersion = "0.153.4" ?>', wxi)

    def test_checksum_mismatch_stages_nothing(self):
        payload = tarball({"codex-x86_64-unknown-linux-musl": b"ELF engine"})
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            stage_engine.stage(
                "x86_64-unknown-linux-gnu",
                self.out,
                TAG,
                fetch=fake_fetch(payload, digest="sha256:" + "0" * 64),
            )
        self.assertFalse((self.out / "engine").exists())

    def test_symlink_member_is_not_an_engine(self):
        payload = tarball({"codex-x86_64-unknown-linux-musl": None})
        with self.assertRaisesRegex(ValueError, "Expected exactly one"):
            stage_engine.stage(
                "x86_64-unknown-linux-gnu", self.out, TAG, fetch=fake_fetch(payload)
            )

    def test_restaging_replaces_a_previous_target(self):
        stage_engine.stage(
            "x86_64-pc-windows-msvc",
            self.out,
            "rust-v0.150.0",
            fetch=fake_fetch(tarball({"codex-x86_64-pc-windows-msvc.exe": b"MZ"})),
        )
        stage_engine.stage(
            "aarch64-apple-darwin",
            self.out,
            TAG,
            fetch=fake_fetch(tarball({"codex-aarch64-apple-darwin": b"Mach-O"})),
        )
        self.assertEqual([p.name for p in (self.out / "engine").iterdir()], ["0.153.4"])
        self.assertEqual(
            [p.name for p in (self.out / "engine/0.153.4").iterdir() if p.name.startswith("pidash")],
            ["pidash-agent-engine"],
        )
        self.assertFalse((self.out / "engine.wxi").exists())

    def test_missing_digest_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "Missing SHA-256"):
            stage_engine.release_digest({"assets": [{"name": "x", "digest": None}]}, "x")


if __name__ == "__main__":
    unittest.main()
