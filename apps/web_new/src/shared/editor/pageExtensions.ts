// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page body extension set (parity/specs/pages.md §10.1, PAGE-028–036).
// The page editor starts from the description set and adds what a page
// body stores beyond an issue description: underline, paragraph/heading
// alignment, checkable task lists, the table node family and the letter/
// wash colour mark. The loaders stay dynamic so every Tiptap module
// arrives in the editor's own chunk, never in the initial closure. Later
// slices extend this same set — blocks and embeds (PAGE-037–044).

import { loadDescriptionExtensions, loadMentionExtension, loadStoredImageExtension } from "./extensions.js";
import { loadPageBlockExtensions } from "./pageBlockExtensions.js";
import { loadPageColorMark, loadPageTableExtensions } from "./pageTableExtensions.js";

/** Tiptap's extension type, named without a static import of the package. */
type AnyExtension = import("@tiptap/core").AnyExtension;

/**
 * Page editing: the description set plus underline, alignment, task
 * lists, tables and colour. The set is typed as Tiptap's own extension
 * array because the description loader's inferred pair is narrower than
 * what a page adds.
 */
export async function loadPageExtensions(workspaceSlug = ""): Promise<AnyExtension[]> {
  const [
    { default: Underline },
    { default: TextAlign },
    { default: TaskList },
    { default: TaskItem },
    base,
    tables,
    color,
    image,
    blocks,
    mention,
  ] = await Promise.all([
    import("@tiptap/extension-underline"),
    import("@tiptap/extension-text-align"),
    import("@tiptap/extension-task-list"),
    import("@tiptap/extension-task-item"),
    loadDescriptionExtensions({ horizontalRule: false }),
    loadPageTableExtensions(),
    loadPageColorMark(),
    loadStoredImageExtension(),
    loadPageBlockExtensions(),
    loadMentionExtension(workspaceSlug),
  ]);
  return [
    ...base,
    Underline,
    TextAlign.configure({ alignments: ["left", "center", "right"], types: ["heading", "paragraph"] }),
    TaskList,
    // Nested checkable items: a sub-item keeps its own checkbox state.
    TaskItem.configure({ nested: true }),
    ...tables,
    color,
    image,
    mention,
    ...(blocks as AnyExtension[]),
  ];
}
