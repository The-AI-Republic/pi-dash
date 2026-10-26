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

import DOMPurify, { type UponSanitizeAttributeHook } from "dompurify";

import { MENTION_CONTENT_TAGS, MENTION_ENTITY } from "./mention.js";
import { isCurrentMentionUser, mentionNameFor } from "./mentionDirectory.js";

/** Editor node elements stored bodies carry beside ordinary HTML. */
export const STORED_CONTENT_TAGS = [
  "mention-component",
  "image-component",
  "label",
  "input",
  "issue-embed-component",
  "entity-reference-component",
  "callout-component",
] as const;

/**
 * Attributes the stored-body contract keeps on the editor nodes
 * (apps/api content_validator.py). `data-name` rides on a mention so
 * readers can show the person's name without another lookup.
 */
const STORED_CONTENT_ATTRIBUTES: Record<string, readonly string[]> = {
  "mention-component": ["id", "entity_identifier", "entity_name", "data-name"],
  // Work-item embeds keep every identifier the document stores (PAGE-040).
  "issue-embed-component": ["id", "entity_identifier", "project_identifier", "workspace_identifier", "entity_name"],
  // Entity references resolve through the shell peek (PAGE-043).
  "entity-reference-component": [
    "entity_name",
    "entity_identifier",
    "project_identifier",
    "workspace_identifier",
    "project_id",
    "data-label",
  ],
  "callout-component": ["data-type"],
  // Stored callout form the API allowlist keeps (PAGE-037).
  div: [
    "data-block-type",
    "data-background",
    "data-icon-name",
    "data-icon-color",
    "data-emoji-unicode",
    "data-emoji-url",
    "data-logo-in-use",
  ],
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
  // Table cells keep their spans and stored widths so a reloaded table
  // matches what the editor wrote (PAGE-033).
  td: ["colspan", "rowspan", "data-colwidth"],
  th: ["colspan", "rowspan", "data-colwidth"],
  // Letter and wash marks (PAGE-036): the stored palette key paints the
  // span and is what a later edit reads back.
  // Letter/wash keys and the inline body emoji marker (PAGE-036, PAGE-042).
  span: ["data-text-color", "data-background-color", "data-type"],
  label: ["class", "for"],
  input: ["type", "checked", "disabled"],
};

/**
 * Attributes the stored-body contract keeps beyond the sanitizer's
 * defaults, each on the one element that carries it. A link stores the
 * window it opens in (`target`), which the API's content validator keeps
 * and the editor writes beside the address; DOMPurify would drop it.
 */
const ELEMENT_ATTRIBUTES: Readonly<Record<string, string>> = { a: "target" };

/** Clean stored rich-text HTML for rendering. Returns safe markup. */
export function sanitizeHtml(dirty: string): string {
  const allowedAttributes = Object.values(STORED_CONTENT_ATTRIBUTES).flat();
  // The hook is the only way the attribute survives: the allowlist would
  // keep it on every element. Unsafe addresses are still rejected.
  const keepElementAttribute: UponSanitizeAttributeHook = (node, data) => {
    if (ELEMENT_ATTRIBUTES[node.nodeName.toLowerCase()] === data.attrName) data.forceKeepAttr = true;
  };
  DOMPurify.addHook("uponSanitizeAttribute", keepElementAttribute);
  try {
    return DOMPurify.sanitize(dirty, {
      ADD_TAGS: [...STORED_CONTENT_TAGS],
      ADD_ATTR: [...new Set([...allowedAttributes, "data-name"])],
    });
  } finally {
    DOMPurify.removeHook("uponSanitizeAttribute", keepElementAttribute);
  }
}

/** Absolute or self-describing sources never need the asset path. */
const ABSOLUTE_ASSET_SRC = /^(https?:|data:|blob:|\/)/i;

/** Where a stored description image lives, given the row it belongs to. */
export interface DescriptionAssetScope {
  workspaceSlug: string;
  /** Null for a draft that holds no project yet. */
  projectId: string | null;
  /** API origin; omitted legs stay app-relative. */
  apiBaseUrl?: string | undefined;
}

/**
 * Resolve a stored `image-component` source (ISS-127). Older clients kept
 * the asset's id in `src`, not a URL: the API rewrites asset ids in
 * `copy_s3_object` and serves the bytes at the workspace/project asset
 * path. Absolute, data, blob and already-rooted legs pass through; a bare
 * id joins that path. Without a scope the id cannot be resolved and is
 * returned untouched.
 */
export function descriptionAssetSrc(src: string, scope: DescriptionAssetScope | undefined): string {
  if (src === "" || ABSOLUTE_ASSET_SRC.test(src) || scope === undefined) return src;
  const project = scope.projectId === null ? "" : `projects/${scope.projectId}/`;
  const path = `/api/assets/v2/workspaces/${scope.workspaceSlug}/${project}${src}/`;
  if (scope.apiBaseUrl === undefined || scope.apiBaseUrl === "") return path;
  return `${scope.apiBaseUrl.replace(/\/+$/, "")}${path}`;
}

/** The asset source a stored node carries, before display resolution. */
export function storedImageSrc(element: Element): string {
  return element.getAttribute("data-stored-src") ?? element.getAttribute("src") ?? "";
}

/**
 * Give the editor a paintable source for legacy `image-component` nodes
 * while keeping the stored id for the round-trip (ISS-127). The node
 * carries the resolved source in `src` and the stored one in
 * `data-stored-src`; serialization reads the stored one back.
 */
export function prepareEditorHtml(dirty: string, resolve: ((src: string) => string) | undefined): string {
  if (resolve === undefined || typeof globalThis.DOMParser === "undefined") return dirty;
  const document = new DOMParser().parseFromString(dirty, "text/html");
  for (const element of Array.from(document.body.querySelectorAll("image-component"))) {
    const stored = element.getAttribute("src") ?? "";
    if (stored === "" || ABSOLUTE_ASSET_SRC.test(stored)) continue;
    element.setAttribute("src", resolve(stored));
    element.setAttribute("data-stored-src", stored);
  }
  return document.body.innerHTML;
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
 * With a workspace slug, a person mention becomes a link to the
 * member's profile (CMT-020); other entity kinds stay plain chips.
 */
export function renderableHtml(
  dirty: string,
  resolveAsset?: ((src: string) => string) | undefined,
  workspaceSlug?: string | undefined
): string {
  const clean = sanitizeHtml(dirty);
  if (typeof globalThis.DOMParser === "undefined") return clean;
  const slug = workspaceSlug ?? "";
  const document = new DOMParser().parseFromString(clean, "text/html");
  for (const element of Array.from(document.body.querySelectorAll("mention-component"))) {
    const mention = storedMentionOf(element);
    if (mention === null) {
      element.remove();
      continue;
    }
    // The href is always an app-relative profile path built from the
    // caller's slug and the stored member id, so it can never become a
    // script URL; attribute serialization escapes the id itself.
    const href =
      slug !== "" && mention.entityName === MENTION_ENTITY ? `/${slug}/profile/${mention.entityIdentifier}` : null;
    const chip = document.createElement(href === null ? "span" : "a");
    if (href !== null) chip.setAttribute("href", href);
    chip.setAttribute("data-mention-id", mention.id);
    const current = isCurrentMentionUser(mention.entityIdentifier);
    chip.setAttribute("class", current ? "mention-chip mention-chip-current" : "mention-chip");
    if (current) chip.setAttribute("data-current-user", "true");
    chip.textContent = `@${mentionNameFor(mention.entityIdentifier, mention.name) || mentionDisplayName(mention)}`;
    element.replaceWith(chip);
  }
  for (const element of Array.from(document.body.querySelectorAll("image-component"))) {
    const stored = storedImageSrc(element);
    const src = resolveAsset === undefined ? stored : resolveAsset(stored);
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
  for (const element of Array.from(
    document.body.querySelectorAll('callout-component, div[data-block-type="callout-component"]')
  )) {
    // A callout is a container in storage; the read surface paints it as a
    // distinct aside while keeping its child blocks intact (PAGE-037). The
    // allowlisted stored form is the div; the tag stays readable for
    // bodies saved by earlier builds.
    const aside = document.createElement("aside");
    aside.setAttribute("data-type", "callout");
    aside.setAttribute("class", "page-callout");
    aside.append(...Array.from(element.childNodes));
    element.replaceWith(aside);
  }
  for (const element of Array.from(document.body.querySelectorAll("issue-embed-component"))) {
    // Community never fetches the referenced work item: it shows the
    // upgrade prompt where a licensed resolver would paint the card
    // (PAGE-040, §13.4). The prompt carries the kit's Pro marker and the
    // kit's outbound-link styling; identifiers remain data attributes for
    // the edition slot. The static read surface cannot mount React, so the
    // marker is the same pill markup the kit renders.
    const card = document.createElement("div");
    card.setAttribute("class", "page-embed-upgrade");
    card.setAttribute("data-embed-upgrade", "");
    for (const name of ["id", "entity_identifier", "project_identifier", "workspace_identifier", "entity_name"]) {
      const value = element.getAttribute(name);
      if (value !== null && value !== "") card.setAttribute(`data-${name.replace(/_/g, "-")}`, value);
    }
    const title = document.createElement("p");
    title.setAttribute("class", "flex items-center gap-2 font-medium");
    title.textContent = "Work-item embeds are a Pro feature ";
    const pill = document.createElement("span");
    pill.setAttribute("data-testid", "upgrade-pill");
    pill.setAttribute(
      "class",
      "inline-flex w-fit items-center rounded-(--radius-full) bg-(--accent-subtle) py-px text-caption font-medium whitespace-nowrap text-(--accent) px-(--space-2)"
    );
    pill.textContent = "Pro";
    title.append(pill);
    const body = document.createElement("p");
    body.textContent = "Upgrade to show live work items inside a page.";
    const link = document.createElement("a");
    link.setAttribute("href", "https://airepublic.com/pricing");
    link.setAttribute("target", "_blank");
    link.setAttribute("rel", "noopener noreferrer");
    link.setAttribute("class", "text-(--accent) underline underline-offset-2 hover:text-(--accent-hover)");
    link.textContent = "Upgrade";
    card.append(title, body, link);
    element.replaceWith(card);
  }
  for (const element of Array.from(document.body.querySelectorAll("entity-reference-component"))) {
    // A recognised entity paints as a preview card; its identifiers stay on
    // the node so the shell can open the matching peek (PAGE-043, §13.2).
    const label = element.getAttribute("data-label") ?? element.getAttribute("entity_identifier") ?? "";
    const preview = document.createElement("span");
    preview.setAttribute("class", "page-entity-preview");
    preview.setAttribute("data-entity-preview", "");
    preview.setAttribute("role", "button");
    preview.setAttribute("tabindex", "0");
    for (const name of [
      "entity_name",
      "entity_identifier",
      "project_identifier",
      "workspace_identifier",
      "project_id",
    ]) {
      const value = element.getAttribute(name);
      if (value !== null && value !== "") preview.setAttribute(`data-${name.replace(/_/g, "-")}`, value);
    }
    preview.textContent = label;
    element.replaceWith(preview);
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
