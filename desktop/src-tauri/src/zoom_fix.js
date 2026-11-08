// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
//
// Ctrl+NumpadPlus zoom-in on Linux and Windows (PIDESKAPP-51).
//
// The `CmdOrCtrl+NumpadPlus` menu accelerator can never fire: muda parses
// it to `Code::NumpadAdd` and then immediately collapses that to
// `Key::Character("+")` (see `code_to_key` in muda's `accelerator.rs`),
// which the GTK backend turns into the main-row `+` keyval (0x2B) and the
// Windows backend into `VK_OEM_PLUS`. The physical numpad key (GTK
// `KP_Add` 0xFFAB / Windows `VK_ADD`) matches neither, so the stroke
// falls through to the page as a plain `+` keydown and nothing zooms.
//
// Catch the stroke in the page instead: `KeyboardEvent.code` is
// `NumpadAdd` for exactly that physical key, and the page is where the
// stroke already arrives. The zoom level itself stays in Rust
// (`ZoomState`, stepped by the View menu); this handler only asks it to
// step up once, so the two paths can never desync. Loaded on Linux and
// Windows only (see main.rs) — macOS keeps `Cmd+=`.

(() => {
  if (window.__PIDASH_ZOOM_FIX__) return;
  window.__PIDASH_ZOOM_FIX__ = true;
  document.addEventListener(
    "keydown",
    (e) => {
      if (e.code !== "NumpadAdd") return;
      if (!e.ctrlKey && !e.metaKey) return;
      if (e.altKey) return;
      e.preventDefault();
      e.stopPropagation();
      window.__TAURI__?.core?.invoke("zoom_in");
    },
    true
  );
})();
