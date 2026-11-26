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
    if: ${{ !cancelled() && needs.scan.result == 'success' }}
    permissions:
      contents: write
    runs-on: ubuntu-22.04
    steps:
      - run: python3 gate.py verify-manifest candidate --expect-digest "$D"
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
        bad = self.GOOD.replace("      - run: python3 gate.py verify-manifest candidate --expect-digest \"$D\"\n", "")
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



# --------------------------------------------------------------------------
# Rework after the first manual review: each class pins one finding.
# --------------------------------------------------------------------------


class ScannerCoversFirstPartyBundles(unittest.TestCase):
    """M1: web bundles under usr/share/ and lib/ are first-party code."""

    def test_key_in_an_nginx_bundle_is_found(self):
        for rel in ("usr/share/nginx/html/assets/index.js", "lib/app/server.js", "usr/local/lib/app/main.js"):
            with self.subTest(rel=rel):
                layer = io.BytesIO()
                with tarfile.open(fileobj=layer, mode="w:gz") as tar:
                    data = f"const k='{SYNTHETIC_AWS}';".encode()
                    info = tarfile.TarInfo(rel)
                    info.size = len(data)
                    tar.addfile(info, io.BytesIO(data))
                findings = []
                gate._scan_bytes(layer.getvalue(), "blobs/sha256/x", [], [], findings)
                self.assertEqual([f.rule for f in findings], ["aws-access-key"])

    def test_real_package_trees_are_still_skipped(self):
        for rel in ("app/node_modules/pkg/test.pem", "usr/lib/python3/site-packages/pkg/key.pem"):
            with self.subTest(rel=rel):
                findings = []
                gate._scan_bytes(SYNTHETIC_KEY.encode(), rel, [], [], findings)
                self.assertEqual(findings, [])


class PolicyCheckerRunsFromTheBase(unittest.TestCase):
    """M2: the policy check cannot run code the pull request supplies."""

    def setUp(self):
        self.wf = _workflow((REPO / ".github/workflows/publication-gate.yml").read_text())

    def _runs(self, job):
        return [s.get("run", "") or "" for s in self.wf["jobs"][job]["steps"]]

    def test_base_checker_runs_isolated_from_its_own_checkout(self):
        job = next(j for j, v in self.wf["jobs"].items() if v.get("name") == "publication gate policy")
        steps = self.wf["jobs"][job]["steps"]
        checkouts = [s for s in steps if str(s.get("uses", "")).startswith("actions/checkout")]
        base = [s for s in checkouts if "base" in str((s.get("with") or {}).get("ref", ""))]
        self.assertTrue(base, "the base ref is checked out")
        base_path = base[0]["with"].get("path")
        self.assertTrue(base_path and base_path not in (".", ""), "into its own directory")
        runs = "\n".join(self._runs(job))
        self.assertIn(f"python -I {base_path}/.github/publication-gate/gate.py check-policy", runs)
        self.assertNotIn("gate.py.base.py", runs)
        # No pull request code runs in the job before the base checker.
        self.assertNotIn("unittest", runs)

    def test_check_policy_accepts_a_repo_root(self):
        self.assertEqual(gate.main(["check-policy", "--repo-root", str(REPO)]), 0)

    def test_codeowners_cover_the_gate(self):
        owners = (REPO / "CODEOWNERS").read_text()
        paths = [line.split()[0] for line in owners.splitlines() if line.strip() and not line.startswith("#")]
        for path in ("/.github/publication-gate/", "/.github/workflows/", "/.github/actions/build-image/", "/CODEOWNERS"):
            self.assertIn(path, paths)
        self.assertNotIn("/.github/actions/build-push/", paths)


class EveryWorkflowIsChecked(unittest.TestCase):
    """M3: a new workflow cannot publish just by having a new name."""

    ROGUE = """
name: rogue
on:
  pull_request_target:
permissions: write-all
jobs:
  ship:
    runs-on: ubuntu-22.04
    steps:
      - uses: docker/login-action@v3
        with:
          username: ${{ secrets.DOCKERHUB_USERNAME }}
          password: ${{ secrets.DOCKERHUB_TOKEN }}
      - run: docker push airepublic/x:latest
      - run: gh release upload v1 x.tar
"""

    def _repo_with(self, name, text):
        root = pathlib.Path(tempfile.mkdtemp())
        (root / ".github" / "workflows").mkdir(parents=True)
        (root / ".github" / "publication-gate").mkdir(parents=True)
        (root / gate.RULESET_PATH).write_text((REPO / gate.RULESET_PATH).read_text())
        (root / ".github" / "workflows" / name).write_text(text)
        return root

    def test_rogue_workflow_fails_every_check(self):
        problems = gate.check_policy(self._repo_with("rogue.yml", self.ROGUE))
        text = "\n".join(problems)
        for needle in ("pull_request_target", "write-all", "docker/login-action", "DOCKERHUB_TOKEN", "docker push", "gh release"):
            with self.subTest(needle=needle):
                self.assertIn(needle, text)

    def test_workflow_run_trigger_is_rejected(self):
        wf = "on:\n  workflow_run:\n    workflows: [x]\npermissions:\n  contents: read\njobs: {}\n"
        self.assertTrue(any("workflow_run" in p for p in gate.check_policy(self._repo_with("w.yml", wf))))

    def test_publisher_environment_only_in_publishing_workflows(self):
        wf = (
            "on: workflow_dispatch\npermissions:\n  contents: read\njobs:\n  p:\n"
            "    environment: release-publisher\n    runs-on: x\n    steps:\n      - run: echo\n"
        )
        self.assertTrue(any("release-publisher" in p for p in gate.check_policy(self._repo_with("w.yml", wf))))

    def test_ordinary_ci_workflow_passes(self):
        wf = (
            "on:\n  pull_request:\njobs:\n  t:\n    runs-on: x\n    permissions:\n      security-events: write\n"
            "    steps:\n      - run: make test\n        env:\n          DB: ${{ secrets.TEST_DATABASE_URL }}\n"
        )
        self.assertEqual(gate.check_policy(self._repo_with("ci.yml", wf)), [])


class SymlinksAreRefused(unittest.TestCase):
    """M4: a symlink cannot smuggle an outside file past the scan."""

    def setUp(self):
        self.outside = pathlib.Path(tempfile.mkdtemp()) / "secret.env"
        self.outside.write_text(f"KEY={SYNTHETIC_AWS}\n")
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        (self.tmp / "notes.md").write_text("ok\n")
        (self.tmp / "config.env").symlink_to(self.outside)

    def test_scan_rejects_a_symlink(self):
        findings = gate.scan_paths([self.tmp], denylist=[], allowlist=[])
        self.assertTrue(any(f.rule == "symlink" for f in findings))

    def test_scan_rejects_a_symlinked_directory(self):
        (self.tmp / "config.env").unlink()
        (self.tmp / "dir").symlink_to(self.outside.parent, target_is_directory=True)
        self.assertTrue(any(f.rule == "symlink" for f in gate.scan_paths([self.tmp], denylist=[], allowlist=[])))

    def test_manifest_refuses_a_symlink(self):
        with self.assertRaises(ValueError):
            gate.build_manifest(self.tmp, tag="v1", sha="a" * 40)

    def test_verify_refuses_a_symlink_added_after_the_scan(self):
        (self.tmp / "config.env").unlink()
        manifest = gate.build_manifest(self.tmp, tag="v1", sha="a" * 40)
        (self.tmp / "config.env").symlink_to(self.outside)
        self.assertTrue(any("symlink" in p for p in gate.verify_manifest(self.tmp, manifest)))

    def test_verify_refuses_a_file_swapped_for_a_symlink(self):
        (self.tmp / "config.env").unlink()
        (self.tmp / "config.env").write_text(self.outside.read_text())
        manifest = gate.build_manifest(self.tmp, tag="v1", sha="a" * 40)
        (self.tmp / "config.env").unlink()
        (self.tmp / "config.env").symlink_to(self.outside)
        self.assertTrue(any("symlink" in p for p in gate.verify_manifest(self.tmp, manifest)))


class SecretExpressions(unittest.TestCase):
    """S1: every way an expression can read secrets is caught."""

    def problems(self, text):
        return gate.policy_problems("wf.yml", _workflow(text), REPO)

    def test_whole_secrets_context_is_rejected(self):
        for expr in ("${{ toJSON(secrets) }}", "${{ toJson( secrets ) }}", "${{ format('{0}', secrets) }}"):
            with self.subTest(expr=expr):
                bad = WorkflowPolicyCatchesViolations.GOOD.replace(
                    "      - run: make\n", f"      - run: make\n        env:\n          ALL: \"{expr}\"\n"
                )
                self.assertTrue(any("secret" in p for p in self.problems(bad)))

    def test_secret_in_an_if_is_checked(self):
        bad = WorkflowPolicyCatchesViolations.GOOD.replace(
            "      - run: make\n", "      - run: make\n        if: secrets.DOCKERHUB_TOKEN != ''\n"
        )
        self.assertTrue(any("DOCKERHUB_TOKEN" in p for p in self.problems(bad)))


class PublisherOrdering(unittest.TestCase):
    """S2: the publisher runs only after a successful scan, verifying first."""

    GOOD = WorkflowPolicyCatchesViolations.GOOD

    def problems(self, text):
        return gate.policy_problems("wf.yml", _workflow(text), REPO)

    def test_good_publisher_passes(self):
        self.assertEqual(self.problems(self.GOOD), [])

    def test_publisher_if_must_require_scan_success(self):
        for cond in ("", "    if: ${{ always() }}\n", "    if: ${{ always() && needs.scan.result == 'success' }}\n",
                     "    if: ${{ needs.scan.result == 'success' || true }}\n"):
            with self.subTest(cond=cond):
                bad = self.GOOD.replace("    if: ${{ !cancelled() && needs.scan.result == 'success' }}\n", cond)
                self.assertTrue(self.problems(bad))

    def test_action_before_verify_is_rejected(self):
        for uses in ("docker/login-action@v3", "some/other-action@v1"):
            with self.subTest(uses=uses):
                bad = self.GOOD.replace(
                    "      - run: python3 gate.py verify-manifest",
                    f"      - uses: {uses}\n      - run: python3 gate.py verify-manifest",
                )
                self.assertTrue(any("before verify-manifest" in p for p in self.problems(bad)))

    def test_publisher_must_pass_an_expected_digest(self):
        bad = self.GOOD.replace(" --expect-digest \"$D\"", "")
        self.assertTrue(any("expect-digest" in p for p in self.problems(bad)))

    def test_job_level_secret_reaches_steps_before_verify(self):
        # Job env is visible to every step, including those before the check.
        bad = self.GOOD.replace(
            "    environment: release-publisher\n",
            "    environment: release-publisher\n    env:\n      T: ${{ secrets.DOCKERHUB_TOKEN }}\n",
        )
        self.assertTrue(any("before verify-manifest" in p for p in self.problems(bad)))

    def test_verify_step_cannot_be_skipped_or_allowed_to_fail(self):
        verify = "      - run: python3 gate.py verify-manifest candidate --expect-digest \"$D\"\n"
        for extra in ("        continue-on-error: true\n", "        if: ${{ false }}\n"):
            with self.subTest(extra=extra):
                bad = self.GOOD.replace(verify, verify + extra)
                self.assertTrue(any("verify-manifest" in p for p in self.problems(bad)))
        bad = self.GOOD.replace("    environment: release-publisher\n",
                                "    environment: release-publisher\n    continue-on-error: true\n")
        self.assertTrue(any("continue-on-error" in p for p in self.problems(bad)))


class MorePublishingCommands(unittest.TestCase):
    """S3: other ways to publish are caught outside the publisher."""

    def test_each_command_is_rejected(self):
        cmds = (
            "docker buildx build --push -t x .",
            "docker buildx imagetools create -t x y",
            "gh api -X POST repos/o/r/releases",
            "curl -X POST https://uploads.github.com/repos/o/r/releases/1/assets",
            "oras push ghcr.io/o/x:1 f",
            "aws s3 cp f s3://bucket/f",
        )
        for cmd in cmds:
            with self.subTest(cmd=cmd):
                bad = WorkflowPolicyCatchesViolations.GOOD.replace("      - run: make\n", f"      - run: {cmd}\n")
                self.assertTrue(gate.policy_problems("wf.yml", _workflow(bad), REPO))

    def test_push_by_expression_is_rejected(self):
        bad = WorkflowPolicyCatchesViolations.GOOD.replace(
            "      - run: make\n", "      - uses: docker/build-push-action@0000000000000000000000000000000000000000\n        with:\n          push: ${{ inputs.push }}\n"
        )
        self.assertTrue(any("pushes an image" in p for p in gate.policy_problems("wf.yml", _workflow(bad), REPO)))


class ExpectDigestIsRequired(unittest.TestCase):
    """S4: verify-manifest always checks against an approved digest."""

    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        (self.tmp / "a.txt").write_text("a\n")
        gate.main(["build-manifest", str(self.tmp), "--tag", "v1", "--sha", "a" * 40])
        self.digest = gate.manifest_digest(json.loads((self.tmp / gate.MANIFEST_NAME).read_text()))

    def test_missing_or_empty_digest_fails(self):
        self.assertNotEqual(gate.main(["verify-manifest", str(self.tmp)]), 0)
        self.assertNotEqual(gate.main(["verify-manifest", str(self.tmp), "--expect-digest", ""]), 0)

    def test_matching_digest_passes(self):
        self.assertEqual(gate.main(["verify-manifest", str(self.tmp), "--expect-digest", self.digest]), 0)


class DenylistReports(unittest.TestCase):
    """S5, S7: denylist findings reveal nothing about the marker."""

    def test_finding_has_no_marker_hash(self):
        import hashlib

        tmp = pathlib.Path(tempfile.mkdtemp())
        (tmp / "a.txt").write_text(f"one\ntwo {SYNTHETIC_MARKER}\n")
        findings = gate.scan_paths([tmp], denylist=[SYNTHETIC_MARKER], allowlist=[])
        report = "\n".join(str(f) for f in findings)
        self.assertIn("line 2", report)
        for salt in (b"denylist\0", b""):
            self.assertNotIn(hashlib.sha256(salt + SYNTHETIC_MARKER.lower().encode()).hexdigest()[:12], report)

    def test_marker_in_a_member_name_is_found(self):
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w") as tar:
            info = tarfile.TarInfo(f"srv/{SYNTHETIC_MARKER}/readme.txt")
            info.size = 2
            tar.addfile(info, io.BytesIO(b"ok"))
        findings = []
        gate._scan_bytes(buf.getvalue(), "image.tar", [SYNTHETIC_MARKER], [], findings)
        self.assertTrue(any(f.rule == "denylist" for f in findings))

    def test_marker_in_a_file_path_is_found(self):
        tmp = pathlib.Path(tempfile.mkdtemp())
        (tmp / f"{SYNTHETIC_MARKER}.txt").write_text("ok\n")
        self.assertTrue(gate.scan_paths([tmp], denylist=[SYNTHETIC_MARKER], allowlist=[]))


class UnknownCompression(unittest.TestCase):
    """S6: a compressed blob the scanner cannot open is not clean."""

    def test_zstd_and_other_formats_are_unreadable(self):
        for magic in (b"\x28\xb5\x2f\xfd", b"\x04\x22\x4d\x18", b"7z\xbc\xaf\x27\x1c", b"Rar!\x1a\x07", b"\x1f\x9d", b"LZIP"):
            with self.subTest(magic=magic):
                findings = []
                gate._scan_bytes(magic + b"\x00" * 64, "blobs/sha256/y", [], [], findings)
                self.assertTrue(findings)


class PublishingWorkflowHygiene(unittest.TestCase):
    """S8-S12 on the real workflows and ruleset."""

    def _wf(self, name):
        return _workflow((REPO / ".github/workflows" / name).read_text())

    def test_denylist_secret_is_scoped_to_its_steps(self):
        for name in gate.PUBLISHING_WORKFLOWS:
            for job_name, job in self._wf(name)["jobs"].items():
                with self.subTest(workflow=name, job=job_name):
                    self.assertNotIn("PUBLICATION_DENYLIST", json.dumps(job.get("env") or {}))
                    for step in job.get("steps") or []:
                        if "PUBLICATION_DENYLIST" in json.dumps(step.get("env") or {}):
                            self.assertRegex(step.get("run", ""), r"gate\.py (self-test|scan)\b")

    def test_third_party_actions_are_pinned(self):
        self.assertEqual([p for p in gate.check_policy() if "pinned" in p], [])
        bad = WorkflowPolicyCatchesViolations.GOOD.replace("      - run: make\n", "      - uses: docker/setup-qemu-action@v3\n")
        self.assertTrue(any("pinned" in p for p in gate.policy_problems("release.yml", _workflow(bad), REPO)))

    def test_untrusted_expressions_stay_out_of_run(self):
        bad = WorkflowPolicyCatchesViolations.GOOD.replace(
            "      - run: make\n", "      - run: echo \"${{ github.event.inputs.releaseVersion }}\"\n"
        )
        self.assertTrue(any("expression" in p for p in gate.policy_problems("release.yml", _workflow(bad), REPO)))

    def test_platform_tag_is_created_before_images_and_floating_tags_last(self):
        steps = self._wf("build-branch.yml")["jobs"]["publish"]["steps"]
        runs = [s.get("run", "") or "" for s in steps]
        tag_at = next(i for i, r in enumerate(runs) if "/git/refs" in r)
        push_at = next(i for i, r in enumerate(runs) if "skopeo copy" in r)
        self.assertLess(tag_at, push_at)
        push = runs[push_at]
        self.assertLess(push.index('for tag in'), push.index('for archive in'))

    def test_ruleset_protects_every_tag(self):
        ruleset = json.loads((REPO / gate.RULESET_PATH).read_text())
        for ref in ("refs/tags/rust-platform-v1", "refs/tags/anything", "refs/tags/a/b"):
            with self.subTest(ref=ref):
                self.assertTrue(gate.ruleset_covers(ruleset, ref))
        narrowed = json.loads(json.dumps(ruleset))
        narrowed["conditions"]["ref_name"]["include"] = ["refs/tags/v*"]
        self.assertTrue(gate.ruleset_problems(narrowed))

if __name__ == "__main__":
    unittest.main()
