# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Overlay layering contract for the bundled desktop frontend.

`merge-web-tree.sh` is the single assembler for the desktop web tree: bundled
dev (`dev-prep.sh`), the desktop tests (`test-overlay.sh`) and every edition's
release all go through it. The order it applies layers in is what lets an
edition and the desktop overlay coexist:

    workspace  ->  $PIDASH_DESKTOP_EXTRA_OVERLAYS  ->  desktop-overlay/  ->  args

The desktop layer is applied last on purpose — it has to *remove* web-only
routes, not merely add files — which means an edition can never override a
desktop file directly and must reach the desktop through the `ce/` seams
instead. Reverse the two and an edition's marketing home page and route list
leak back into the shipped app.

These run the real script against a synthetic checkout: the script derives its
own `OSS_DIR` from its location, so copying it into a temp tree exercises the
shipped code without rsyncing this repo.
"""

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/merge-web-tree.sh"


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


class MergeWebTreeTests(unittest.TestCase):
    def setUp(self):
        if not shutil.which("rsync"):
            self.skipTest("rsync is required by merge-web-tree.sh")
        self.tmp = Path(tempfile.mkdtemp(prefix="merge-web-tree-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)

        # A synthetic OSS checkout. The script resolves OSS_DIR as the parent of
        # the directory holding it, so this copy is rooted at self.tmp.
        self.script = self.tmp / "desktop/scripts/merge-web-tree.sh"
        self.script.parent.mkdir(parents=True)
        shutil.copy2(SCRIPT, self.script)

        # merge-web-tree.sh refuses to finish without the web app.
        write(self.tmp / "apps/web/package.json", '{"name":"web"}\n')
        write(self.tmp / "apps/web/base-only.txt", "workspace\n")

        # The desktop layer, as shipped.
        write(self.tmp / "desktop-overlay/apps/web/contested.txt", "desktop\n")
        write(self.tmp / "desktop-overlay/README.md", "documents the overlay\n")

        # Stand-in for an edition overlay (private ee-overlay/).
        write(self.tmp / "edition/apps/web/contested.txt", "edition\n")
        write(self.tmp / "edition/apps/web/seam.txt", "edition\n")
        write(self.tmp / "edition/README.md", "documents the edition\n")

        self.dest = self.tmp / "out"

    def merge(self, *args, extras=None):
        env = {"PATH": "/usr/bin:/bin:/usr/local/bin"}
        if extras is not None:
            env["PIDASH_DESKTOP_EXTRA_OVERLAYS"] = extras
        return subprocess.run(
            ["bash", str(self.script), str(self.dest), *args],
            capture_output=True,
            text=True,
            env=env,
            check=True,
        )

    def test_desktop_overlay_wins_over_the_edition(self):
        self.merge(extras=str(self.tmp / "edition"))
        # Both layers ship apps/web/contested.txt; the desktop one is applied
        # last, so the shipped bundle gets desktop behaviour — this is what
        # keeps an edition's home page and route list out of the desktop app.
        self.assertEqual((self.dest / "apps/web/contested.txt").read_text(), "desktop\n")

    def test_edition_only_files_survive_the_desktop_layer(self):
        self.merge(extras=str(self.tmp / "edition"))
        # A path the desktop layer does not contain is untouched by it. This is
        # the seam mechanism: an edition replaces ce/components/desktop/* and
        # desktop-overlay/ imports those files rather than overriding them.
        self.assertEqual((self.dest / "apps/web/seam.txt").read_text(), "edition\n")
        self.assertEqual((self.dest / "apps/web/base-only.txt").read_text(), "workspace\n")

    def test_plain_build_needs_no_edition_layer(self):
        self.merge()
        self.assertEqual((self.dest / "apps/web/contested.txt").read_text(), "desktop\n")
        self.assertFalse((self.dest / "apps/web/seam.txt").exists())

    def test_trailing_arguments_are_applied_after_every_overlay(self):
        # How an edition adds its own test files without displacing a layer.
        write(self.tmp / "extra-tests/apps/web/contested.txt", "last\n")
        self.merge(str(self.tmp / "extra-tests"), extras=str(self.tmp / "edition"))
        self.assertEqual((self.dest / "apps/web/contested.txt").read_text(), "last\n")

    def test_overlay_readmes_do_not_replace_the_workspace_readme(self):
        write(self.tmp / "README.md", "the workspace\n")
        self.merge(extras=str(self.tmp / "edition"))
        self.assertEqual((self.dest / "README.md").read_text(), "the workspace\n")

    def test_a_missing_overlay_fails_loudly(self):
        # Silently skipping a missing edition overlay would ship a community
        # bundle under an edition's name.
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.merge(extras=str(self.tmp / "no-such-overlay"))
        self.assertIn("overlay is not a directory", caught.exception.stderr)

    def test_the_destination_never_contains_the_desktop_sources(self):
        self.merge(extras=str(self.tmp / "edition"))
        # desktop/ holds the default destination itself, and desktop-overlay/ is
        # a source layer — copying either in would recurse or ship the overlay
        # twice.
        self.assertFalse((self.dest / "desktop").exists())
        self.assertFalse((self.dest / "desktop-overlay").exists())


if __name__ == "__main__":
    unittest.main()
