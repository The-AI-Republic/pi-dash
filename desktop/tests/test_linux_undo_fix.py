# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Linux undo/redo for plain text fields (PIDESKAPP-52).

On Linux the Tauri webview is WebKitGTK, which leaves undo/redo to the
embedder: Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y reach the page but do nothing in
an ``<input>``/``<textarea>``, while cut/copy/paste/select-all — which
WebKit implements itself — work. ``desktop/src-tauri/src/linux_edit_fix.js``
drives the browser's own edit commands for plain fields only, via a
Linux-gated ``initialization_script`` in ``main.rs``; Tiptap editors (the
description editor) keep a ProseMirror history and their own Mod+Z keymap
and must stay untouched.

The behavioral half runs the script in Node against a stubbed document and
checks the full routing table: undo/redo fire, with preventDefault and
stopPropagation, only for Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y in INPUT/TEXTAREA
targets — never for contenteditable/div targets, never for other keys
(Ctrl+C is WebKit's native clipboard path and must stay untouched), never
without Ctrl, and never with Meta or Alt held.
"""

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

DESKTOP_DIR = Path(__file__).resolve().parents[1]
SCRIPT = DESKTOP_DIR / "src-tauri/src/linux_edit_fix.js"
MAIN = DESKTOP_DIR / "src-tauri/src/main.rs"

NODE_HARNESS = """
const fs = require('fs');
const script = fs.readFileSync(process.argv[2], 'utf8');
const listeners = [];
const execCalls = [];
global.window = {};
let harnessDocument = {
  addEventListener: (type, fn, capture) => {
    listeners.push({ type, fn, capture });
  },
  execCommand: (command) => {
    execCalls.push(command);
    return true;
  },
};
global.document = harnessDocument;

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
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    target: { tagName: 'TEXTAREA' },
    prevented: false,
    stopped: false,
    preventDefault() { this.prevented = true; },
    stopPropagation() { this.stopped = true; },
  };
  Object.assign(event, arguments[0] || {});
  execCalls.length = 0;
  entry.fn(event);
  return event;
}

function expectHandled(label, event, command) {
  if (execCalls.length !== 1 || execCalls[0] !== command) {
    fail(label + ': expected execCommand(' + command + '), got ' + JSON.stringify(execCalls));
  }
  if (!event.prevented) fail(label + ': keydown was not prevented');
  if (!event.stopped) fail(label + ': propagation was not stopped');
}

function expectIgnored(label, event) {
  if (execCalls.length !== 0) {
    fail(label + ': unexpected execCommand ' + JSON.stringify(execCalls));
  }
  if (event.prevented) fail(label + ': keydown was prevented');
  if (event.stopped) fail(label + ': propagation was stopped');
}

eval(script);
if (window.__PIDASH_LINUX_EDIT_FIX__ !== true) fail('idempotency flag not set');
// Re-injection (every top-level navigation) must not stack listeners.
eval(script);
if (listeners.length !== 1) fail('re-injection registered a second listener');

expectHandled('ctrl+z textarea', fire({ ctrlKey: true, code: 'KeyZ', target: { tagName: 'TEXTAREA' } }), 'undo');
expectHandled('ctrl+shift+z textarea', fire({ ctrlKey: true, shiftKey: true, code: 'KeyZ', target: { tagName: 'TEXTAREA' } }), 'redo');
expectHandled('ctrl+z input', fire({ ctrlKey: true, code: 'KeyZ', target: { tagName: 'INPUT' } }), 'undo');
expectHandled('ctrl+y input', fire({ ctrlKey: true, code: 'KeyY', target: { tagName: 'INPUT' } }), 'redo');
expectHandled('ctrl+shift+y textarea', fire({ ctrlKey: true, shiftKey: true, code: 'KeyY', target: { tagName: 'TEXTAREA' } }), 'redo');
expectIgnored('ctrl+z tiptap div', fire({ ctrlKey: true, code: 'KeyZ', target: { tagName: 'DIV' } }));
expectIgnored('ctrl+y tiptap div', fire({ ctrlKey: true, code: 'KeyY', target: { tagName: 'DIV' } }));
expectIgnored('ctrl+z body', fire({ ctrlKey: true, code: 'KeyZ', target: { tagName: 'BODY' } }));
expectIgnored('ctrl+c textarea', fire({ ctrlKey: true, code: 'KeyC', target: { tagName: 'TEXTAREA' } }));
expectIgnored('plain z textarea', fire({ code: 'KeyZ', target: { tagName: 'TEXTAREA' } }));
expectIgnored('meta+z textarea', fire({ metaKey: true, code: 'KeyZ', target: { tagName: 'TEXTAREA' } }));
expectIgnored('ctrl+meta+z textarea', fire({ ctrlKey: true, metaKey: true, code: 'KeyZ', target: { tagName: 'TEXTAREA' } }));
expectIgnored('ctrl+alt+z textarea', fire({ ctrlKey: true, altKey: true, code: 'KeyZ', target: { tagName: 'TEXTAREA' } }));

console.log('ok');
"""


class LinuxUndoFixStructuralTests(unittest.TestCase):
    def test_script_file_exists_and_routes_both_directions(self):
        self.assertTrue(SCRIPT.is_file(), f"{SCRIPT} is missing")
        script = SCRIPT.read_text()
        for marker in (
            '"keydown"',
            "addEventListener",
            "TEXTAREA",
            "INPUT",
            "execCommand",
            '"undo"',
            '"redo"',
            "KeyZ",
            "KeyY",
            "preventDefault",
            "stopPropagation",
            "__PIDASH_LINUX_EDIT_FIX__",
        ):
            self.assertIn(marker, script, f"missing {marker!r} in {SCRIPT.name}")

    def test_main_wires_it_linux_only(self):
        main = MAIN.read_text()
        self.assertIn('include_str!("linux_edit_fix.js")', main)
        lines = main.splitlines()
        idx = next(
            i
            for i, line in enumerate(lines)
            if "window = window.initialization_script(LINUX_EDIT_FIX);" in line
        )
        gate = lines[max(0, idx - 4) : idx]
        self.assertTrue(
            any('#[cfg(target_os = "linux")]' in line for line in gate),
            "main.rs must load LINUX_EDIT_FIX under #[cfg(target_os = \"linux\")]",
        )


class LinuxUndoFixBehaviorTests(unittest.TestCase):
    def test_handler_routing_table(self):
        node = shutil.which("node")
        if not node:
            self.skipTest("node is required to run the handler harness")
        with tempfile.TemporaryDirectory(prefix="linux-undo-fix-") as tmp:
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
