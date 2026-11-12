# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The macOS bundle declares the mic permissions chat dictation needs.

macOS TCC refuses microphone capture from an app whose bundle has no
`NSMicrophoneUsageDescription` in its Info.plist, and the hardened-runtime /
sandboxed builds additionally need the `com.apple.security.device.audio-input`
entitlement. Without them the push-to-talk mic in the chat composer can never
record: WKWebView exposes `navigator.mediaDevices.getUserMedia`, but the OS
blocks the capture (and may terminate the app for touching the mic without a
usage string).

The direct-download (cloud) build inherits these keys from the OSS base
config — Tauri deep-merges `--config` overlays, and `desktop/tauri.cloud.json`
only sets `hardenedRuntime` — so guarding the base config covers it. The Mac
App Store build overrides both paths with its own `appstore/` files (guarded
by `test_mac_app_store_workflow.py` in the private repo instead).
"""

import json
import plistlib
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SRC_TAURI = REPO / "desktop" / "src-tauri"
CONFIG = json.loads((SRC_TAURI / "tauri.conf.json").read_text())


class MacOsMicConfigTests(unittest.TestCase):
    def macos(self):
        return CONFIG["bundle"]["macOS"]

    def load_plist(self, config_key):
        path = SRC_TAURI / self.macos()[config_key]
        with path.open("rb") as handle:
            return plistlib.load(handle)

    def test_mic_usage_string_is_declared(self):
        self.assertIn("infoPlist", self.macos())
        plist = self.load_plist("infoPlist")
        self.assertTrue(
            plist.get("NSMicrophoneUsageDescription", "").strip(),
            "Info.plist must give a user-facing microphone usage string",
        )

    def test_audio_input_entitlement_is_declared(self):
        self.assertIn("entitlements", self.macos())
        entitlements = self.load_plist("entitlements")
        self.assertTrue(
            entitlements.get("com.apple.security.device.audio-input"),
            "entitlements must grant audio input for chat dictation",
        )

    def test_hardened_runtime_is_enabled(self):
        self.assertTrue(self.macos().get("hardenedRuntime"))

    def test_direct_build_is_not_sandboxed(self):
        entitlements = self.load_plist("entitlements")
        self.assertNotIn(
            "com.apple.security.app-sandbox",
            entitlements,
            "the direct (non-App-Store) build must stay unsandboxed",
        )


if __name__ == "__main__":
    unittest.main()
