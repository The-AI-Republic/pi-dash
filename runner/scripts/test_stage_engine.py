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


def digest_of(payload):
    return "sha256:" + hashlib.sha256(payload).hexdigest()


def target_payloads(target, engine_member=b"ELF engine", engine_tarball=None):
    """{asset: .tar.gz bytes} for `target`: the engine plus every helper."""
    payloads = {}
    asset, member, _ = stage_engine.upstream_asset(target)
    payloads[asset] = engine_tarball if engine_tarball is not None else tarball({member: engine_member})
    for helper_asset, helper_member_name, _ in stage_engine.upstream_helpers(target):
        payloads[helper_asset] = tarball({helper_member_name: b"helper binary"})
    return payloads


def pins_for_payloads(payloads):
    """Pins that accept every archive in `payloads`."""
    return {asset: digest_of(payload) for asset, payload in payloads.items()}


def fake_fetch(payloads, digests=None):
    digests = digests or {}

    def fetch(tag, asset):
        payload = payloads[asset]
        return payload, digests.get(asset, digest_of(payload)), b"Apache License 2.0\n", b"OpenAI Codex\n"

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

    def test_linux_targets_stage_the_bundled_bwrap(self):
        for target in ("x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"):
            self.assertEqual(
                stage_engine.upstream_helpers(target),
                [
                    (
                        "bwrap-x86_64-unknown-linux-musl.tar.gz",
                        "bwrap-x86_64-unknown-linux-musl",
                        "bwrap",
                    )
                ],
            )
        self.assertEqual(
            stage_engine.upstream_helpers("aarch64-unknown-linux-gnu"),
            [
                (
                    "bwrap-aarch64-unknown-linux-musl.tar.gz",
                    "bwrap-aarch64-unknown-linux-musl",
                    "bwrap",
                )
            ],
        )

    def test_windows_stages_both_sandbox_helpers(self):
        self.assertEqual(
            stage_engine.upstream_helpers("x86_64-pc-windows-msvc"),
            [
                (
                    "codex-command-runner-x86_64-pc-windows-msvc.exe.tar.gz",
                    "codex-command-runner-x86_64-pc-windows-msvc.exe",
                    "codex-command-runner.exe",
                ),
                (
                    "codex-windows-sandbox-setup-x86_64-pc-windows-msvc.exe.tar.gz",
                    "codex-windows-sandbox-setup-x86_64-pc-windows-msvc.exe",
                    "codex-windows-sandbox-setup.exe",
                ),
            ],
        )

    def test_macos_stages_no_helpers(self):
        self.assertEqual(stage_engine.upstream_helpers("aarch64-apple-darwin"), [])
        self.assertEqual(stage_engine.upstream_helpers("x86_64-apple-darwin"), [])

    def test_unknown_target_helpers_rejected(self):
        with self.assertRaises(ValueError):
            stage_engine.upstream_helpers("riscv64gc-unknown-linux-gnu")


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


class PinsFileTests(unittest.TestCase):
    def test_repo_pins_cover_every_target_for_the_pinned_tag(self):
        tag = stage_engine.read_engine_tag()
        pins = stage_engine.read_pins(tag)
        for target in stage_engine.SUPPORTED_TARGETS:
            asset, _, _ = stage_engine.upstream_asset(target)
            self.assertRegex(pins[asset], r"^sha256:[0-9a-f]{64}$")

    def test_repo_pins_cover_every_helper_for_the_pinned_tag(self):
        tag = stage_engine.read_engine_tag()
        pins = stage_engine.read_pins(tag)
        for target in stage_engine.SUPPORTED_TARGETS:
            for asset, _, _ in stage_engine.upstream_helpers(target):
                self.assertRegex(pins[asset], r"^sha256:[0-9a-f]{64}$")

    def test_pins_for_another_tag_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "update both together"):
            stage_engine.read_pins("rust-v9.9.9")

    def test_malformed_pin_line_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ENGINE_SHA256"
            path.write_text(f"tag {TAG}\nnot-a-digest  codex-x.tar.gz\n")
            with self.assertRaisesRegex(ValueError, "expected"):
                stage_engine.read_pins(TAG, path)


class StageTests(unittest.TestCase):
    def setUp(self):
        self.out = Path(tempfile.mkdtemp(prefix="stage-engine-"))
        self.addCleanup(lambda: __import__("shutil").rmtree(self.out, True))

    def test_stages_renamed_engine_with_licence_files(self):
        payloads = target_payloads("x86_64-unknown-linux-gnu", b"ELF engine")
        directory = stage_engine.stage(
            "x86_64-unknown-linux-gnu",
            self.out,
            TAG,
            pins_for_payloads(payloads),
            fetch=fake_fetch(payloads),
        )
        self.assertEqual(directory, self.out / "engine/0.153.4")
        self.assertEqual(
            sorted(p.name for p in directory.iterdir()),
            ["LICENSE", "NOTICE", "README.txt", "bwrap", "pidash-agent-engine"],
        )
        self.assertEqual((directory / "pidash-agent-engine").read_bytes(), b"ELF engine")
        self.assertIn("rust-v0.153.4", (directory / "README.txt").read_text())
        self.assertFalse((self.out / "engine.wxi").exists())

    def test_linux_stages_bwrap_beside_the_engine(self):
        payloads = target_payloads("aarch64-unknown-linux-musl", b"ELF engine")
        directory = stage_engine.stage(
            "aarch64-unknown-linux-musl",
            self.out,
            TAG,
            pins_for_payloads(payloads),
            fetch=fake_fetch(payloads),
        )
        self.assertEqual((directory / "bwrap").read_bytes(), b"helper binary")
        self.assertTrue((directory / "bwrap").stat().st_mode & 0o111)

    def test_macos_stages_no_helpers(self):
        payloads = target_payloads("aarch64-apple-darwin", b"Mach-O")
        directory = stage_engine.stage(
            "aarch64-apple-darwin",
            self.out,
            TAG,
            pins_for_payloads(payloads),
            fetch=fake_fetch(payloads),
        )
        self.assertEqual(
            sorted(p.name for p in directory.iterdir()),
            ["LICENSE", "NOTICE", "README.txt", "pidash-agent-engine"],
        )

    def test_windows_stages_the_msi_version_define(self):
        payloads = target_payloads("x86_64-pc-windows-msvc", b"MZ engine")
        directory = stage_engine.stage(
            "x86_64-pc-windows-msvc",
            self.out,
            TAG,
            pins_for_payloads(payloads),
            fetch=fake_fetch(payloads),
        )
        self.assertTrue((directory / "pidash-agent-engine.exe").is_file())
        self.assertEqual((directory / "codex-command-runner.exe").read_bytes(), b"helper binary")
        self.assertEqual(
            (directory / "codex-windows-sandbox-setup.exe").read_bytes(), b"helper binary"
        )
        wxi = (self.out / "engine.wxi").read_text()
        self.assertIn('<?define EngineVersion = "0.153.4" ?>', wxi)

    def test_checksum_mismatch_stages_nothing(self):
        payloads = target_payloads("x86_64-unknown-linux-gnu", b"ELF engine")
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            stage_engine.stage(
                "x86_64-unknown-linux-gnu",
                self.out,
                TAG,
                {"codex-x86_64-unknown-linux-musl.tar.gz": "sha256:" + "0" * 64},
                fetch=fake_fetch(payloads, digests={"codex-x86_64-unknown-linux-musl.tar.gz": "sha256:" + "0" * 64}),
            )
        self.assertFalse((self.out / "engine").exists())

    def test_helper_checksum_mismatch_stages_nothing(self):
        # The bwrap archive's payload does not match its pinned digest,
        # while upstream metadata agrees with the pin: a swapped download.
        payloads = target_payloads("x86_64-unknown-linux-gnu", b"ELF engine")
        pins = pins_for_payloads(payloads)
        pins["bwrap-x86_64-unknown-linux-musl.tar.gz"] = "sha256:" + "0" * 64
        fetch = fake_fetch(payloads, digests={"bwrap-x86_64-unknown-linux-musl.tar.gz": "sha256:" + "0" * 64})
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            stage_engine.stage(
                "x86_64-unknown-linux-gnu",
                self.out,
                TAG,
                pins,
                fetch=fetch,
            )
        self.assertFalse((self.out / "engine").exists())

    def test_asset_without_a_pin_is_refused(self):
        payloads = target_payloads("x86_64-unknown-linux-gnu", b"ELF engine")
        with self.assertRaisesRegex(ValueError, "No SHA-256 pinned"):
            stage_engine.stage(
                "x86_64-unknown-linux-gnu", self.out, TAG, {}, fetch=fake_fetch(payloads)
            )

    def test_helper_without_a_pin_is_refused(self):
        payloads = target_payloads("x86_64-unknown-linux-gnu", b"ELF engine")
        with self.assertRaisesRegex(ValueError, "No SHA-256 pinned"):
            stage_engine.stage(
                "x86_64-unknown-linux-gnu",
                self.out,
                TAG,
                {"codex-x86_64-unknown-linux-musl.tar.gz": digest_of(payloads["codex-x86_64-unknown-linux-musl.tar.gz"])},
                fetch=fake_fetch(payloads),
            )
        self.assertFalse((self.out / "engine").exists())

    def test_symlink_member_is_not_an_engine(self):
        payloads = target_payloads(
            "x86_64-unknown-linux-gnu",
            engine_tarball=tarball({"codex-x86_64-unknown-linux-musl": None}),
        )
        with self.assertRaisesRegex(ValueError, "Expected exactly one"):
            stage_engine.stage(
                "x86_64-unknown-linux-gnu",
                self.out,
                TAG,
                pins_for_payloads(payloads),
                fetch=fake_fetch(payloads),
            )

    def test_restaging_replaces_a_previous_target(self):
        windows = target_payloads("x86_64-pc-windows-msvc", b"MZ")
        stage_engine.stage(
            "x86_64-pc-windows-msvc",
            self.out,
            "rust-v0.150.0",
            pins_for_payloads(windows),
            fetch=fake_fetch(windows),
        )
        mac = target_payloads("aarch64-apple-darwin", b"Mach-O")
        stage_engine.stage(
            "aarch64-apple-darwin",
            self.out,
            TAG,
            pins_for_payloads(mac),
            fetch=fake_fetch(mac),
        )
        self.assertEqual([p.name for p in (self.out / "engine").iterdir()], ["0.153.4"])
        self.assertEqual(
            [p.name for p in (self.out / "engine/0.153.4").iterdir() if p.name.startswith("pidash")],
            ["pidash-agent-engine"],
        )
        self.assertFalse((self.out / "engine.wxi").exists())

    def test_replaced_upstream_asset_is_refused(self):
        # Upstream metadata and payload agree with each other, but not with
        # the digest pinned in the repo: an asset swapped upstream.
        original = target_payloads("x86_64-unknown-linux-gnu", b"ELF engine")
        replaced = target_payloads("x86_64-unknown-linux-gnu", b"ELF evil")
        with self.assertRaisesRegex(ValueError, "pins"):
            stage_engine.stage(
                "x86_64-unknown-linux-gnu",
                self.out,
                TAG,
                pins_for_payloads(original),
                fetch=fake_fetch(replaced),
            )
        self.assertFalse((self.out / "engine").exists())

    def test_missing_digest_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "Missing SHA-256"):
            stage_engine.release_digest({"assets": [{"name": "x", "digest": None}]}, "x")


if __name__ == "__main__":
    unittest.main()
