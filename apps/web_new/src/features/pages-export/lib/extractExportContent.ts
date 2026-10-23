// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export content extraction (parity/specs/exports.md §5, EXP-018):
// paragraphs and picture URLs from the host's live editor HTML. Mentions
// resolve to their display names through the text read; nodes the Pages
// area marks data-export="exclude" (embedded issue cards) are dropped.
export interface ExtractedExportContent {
  paragraphs: string[];
  imageUrls: string[];
}

/** Block elements that become one paragraph each, outermost match wins. */
const BLOCK_SELECTOR = "p, h1, h2, h3, h4, h5, h6, li, blockquote, pre";

/** Subtrees that never export (plus script/style, which carry no text). */
const EXCLUDED_SELECTOR = '[data-export="exclude"], script, style';

/** Collect outermost-block texts in document order (no nested duplication). */
function collectBlocks(element: Element, into: string[]): void {
  for (const child of element.children) {
    if (child.matches(BLOCK_SELECTOR)) {
      const text = (child.textContent || "").trim();
      if (text !== "") into.push(text);
    } else {
      collectBlocks(child, into);
    }
  }
}

/** Paragraphs and picture URLs from editor HTML. Requires a DOM. */
export function extractExportContent(html: string): ExtractedExportContent {
  const parsed = new DOMParser().parseFromString(html, "text/html");
  for (const excluded of parsed.querySelectorAll(EXCLUDED_SELECTOR)) excluded.remove();
  const imageUrls: string[] = [];
  for (const image of parsed.querySelectorAll("img")) {
    const src = image.getAttribute("src");
    if (src === null || src === "") continue;
    imageUrls.push(src);
  }
  const paragraphs: string[] = [];
  collectBlocks(parsed.body, paragraphs);
  if (paragraphs.length === 0) {
    const fallback = (parsed.body.textContent || "").trim();
    if (fallback !== "") paragraphs.push(fallback);
  }
  return { paragraphs, imageUrls };
}
