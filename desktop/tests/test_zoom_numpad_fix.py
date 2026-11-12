# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Ctrl+NumpadPlus zoom-in on Linux and Windows (PIDESKAPP-51).

The `CmdOrCtrl+NumpadPlus` menu accelerator can never fire: muda parses
it to `Code::NumpadAdd` and then immediately collapses that to
`Key::Character("+")`, which the GTK backend turns into the main-row `+`
keyval (0x2B) and the Windows backend into `VK_OEM_PLUS`. The physical
numpad key (GTK `KP_Add` / Windows `VK_ADD`) therefore matches nothing,
falls through to the page as a plain `+` keydown, and never zooms.

`desktop/src-tauri/src/zoom_fix.js` catches the stroke in the page —
`KeyboardEvent.code` is `NumpadAdd` for exactly that key — and routes it
to the Rust `ZoomState` via the `zoom_in` command, so the page path and
the View-menu path share one zoom level. Loaded on Linux and Windows
only; the dead menu item it replaces must stay gone.

The behavioral half runs the script in Node against a stubbed document
and Tauri global and checks the full routing table: Ctrl+NumpadPlus
fires one `zoom_in` invoke with preventDefault + stopPropagation, while
plain NumpadPlus, Ctrl+=, Ctrl+NumpadSubtract, Ctrl+Alt+NumpadPlus, and
unrelated keys are all left alone.
"""

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

DESKTOP_DIR = Path(__file__).resolve().parents[1]
SCRIPT = DESKTOP_DIR / "src-tauri/src/zoom_fix.js"
MAIN = DESKTOP_DIR / "src-tauri/src/main.rs"

NODE_HARNESS = """
const fs = require('fs');
const script = fs.readFileSync(process.argv[2], 'utf8');
const listeners = [];
const invokes = [];
global.window = {
  __TAURI__: {
    core: {
      invoke: (command) => {
        invokes.push(command);
        return Promise.resolve();
      },
    },
  },
};
global.document = {
  addEventListener: (type, fn, capture) => {
    listeners.push({ type, fn, capture });
  },
};

function fail(message) {
  console.error('FAIL: ' + message);
  process.exit(1);
}

function fire() {
  if (listeners.length !== 1) {
    fail('expected exactly one listener, got ' + listeners.length);
  }
  const entry = listeners[0];
  if (entry.type !== 'keydown') fail('listener is not keydown: ' + entry.type);
  if (entry.capture !== true) fail('listener is not capture-phase');
  const event = {
    code: 'KeyZ',
    key: 'z',
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    prevented: false,
    stopped: false,
    preventDefault() { this.prevented = true; },
    stopPropagation() { this.stopped = true; },
  };
  Object.assign(event, arguments[0] || {});
  invokes.length = 0;
  entry.fn(event);
  return event;
}

function expectHandled(label, event) {
  if (invokes.length !== 1 || invokes[0] !== 'zoom_in') {
    fail(label + ': expected one zoom_in invoke, got ' + JSON.stringify(invokes));
  }
  if (!event.prevented) fail(label + ': keydown was not prevented');
  if (!event.stopped) fail(label + ': propagation was not stopped');
}

function expectIgnored(label, event) {
  if (invokes.length !== 0) {
    fail(label + ': unexpected invoke ' + JSON.stringify(invokes));
  }
  if (event.prevented) fail(label + ': keydown was prevented');
  if (event.stopped) fail(label + ': propagation was stopped');
}

eval(script);
if (window.__PIDASH_ZOOM_FIX__ !== true) fail('idempotency flag not set');
// Re-injection (every top-level navigation) must not stack listeners.
eval(script);
if (listeners.length !== 1) fail('re-injection registered a second listener');

expectHandled('ctrl+numpadplus', fire({ ctrlKey: true, code: 'NumpadAdd', key: '+' }));
expectHandled('meta+numpadplus', fire({ metaKey: true, code: 'NumpadAdd', key: '+' }));
expectIgnored('plain numpadplus types plus', fire({ code: 'NumpadAdd', key: '+' }));
expectIgnored('ctrl+equal is the menu accelerator', fire({ ctrlKey: true, code: 'Equal', key: '=' }));
expectIgnored('ctrl+numpadsubtract is out of scope', fire({ ctrlKey: true, code: 'NumpadSubtract', key: '-' }));
expectIgnored('ctrl+alt+numpadplus is left alone', fire({ ctrlKey: true, altKey: true, code: 'NumpadAdd', key: '+' }));
expectIgnored('ctrl+z untouched', fire({ ctrlKey: true, code: 'KeyZ', key: 'z' }));

// Without Tauri globals (e.g. dev preview) the key is still swallowed so
// no bogus `+` lands in a field, and nothing throws.
delete window.__TAURI__;
{
  const event = fire({ ctrlKey: true, code: 'NumpadAdd', key: '+' });
  if (invokes.length !== 0) fail('no-tauri: unexpected invoke ' + JSON.stringify(invokes));
  if (!event.prevented) fail('no-tauri: keydown was not prevented');
  if (!event.stopped) fail('no-tauri: propagation was not stopped');
}

console.log('ok');
"""


class ZoomNumpadFixStructuralTests(unittest.TestCase):
    def test_script_file_exists_and_routes_to_zoom_in(self):
        self.assertTrue(SCRIPT.is_file(), f"{SCRIPT} is missing")
        script = SCRIPT.read_text()
        for marker in (
            '"keydown"',
            "addEventListener",
            "NumpadAdd",
            "__TAURI__",
            "zoom_in",
            "preventDefault",
            "stopPropagation",
            "__PIDASH_ZOOM_FIX__",
        ):
            self.assertIn(marker, script, f"missing {marker!r} in {SCRIPT.name}")
        self.assertNotIn(
            "NumpadSubtract",
            script,
            "zoom-out is out of scope; the script must not grow a second route",
        )

    def test_main_wires_it_linux_windows_only(self):
        main = MAIN.read_text()
        self.assertIn('include_str!("zoom_fix.js")', main)
        lines = main.splitlines()
        idx = next(
            i
            for i, line in enumerate(lines)
            if "window = window.initialization_script(ZOOM_FIX);" in line
        )
        gate = lines[max(0, idx - 6) : idx]
        self.assertTrue(
            any('target_os = "linux"' in line for line in gate)
            and any('target_os = "windows"' in line for line in gate),
            "main.rs must load ZOOM_FIX under linux+windows gating",
        )

    def test_dead_menu_item_is_gone(self):
        # The CmdOrCtrl+NumpadPlus accelerator silently registers the
        # main-row plus (muda collapses NumpadAdd onto "+"), so the menu
        # item can never zoom and must not come back.
        main = MAIN.read_text()
        self.assertNotIn('.accelerator("CmdOrCtrl+NumpadPlus")', main)
        self.assertNotIn("zoom_in_numpad", main)

    def test_zoom_in_command_exists_and_is_registered(self):
        main = MAIN.read_text()
        self.assertIn("#[tauri::command]", main)
        lines = main.splitlines()
        cmd = next(
            i for i, line in enumerate(lines) if "fn zoom_in(" in line
        )
        above = lines[max(0, cmd - 2) : cmd]
        self.assertTrue(
            any("#[tauri::command]" in line for line in above),
            "zoom_in must be a #[tauri::command]",
        )
        handler = next(
            i for i, line in enumerate(lines) if "generate_handler!" in line
        )
        window = "\n".join(lines[handler : handler + 8])
        self.assertIn("zoom_in", window, "zoom_in must be in generate_handler!")


class ZoomNumpadFixBehaviorTests(unittest.TestCase):
    def test_handler_routing_table(self):
        node = shutil.which("node")
        if not node:
            self.skipTest("node is required to run the handler harness")
        with tempfile.TemporaryDirectory(prefix="zoom-numpad-fix-") as tmp:
            harness = Path(tmp) / "harness.js"
            harness.write_text(NODE_HARNESS)
            result = subprocess.run(
                [node, str(harness), str(SCRIPT)],
                capture_output=True,
                text=True,
                timeout=60,
            )
            self.assertEqual(result.returncode, 0, f"node said:\n{result.stdout}\n{result.stderr}")
            self.assertIn("ok", result.stdout)


if __name__ == "__main__":
    unittest.main()
