"""Release metadata tests; run: python3 -m unittest discover -s .github/tests."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "release_tools" / "rust_release.py"
SPEC = importlib.util.spec_from_file_location("rust_release", SCRIPT)
RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE)


class RustReleaseTests(unittest.TestCase):
    def test_final_and_prerelease_versions(self):
        for suffix, expected in [("", "false"), ("-rc.1", "true"), ("-alpha-1", "true"), ("-0", "true")]:
            with self.subTest(suffix=suffix):
                result = RELEASE.metadata(f"rust-platform-v1.2.3{suffix}")
                self.assertEqual(result["version"], f"v1.2.3{suffix}")
                self.assertEqual(result["prerelease"], expected)

    def test_other_release_streams_and_invalid_tags_are_rejected(self):
        for tag in [
            "platform-v1.2.3", "pidash-v1.2.3", "v1.2.3", "rust-platform-v1.2",
            "rust-platform-v01.2.3", "rust-platform-v1.2.3-01", "rust-platform-v1.2.3-rc..1",
            "rust-platform-v1.2.3+build", "rust-platform-v1.2.3\nprerelease=false",
            "rust-platform-v1.2.3-" + "a" * 70,
        ]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                RELEASE.metadata(tag)

    def test_github_prerelease_never_promotes_stable(self):
        self.assertEqual(RELEASE.metadata("rust-platform-v1.2.3", True)["prerelease"], "true")
        self.assertEqual(RELEASE.metadata("rust-platform-v1.2.3-rc.1", False)["prerelease"], "true")

    def test_all_events_select_the_release_tag_not_dispatch_branch(self):
        cases = [
            ("push", {}, "rust-platform-v1.2.3", "false"),
            ("release", {"release": {"tag_name": "rust-platform-v1.2.3", "prerelease": True}}, "main", "true"),
            ("workflow_dispatch", {"inputs": {"tag": "rust-platform-v1.2.3"}}, "main", "false"),
        ]
        for kind, event, ref, prerelease in cases:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                event_path = Path(directory) / "event.json"
                output_path = Path(directory) / "output"
                event_path.write_text(json.dumps(event))
                result = subprocess.run(
                    ["python3", str(SCRIPT)],
                    env={**os.environ, "GITHUB_EVENT_NAME": kind, "GITHUB_EVENT_PATH": str(event_path),
                         "GITHUB_REF_NAME": ref, "GITHUB_OUTPUT": str(output_path)},
                    capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output_path.read_text(),
                                 f"tag=rust-platform-v1.2.3\nversion=v1.2.3\nprerelease={prerelease}\n")
