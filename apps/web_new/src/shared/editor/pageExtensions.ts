// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page body extension set (parity/specs/pages.md §10.1, PAGE-028–032).
// The page editor starts from the description set and adds what a page
// body stores beyond an issue description: underline, paragraph/heading
// alignment, and checkable task lists. The loaders stay dynamic so
// every Tiptap module arrives in the editor's own chunk, never in the
// initial closure. Later slices extend this same set — tables, images
// and colours (PAGE-033–036), then blocks/embeds (PAGE-037–044).

import { loadDescriptionExtensions } from "./extensions.js";

/** Tiptap's extension type, named without a static import of the package. */
type AnyExtension = import("@tiptap/core").AnyExtension;

/**
 * Page editing: the description set plus underline, alignment and task
 * lists. The set is typed as Tiptap's own extension array because the
 * description loader's inferred pair is narrower than what a page adds.
 */
export async function loadPageExtensions(): Promise<AnyExtension[]> {
  const [{ default: Underline }, { default: TextAlign }, { default: TaskList }, { default: TaskItem }] =
    await Promise.all([
      import("@tiptap/extension-underline"),
      import("@tiptap/extension-text-align"),
      import("@tiptap/extension-task-list"),
      import("@tiptap/extension-task-item"),
    ]);
  const base = await loadDescriptionExtensions();
  return [
    ...base,
    Underline,
    TextAlign.configure({ alignments: ["left", "center", "right"], types: ["heading", "paragraph"] }),
    TaskList,
    // Nested checkable items: a sub-item keeps its own checkbox state.
    TaskItem.configure({ nested: true }),
  ];
}
