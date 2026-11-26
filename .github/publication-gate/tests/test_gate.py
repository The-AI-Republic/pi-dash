# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the publication gate.

The gate's promises, each pinned by a test below:

1. An unscanned ref or artifact cannot reach the public repo: only the
   protected publisher job may create refs, releases or pushed images, and it
   runs only after the scan job and only on the exact scanned files.
2. Ordinary identities cannot push in-scope refs: the tag ruleset covers every
   release tag pattern and only the publisher identity may bypass it.
3. Creating an approved public tag cannot trigger publication on its own: no
   workflow starts on a tag push, a ref creation or a release event.

Run with: python -m unittest discover -s .github/publication-gate/tests
"""

import base64
import gzip
import io
import json
import os
import pathlib
import sys
import tarfile
import tempfile
import unittest
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import gate  # noqa: E402

REPO = gate.REPO_ROOT


def _workflow(text):
    import yaml

    return yaml.safe_load(text)


class LiveWorkflowPolicy(unittest.TestCase):
    """The workflows in this checkout satisfy the gate."""

    def test_no_workflow_starts_on_tag_or_release_events(self):
        problems = []
        for path in sorted((REPO / ".github" / "workflows").glob("*.y*ml")):
            problems += gate.trigger_problems(path.name, gate.load_yaml(path))
        self.assertEqual(problems, [])

    def test_publishing_workflows_pass_the_gate(self):
        for name in gate.PUBLISHING_WORKFLOWS:
            with self.subTest(workflow=name):
                path = REPO / ".github" / "workflows" / name
                self.assertEqual(gate.workflow_problems(path), [])

    def test_whole_policy_passes(self):
        self.assertEqual(gate.check_policy(), [])


class WorkflowPolicyCatchesViolations(unittest.TestCase):
    """The checker rejects each way publication could bypass the gate."""

    GOOD = """
name: ok
on:
  workflow_dispatch:
permissions:
  contents: read
jobs:
  build:
    runs-on: ubuntu-22.04
    steps:
      - run: make
  scan:
    needs: [build]
    environment: publication-scan
    runs-on: ubuntu-22.04
    steps:
      - run: python3 gate.py scan --denylist-env PUBLICATION_DENYLIST candidate
        env:
          PUBLICATION_DENYLIST: ${{ secrets.PUBLICATION_DENYLIST }}
  publish:
    needs: [scan]
    environment: release-publisher
    permissions:
      contents: write
    runs-on: ubuntu-22.04
    steps:
      - run: python3 gate.py verify-manifest candidate
      - run: gh release create v1.0.0 candidate/*
        env:
          GH_TOKEN: ${{ secrets.PUBLISHER_TOKEN }}
"""

    def problems(self, text):
        return gate.policy_problems("wf.yml", _workflow(text), REPO)

    def test_good_workflow_passes(self):
        self.assertEqual(self.problems(self.GOOD), [])

    def test_tag_push_trigger_is_rejected(self):
        bad = self.GOOD.replace("  workflow_dispatch:\n", "  push:\n    tags: ['v*']\n")
        self.assertTrue(any("tag" in p for p in self.problems(bad)))

    def test_push_without_branch_filter_is_rejected(self):
        # A push trigger with no branch filter also fires on tag pushes
        # (path filters are not evaluated for tags).
        for trigger in ("  push:\n", "  push:\n    paths: ['src/**']\n"):
            with self.subTest(trigger=trigger):
                bad = self.GOOD.replace("  workflow_dispatch:\n", trigger)
                self.assertTrue(any("tag" in p for p in self.problems(bad)))
        ok = self.GOOD.replace("  workflow_dispatch:\n", "  push:\n    branches: [main]\n")
        self.assertFalse([p for p in self.problems(ok) if "tag" in p])

    def test_release_and_create_triggers_are_rejected(self):
        for event in ("release", "create"):
            with self.subTest(event=event):
                bad = self.GOOD.replace("  workflow_dispatch:\n", f"  {event}:\n")
                self.assertTrue(self.problems(bad))

    def test_publish_credential_outside_publisher_is_rejected(self):
        bad = self.GOOD.replace("      - run: make\n", "      - run: make\n        env:\n          T: ${{ secrets.DOCKERHUB_TOKEN }}\n")
        self.assertTrue(any("DOCKERHUB_TOKEN" in p for p in self.problems(bad)))

    def test_write_permission_outside_publisher_is_rejected(self):
        bad = self.GOOD.replace("permissions:\n  contents: read\n", "permissions:\n  contents: write\n", 1)
        self.assertTrue(self.problems(bad))

    def test_missing_top_level_permissions_is_rejected(self):
        bad = self.GOOD.replace("permissions:\n  contents: read\n", "", 1)
        self.assertTrue(self.problems(bad))

    def test_publishing_command_outside_publisher_is_rejected(self):
        for cmd in ("git push origin v1.0.0", "gh release create v1", "skopeo copy a b", "docker push x"):
            with self.subTest(cmd=cmd):
                bad = self.GOOD.replace("      - run: make\n", f"      - run: {cmd}\n")
                self.assertTrue(self.problems(bad))

    def test_read_only_git_commands_are_allowed(self):
        ok = self.GOOD.replace("      - run: make\n", "      - run: git tag --list 'v*' --merged HEAD\n")
        self.assertEqual(self.problems(ok), [])
        bad = self.GOOD.replace("      - run: make\n", "      - run: git tag v1.0.0\n")
        self.assertTrue(self.problems(bad))

    def test_publisher_without_scan_dependency_is_rejected(self):
        bad = self.GOOD.replace("    needs: [scan]\n", "    needs: [build]\n")
        self.assertTrue(any("scan" in p for p in self.problems(bad)))

    def test_publisher_without_manifest_check_is_rejected(self):
        bad = self.GOOD.replace("      - run: python3 gate.py verify-manifest candidate\n", "")
        self.assertTrue(any("verify-manifest" in p for p in self.problems(bad)))

    def test_publisher_without_protected_environment_is_rejected(self):
        bad = self.GOOD.replace("    environment: release-publisher\n", "")
        self.assertTrue(self.problems(bad))


class TagRuleset(unittest.TestCase):
    """Ordinary identities cannot create, move or delete in-scope tags."""

    def setUp(self):
        self.ruleset = json.loads((REPO / gate.RULESET_PATH).read_text())

    def test_committed_ruleset_passes(self):
        self.assertEqual(gate.ruleset_problems(self.ruleset), [])

    def test_every_release_tag_shape_is_covered(self):
        for ref in ("refs/tags/pidash-v1.2.3", "refs/tags/platform-v1.2.3", "refs/tags/v1.2.3", "refs/tags/v1.2.3-rc.1"):
            with self.subTest(ref=ref):
                self.assertTrue(gate.ruleset_covers(self.ruleset, ref))

    def test_ruleset_missing_a_rule_is_rejected(self):
        bad = json.loads(json.dumps(self.ruleset))
        bad["rules"] = [r for r in bad["rules"] if r["type"] != "creation"]
        self.assertTrue(gate.ruleset_problems(bad))

    def test_extra_bypass_actor_is_rejected(self):
        bad = json.loads(json.dumps(self.ruleset))
        bad["bypass_actors"].append({"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"})
        self.assertTrue(gate.ruleset_problems(bad))

    def test_evaluate_mode_is_rejected(self):
        bad = dict(self.ruleset, enforcement="evaluate")
        self.assertTrue(gate.ruleset_problems(bad))

    def test_live_ruleset_must_name_the_publisher_app(self):
        live = json.loads(json.dumps(self.ruleset))
        live["bypass_actors"][0]["actor_id"] = 42
        self.assertEqual(gate.live_ruleset_problems([live], app_id=42), [])
        self.assertTrue(gate.live_ruleset_problems([live], app_id=43))
        self.assertTrue(gate.live_ruleset_problems([], app_id=42))


def _fake(*parts):
    # Assemble look-alike secrets at runtime so this file holds no literal
    # that a scanner of this repository would flag.
    return "".join(parts)


SYNTHETIC_AWS = _fake("AKIA", "Z7Q2", "W9X4", "K1M3", "P8R5")
SYNTHETIC_GH = _fake("ghp", "_", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8s9")
SYNTHETIC_KEY = _fake("-----BEGIN ", "OPENSSH PRIVATE", " KEY-----\n", "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ")
KEY_HEADER_ONLY = _fake("-----BEGIN ", "RSA PRIVATE", " KEY-----")
SYNTHETIC_MARKER = _fake("not-for-", "public-", "canary-7f3a")


class Scanner(unittest.TestCase):
    """A synthetic leak anywhere in a candidate is rejected."""

    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        (self.tmp / "README.md").write_text("clean text\n")

    def scan(self, denylist=()):
        return gate.scan_paths([self.tmp], denylist=list(denylist), allowlist=[])

    def test_clean_candidate_passes(self):
        self.assertEqual(self.scan(), [])

    def test_plain_file_leaks_are_found(self):
        for leak in (SYNTHETIC_AWS, SYNTHETIC_GH, SYNTHETIC_KEY):
            with self.subTest(leak=leak[:6]):
                (self.tmp / "notes.txt").write_text(f"x = {leak}\n")
                self.assertTrue(self.scan())

    def test_denylist_marker_is_found(self):
        (self.tmp / "install.sh").write_text(f"echo {SYNTHETIC_MARKER}\n")
        self.assertEqual(self.scan(), [])
        self.assertTrue(self.scan(denylist=[SYNTHETIC_MARKER]))

    def test_leak_nested_in_archives_is_found(self):
        inner = io.BytesIO()
        with tarfile.open(fileobj=inner, mode="w:gz") as tar:
            data = f"token={SYNTHETIC_GH}\n".encode()
            info = tarfile.TarInfo("app/config.env")
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
        with zipfile.ZipFile(self.tmp / "pidash-x86_64-pc-windows-msvc.zip", "w") as zf:
            # A blob with no extension, as in an OCI layout.
            zf.writestr("blobs/sha256/abc", inner.getvalue())
        findings = self.scan()
        self.assertTrue(findings)
        self.assertIn("config.env", findings[0].location)

    def test_a_bare_key_header_is_not_a_key(self):
        # Compiled PEM parsers carry the header text without any key.
        (self.tmp / "pidash").write_bytes(b"\x7fELF" + KEY_HEADER_ONLY.encode() + b"\x00\x01")
        self.assertEqual(self.scan(), [])

    def test_leak_in_an_image_layer_is_found(self):
        # An OCI image archive: an uncompressed tar whose blobs are gzipped
        # layer tars with no file extension.
        layer = io.BytesIO()
        with tarfile.open(fileobj=layer, mode="w:gz") as tar:
            data = b"\0" * 100_000 + f"KEY={SYNTHETIC_AWS}".encode()
            info = tarfile.TarInfo("srv/app/settings.py")
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
        with tarfile.open(self.tmp / "pi-dash-backend.tar", "w") as oci:
            blob = layer.getvalue()
            info = tarfile.TarInfo("blobs/sha256/" + "0" * 64)
            info.size = len(blob)
            oci.addfile(info, io.BytesIO(blob))
        findings = self.scan()
        self.assertEqual([f.rule for f in findings], ["aws-access-key"])
        self.assertIn("srv/app/settings.py", findings[0].location)

    def test_leak_across_a_read_boundary_is_found(self):
        old = gate.CHUNK
        gate.CHUNK = 64  # the leak straddles reads after the 512-byte sniff
        try:
            (self.tmp / "big.txt").write_text("x" * 1000 + " " + SYNTHETIC_GH + "\n")
            self.assertTrue(self.scan())
        finally:
            gate.CHUNK = old

    def test_findings_never_echo_the_secret(self):
        (self.tmp / "notes.txt").write_text(f"x = {SYNTHETIC_GH}\n")
        report = "\n".join(str(f) for f in self.scan())
        self.assertNotIn(SYNTHETIC_GH, report)

    def test_third_party_paths_skip_generic_rules_but_not_the_denylist(self):
        dep = self.tmp / "node_modules" / "pkg"
        dep.mkdir(parents=True)
        (dep / "test.pem").write_text(SYNTHETIC_KEY + "\n")
        self.assertEqual(self.scan(), [])
        (dep / "test.pem").write_text(SYNTHETIC_MARKER + "\n")
        self.assertTrue(self.scan(denylist=[SYNTHETIC_MARKER]))

    def test_allowlist_entry_suppresses_one_path(self):
        (self.tmp / "fixture.py").write_text(SYNTHETIC_KEY + "\n")
        allow = gate.parse_allowlist("private-key fixture.py\n")
        self.assertEqual(gate.scan_paths([self.tmp], denylist=[], allowlist=allow), [])

    def test_self_test_proves_the_scanner_is_live(self):
        self.assertTrue(gate.self_test(denylist=[SYNTHETIC_MARKER]))

    def test_denylist_env_is_decoded(self):
        os.environ["PG_TEST_DENYLIST"] = base64.b64encode(f"# comment\n{SYNTHETIC_MARKER}\n".encode()).decode()
        try:
            self.assertEqual(gate.denylist_from_env("PG_TEST_DENYLIST"), [SYNTHETIC_MARKER])
        finally:
            del os.environ["PG_TEST_DENYLIST"]

    def test_scan_cli_fails_on_a_leak(self):
        (self.tmp / "notes.txt").write_text(f"x = {SYNTHETIC_AWS}\n")
        self.assertNotEqual(gate.main(["scan", str(self.tmp)]), 0)


class Manifest(unittest.TestCase):
    """The publisher publishes exactly the files that were scanned."""

    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        (self.tmp / "a.tar.xz").write_bytes(gzip.compress(b"a"))
        (self.tmp / "notes.md").write_text("release notes\n")
        self.manifest = gate.build_manifest(self.tmp, tag="pidash-v1.2.3", sha="a" * 40)

    def test_unchanged_candidate_verifies(self):
        self.assertEqual(gate.verify_manifest(self.tmp, self.manifest), [])

    def test_changed_file_is_rejected(self):
        (self.tmp / "notes.md").write_text("swapped after the scan\n")
        self.assertTrue(gate.verify_manifest(self.tmp, self.manifest))

    def test_added_file_is_rejected(self):
        (self.tmp / "extra.sh").write_text("echo hi\n")
        self.assertTrue(gate.verify_manifest(self.tmp, self.manifest))

    def test_removed_file_is_rejected(self):
        (self.tmp / "notes.md").unlink()
        self.assertTrue(gate.verify_manifest(self.tmp, self.manifest))

    def test_manifest_digest_pins_the_whole_set(self):
        other = gate.build_manifest(self.tmp, tag="pidash-v1.2.4", sha="a" * 40)
        self.assertNotEqual(gate.manifest_digest(self.manifest), gate.manifest_digest(other))


if __name__ == "__main__":
    unittest.main()
