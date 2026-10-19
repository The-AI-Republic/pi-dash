// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored-HTML sanitizer (UI kit and editor page). Every read surface
// (detail, peek, comments, activity) renders description and comment
// HTML through here, never raw: stored markup comes from other clients
// and may carry scripts, event handlers, or unsafe links. The editor's
// own node elements (mentions, images) are part of stored bodies, so
// their names and attributes are allowed exactly as the API's content
// validator allows them; anything else stays stripped.

import DOMPurify from "dompurify";

import { MENTION_CONTENT_TAGS } from "./mention.js";

/** Editor node elements stored bodies carry beside ordinary HTML. */
export const STORED_CONTENT_TAGS = ["mention-component", "image-component", "label", "input"] as const;

/**
 * Attributes the stored-body contract keeps on the editor nodes
 * (apps/api content_validator.py). `data-name` rides on a mention so
 * readers can show the person's name without another lookup.
 */
const STORED_CONTENT_ATTRIBUTES: Record<string, readonly string[]> = {
  "mention-component": ["id", "entity_identifier", "entity_name", "data-name"],
  "image-component": [
    "id",
    "src",
    "width",
    "height",
    "aspectRatio",
    "aspectratio",
    "alignment",
    "status",
    "alt",
    "title",
  ],
  img: ["src", "alt", "title", "width", "height", "aspectRatio", "aspectratio", "alignment"],
  label: ["class", "for"],
  input: ["type", "checked", "disabled"],
};

/** Clean stored rich-text HTML for rendering. Returns safe markup. */
export function sanitizeHtml(dirty: string): string {
  const allowedAttributes = Object.values(STORED_CONTENT_ATTRIBUTES).flat();
  return DOMPurify.sanitize(dirty, {
    ADD_TAGS: [...STORED_CONTENT_TAGS],
    ADD_ATTR: [...new Set([...allowedAttributes, "data-name"])],
  });
}

/** The name a stored mention shows: the stored name, else the id. */
export function mentionDisplayName(mention: StoredMention): string {
  if (mention.name !== null && mention.name.trim() !== "") return mention.name;
  return mention.entityIdentifier !== "" ? mention.entityIdentifier : mention.id;
}

/**
 * Stored HTML turned into markup every read surface can paint (ISS-127).
 * Stored bodies carry the editor's custom elements: a mention has no
 * text of its own and an image is an `image-component`, so a plain
 * sanitize would leave a gap where the person or picture was. This
 * wraps each mention in a chip naming the person and rewrites an image
 * component as an `<img>`; everything else passes through the sanitizer.
 * The stored body itself is never changed — only this view is.
 */
export function renderableHtml(dirty: string): string {
  const clean = sanitizeHtml(dirty);
  if (typeof globalThis.DOMParser === "undefined") return clean;
  const document = new DOMParser().parseFromString(clean, "text/html");
  for (const element of Array.from(document.body.querySelectorAll("mention-component"))) {
    const mention = storedMentionOf(element);
    if (mention === null) {
      element.remove();
      continue;
    }
    const chip = document.createElement("span");
    chip.setAttribute("data-mention-id", mention.id);
    chip.setAttribute("class", "mention-chip");
    chip.textContent = `@${mentionDisplayName(mention)}`;
    element.replaceWith(chip);
  }
  for (const element of Array.from(document.body.querySelectorAll("image-component"))) {
    const src = element.getAttribute("src") ?? "";
    if (src === "") {
      element.remove();
      continue;
    }
    const image = document.createElement("img");
    image.setAttribute("src", src);
    for (const name of ["alt", "title", "width", "height"]) {
      const value = element.getAttribute(name);
      if (value !== null && value !== "") image.setAttribute(name, value);
    }
    element.replaceWith(image);
  }
  return document.body.innerHTML;
}

/** One stored mention's identity and, when the writer kept it, its name. */
export interface StoredMention {
  id: string;
  entityIdentifier: string;
  entityName: string;
  /** Display name the writer stored, or null on older bodies. */
  name: string | null;
}

/** Read one mention element; null when it carries no identity. */
export function storedMentionOf(element: Element): StoredMention | null {
  const entityIdentifier = element.getAttribute("entity_identifier") ?? "";
  const id = element.getAttribute("id") ?? entityIdentifier;
  if (id === "" && entityIdentifier === "") return null;
  return {
    id,
    entityIdentifier: entityIdentifier === "" ? id : entityIdentifier,
    entityName: element.getAttribute("entity_name") ?? "user_mention",
    name: element.getAttribute("data-name"),
  };
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
