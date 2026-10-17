// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor extension sets (UI kit and editor page). Tiptap arrives only
// through dynamic import() (Stack: heavy dependencies never join the
// eager closure), so each surface exposes an async loader. The
// description set below covers the detail core (ISS-147); the slices
// that own mentions, uploads, task lists and highlighting (ISS-127,
// ISS-197) extend it with their own loaders. The compatibility
// fixtures (compat.test.ts) load this same set, so what the tests pin
// is exactly what the editor runs.

/** Description editing: paragraphs, headings, lists, code, links. */
export async function loadDescriptionExtensions() {
  const [{ default: StarterKit }, { default: Link }] = await Promise.all([
    import("@tiptap/starter-kit"),
    import("@tiptap/extension-link"),
  ]);
  // Links stay inert while editing: a click moves the caret instead of
  // navigating away mid-edit. Serialized links keep the extension's
  // default target/rel (pinned in compat.test.ts).
  return [StarterKit, Link.configure({ openOnClick: false })];
}
