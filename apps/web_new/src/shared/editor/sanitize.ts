// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored-HTML sanitizer (UI kit and editor page). Every read surface
// (detail, peek, comments, activity) renders description and comment
// HTML through here, never raw: stored markup comes from other clients
// and may carry scripts, event handlers, or unsafe links.

import DOMPurify from "dompurify";

import { MENTION_CONTENT_TAGS } from "./mention.js";

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

/**
 * True when stored HTML carries neither visible text nor any of the
 * `allowedTags` (lower-case names) the surface treats as content. Comment
 * bodies count images and mentions as words: a picture with no caption is
 * still a comment.
 */
export function isStoredHtmlEmptyIgnoring(html: string, allowedTags: readonly string[]): boolean {
  if (storedTextContent(html).trim().length !== 0) return false;
  if (typeof globalThis.DOMParser === "undefined") return true;
  const document = new DOMParser().parseFromString(html, "text/html");
  const names = new Set(allowedTags.map((tag) => tag.toLowerCase()));
  return !Array.from(document.body.querySelectorAll("*")).some((element) => names.has(element.tagName.toLowerCase()));
}

/**
 * The one "stored description has content" rule (ISS-126/127): words, an
 * image or a person mention all count. The editor's mount-time empty
 * paragraph counts as nothing. Payload builders, the discard guard and
 * the detail-read baseline fold all use this so a picture-only body is
 * never dropped on save.
 */
export function hasStoredDescriptionContent(html: string): boolean {
  return !isStoredHtmlEmptyIgnoring(html, MENTION_CONTENT_TAGS);
}
