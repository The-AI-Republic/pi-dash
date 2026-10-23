// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Plain-text export scope (parity/specs/exports.md §5, EXP-020; intended
// behavior for NEWFRONT-356): the image-free scope drops every Markdown
// image tag while keeping the surrounding text. Link definitions are
// left alone (they are indistinguishable from link targets).
import type { PageExportScope } from "./pageExportModel.js";

/** Inline `![alt](url)` image tags. */
const INLINE_IMAGE = /!\[[^\]\n]*\]\([^)\n]*\)/g;

/** Full-reference `![alt][ref]` and collapsed `![alt][]` image tags. */
const REFERENCE_IMAGE = /!\[[^\]\n]*\]\[[^\]\n]*\]/g;

/** Shortcut-reference `![alt]` image tags (after the longer forms are gone). */
const SHORTCUT_IMAGE = /!\[[^\]\n]*\]/g;

/** Remove every Markdown image tag from the text. */
export function stripMarkdownImages(markdown: string): string {
  return markdown.replace(INLINE_IMAGE, "").replace(REFERENCE_IMAGE, "").replace(SHORTCUT_IMAGE, "");
}

/** Honor the content scope: full fidelity keeps the markup untouched. */
export function applyMarkdownScope(markdown: string, scope: PageExportScope): string {
  if (scope === "No images") return stripMarkdownImages(markdown);
  return markdown;
}
