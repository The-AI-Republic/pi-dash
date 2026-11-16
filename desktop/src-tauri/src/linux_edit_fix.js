// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
//
// Linux (WebKitGTK) undo/redo for plain text fields (PIDESKAPP-52).
//
// WebKitGTK leaves undo/redo to the embedder: Ctrl+Z / Ctrl+Shift+Z /
// Ctrl+Y reach the page but do nothing in an <input>/<textarea>, while
// cut/copy/paste/select-all — which WebKit implements itself — work, and
// Tauri neither supplies the editing commands nor an Edit menu on any
// platform. Drive the browser's own edit commands for plain fields only.
// Tiptap editors (the description editor) keep a ProseMirror history and
// their own Mod+Z keymap, so anything that is not an INPUT/TEXTAREA is
// left alone. Loaded on Linux only (see main.rs).

(() => {
  if (window.__PIDASH_LINUX_EDIT_FIX__) return;
  window.__PIDASH_LINUX_EDIT_FIX__ = true;
  document.addEventListener(
    "keydown",
    (e) => {
      if (!e.ctrlKey || e.metaKey || e.altKey) return;
      let command = null;
      if (e.code === "KeyZ") command = e.shiftKey ? "redo" : "undo";
      else if (e.code === "KeyY") command = "redo";
      if (!command) return;
      const tag = e.target && e.target.tagName;
      if (tag !== "TEXTAREA" && tag !== "INPUT") return;
      e.preventDefault();
      e.stopPropagation();
      document.execCommand(command);
    },
    true
  );
})();
