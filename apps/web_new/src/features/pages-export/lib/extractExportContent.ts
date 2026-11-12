// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export content extraction (parity/specs/exports.md §5, EXP-018):
// paragraphs and picture URLs from the host's live editor HTML. Mentions
// resolve to their display names through the text read; nodes the Pages
// area marks data-export="exclude" (embedded issue cards) are dropped.
// Uploaded pictures arrive in the stored image-component form carrying a
// bare asset id, which resolves to its served stack URL when the host's
// workspace/project context is present (NEWFRONT-470).
import { pageAssetUrl } from "@pidash/api-client";

export interface ExtractedExportContent {
  paragraphs: string[];
  imageUrls: string[];
}

/** Workspace/project context that turns stored asset ids into served URLs. */
export interface ExtractExportContentOptions {
  workspaceSlug?: string | undefined;
  projectId?: string | undefined;
}

/** Block elements that become one paragraph each, outermost match wins. */
const BLOCK_SELECTOR = "p, h1, h2, h3, h4, h5, h6, li, blockquote, pre";

/** Subtrees that never export (plus script/style, which carry no text). */
const EXCLUDED_SELECTOR = '[data-export="exclude"], script, style';

/** Picture elements the extractor reads, in document order. */
const IMAGE_SELECTOR = "image-component, img";

/** A picture source that is already an address: a scheme, protocol-relative or root-relative. */
const SERVED_IMAGE_SOURCE = /^(?:[a-z][a-z0-9+.-]*:|\/\/|\/)/i;

/** True when a non-empty source is a stored asset id rather than an address: no scheme, no leading slash, no path. */
function isStoredAssetId(src: string): boolean {
  return !SERVED_IMAGE_SOURCE.test(src) && !src.includes("/");
}

/** Stored ids resolve to their served stack URL when the context names both legs; anything else passes through. */
function resolveImageSource(src: string, options: ExtractExportContentOptions): string {
  const slug = options.workspaceSlug ?? "";
  const project = options.projectId ?? "";
  if (isStoredAssetId(src) && slug !== "" && project !== "") return pageAssetUrl(slug, project, src);
  return src;
}

/** The stored picture's own source: the kept id wins over the painted address, falling back to src. */
function storedImageSource(element: Element): string {
  const stored = element.getAttribute("data-stored-src");
  if (stored !== null && stored !== "") return stored;
  return element.getAttribute("src") ?? "";
}

/** True when the picture sits inside a stored element that already resolves it (its painted inner image). */
function hasResolvingAncestor(image: Element): boolean {
  const ancestor = image.closest("image-component");
  return ancestor !== null && storedImageSource(ancestor) !== "";
}

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
export function extractExportContent(html: string, options: ExtractExportContentOptions = {}): ExtractedExportContent {
  const parsed = new DOMParser().parseFromString(html, "text/html");
  for (const excluded of parsed.querySelectorAll(EXCLUDED_SELECTOR)) excluded.remove();
  const imageUrls: string[] = [];
  for (const image of parsed.querySelectorAll(IMAGE_SELECTOR)) {
    if (image.tagName.toLowerCase() === "image-component") {
      const src = storedImageSource(image);
      if (src === "") continue;
      imageUrls.push(resolveImageSource(src, options));
    } else {
      if (hasResolvingAncestor(image)) continue;
      const src = image.getAttribute("src");
      if (src === null || src === "") continue;
      imageUrls.push(resolveImageSource(src, options));
    }
  }
  const paragraphs: string[] = [];
  collectBlocks(parsed.body, paragraphs);
  if (paragraphs.length === 0) {
    const fallback = (parsed.body.textContent || "").trim();
    if (fallback !== "") paragraphs.push(fallback);
  }
  return { paragraphs, imageUrls };
}
