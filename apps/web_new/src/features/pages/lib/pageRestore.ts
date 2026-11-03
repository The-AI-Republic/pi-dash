// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Restoring a snapshot into the live document (parity/specs/pages.md
// §6.9, §11.1, PAGE-053). There is no restore endpoint: the snapshot's
// body replaces the one the editor is holding, in the page the reader is
// already on. This module is the whole write, so the caller only has to
// say whether it happened and pick the notice for each answer.

import type { PageEditorHandle } from "./pageEditorAdapter.js";

/** Shown when the snapshot became the live body. */
export const PAGE_VERSION_RESTORE_SUCCESS = "Page version restored.";

/** Shown when the snapshot could not reach the live editor. */
export const PAGE_VERSION_RESTORE_FAILED = "Page version could not be restored.";

/**
 * Write `html` over the live body. Answers false when there is no live
 * editor to write into (its chunk never resolved, or the canvas went
 * away) or when the write itself was refused, so the caller reports the
 * failure instead of pretending the document changed.
 */
export function applyPageRestore(handle: PageEditorHandle | null, html: string): boolean {
  if (handle === null) return false;
  try {
    return handle.replaceBody(html);
  } catch {
    return false;
  }
}
