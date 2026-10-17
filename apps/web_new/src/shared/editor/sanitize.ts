// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored-HTML sanitizer (UI kit and editor page). Every read surface
// (detail, peek, comments, activity) renders description and comment
// HTML through here, never raw: stored markup comes from other clients
// and may carry scripts, event handlers, or unsafe links.

import DOMPurify from "dompurify";

/** Clean stored rich-text HTML for rendering. Returns safe markup. */
export function sanitizeHtml(dirty: string): string {
  return DOMPurify.sanitize(dirty);
}

/** Plain text of stored rich-text HTML, for empty checks and previews. */
export function storedTextContent(html: string): string {
  if (typeof globalThis.DOMParser === "undefined") return "";
  const document = new DOMParser().parseFromString(html, "text/html");
  return document.body.textContent ?? "";
}

/** True when stored HTML carries no visible text (empty editor output). */
export function isStoredHtmlEmpty(html: string): boolean {
  return storedTextContent(html).trim().length === 0;
}
