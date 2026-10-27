// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages navigation pane model (parity/specs/pages.md §6.5–§6.9,
// PAGE-045–050). Everything the pane shows is derived from the live
// document: the outline lists the first three heading levels in their
// reading order, the statistics read the document's text, and the assets
// tab lists the pictures the document embeds. The same pure derivation
// serves the read-only body and every live editor, so the pane can never
// disagree with what is on the canvas.

import type { WorkspaceMemberList } from "@pidash/api-client";

import { PAGE_PANE_TABS, type PagePaneTab } from "./pageDetail.js";

export type PageHeadingLevel = 1 | 2 | 3;

/** One outline entry: the heading's level, text, and per-level rank. */
export interface PageHeading {
  level: PageHeadingLevel;
  text: string;
  /** 1-based occurrence among headings of the same level (scroll target). */
  levelIndex: number;
}

/** Live document statistics (PAGE-049). */
export interface PageDocumentStats {
  words: number;
  characters: number;
  paragraphs: number;
}

/** One attachment row (PAGE-048); plain links are never rows. */
export interface PageAssetRow {
  /** Stored source: an absolute URL or a workspace asset id. */
  source: string;
  /** Name the row shows (alt text, else the file name). */
  name: string;
  /** Image rows render a thumbnail; other kinds use the extension renderer. */
  kind: "image" | "attachment";
  /** Where selecting the row goes (the view variant). */
  viewHref: string;
  /** Where the hover download action goes (the download variant). */
  downloadHref: string;
  /** Thumbnail source; null when the row carries no picture. */
  thumbnailHref: string | null;
}

/** Everything the pane reads from one document state. */
export interface PageDocument {
  headings: PageHeading[];
  stats: PageDocumentStats;
  assets: PageAssetRow[];
}

export const EMPTY_PAGE_DOCUMENT: PageDocument = {
  headings: [],
  stats: { words: 0, characters: 0, paragraphs: 0 },
  assets: [],
};

/** Reading speed the estimate assumes (spec §6.8, oracle-pinned wording). */
export const PAGE_READ_WORDS_PER_MINUTE = 200;

const ABSOLUTE_SOURCE = /^(https?:|data:|blob:|\/)/i;

/** The workspace/project asset prefix a served picture carries (spec §11.4). */
function assetViewPrefix(scope: PageAssetScope): string {
  const project = scope.projectId === null ? "" : `projects/${scope.projectId}/`;
  return `/api/assets/v2/workspaces/${scope.workspaceSlug}/${project}`;
}

/** Tabs in their pinned order (PAGE-045: Outline, Info, Assets). */
export const PAGE_PANE_TAB_ORDER: readonly PagePaneTab[] = PAGE_PANE_TABS.map((tab) => tab.value);

/** The pane's fixed width in CSS pixels (PAGE-045, oracle: 290–298). */
export const PAGE_PANE_WIDTH_PX = 296;

/**
 * The canvas's readable measure (spec §6.5; the oracle pins both ends):
 * the narrow measure while the pane is shut, the wide one while it is
 * open, so the pane's slide reflows the canvas rather than overlaying it.
 */
export const PAGE_CANVAS_MEASURE_PX = { readable: "720px", wide: "1152px" } as const;

/** Indentation per outline level; the ratio 1:2:4 is oracle-pinned. */
export const PAGE_OUTLINE_INDENT_PX: Record<PageHeadingLevel, number> = { 1: 16, 2: 32, 3: 64 };

/** Bar widths per heading level in the collapsed preview (PAGE-046). */
export const PAGE_PREVIEW_BAR_WIDTH_PX: Record<PageHeadingLevel, number> = { 1: 24, 2: 18, 3: 14 };

/** Workspace asset URL context (spec §11.4). */
export interface PageAssetScope {
  workspaceSlug: string;
  projectId: string | null;
}

function parseHtml(html: string): Document | null {
  if (html.trim() === "" || typeof globalThis.DOMParser === "undefined") return null;
  return new DOMParser().parseFromString(html, "text/html");
}

function normalizeSource(src: string): string {
  return src.trim();
}

/** Last segment of a path, or the empty string when it names no file. */
function lastPathSegment(path: string): string {
  const segments = path.split("/").filter((segment) => segment !== "");
  return segments.length === 0 ? "" : (segments[segments.length - 1] as string);
}

/** File name a stored source ends with, without its query or fragments. */
export function assetFileName(source: string): string {
  const withoutQuery = normalizeSource(source).replace(/[?#].*$/, "");
  // An absolute address names a file by its path, never by its host.
  let path = withoutQuery;
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(withoutQuery)) {
    try {
      path = new URL(withoutQuery).pathname;
    } catch {
      path = withoutQuery;
    }
  }
  const tail = lastPathSegment(path);
  try {
    return decodeURIComponent(tail);
  } catch {
    return tail;
  }
}

/** View URL for a stored source: absolute sources stay as they are. */
export function pageAssetViewHref(source: string, scope: PageAssetScope): string {
  const stored = normalizeSource(source);
  if (stored === "") return "";
  if (ABSOLUTE_SOURCE.test(stored)) return stored;
  const assetId = encodeURIComponent(stored);
  return scope.projectId === null
    ? `/api/assets/v2/workspaces/${scope.workspaceSlug}/${assetId}/`
    : `/api/assets/v2/workspaces/${scope.workspaceSlug}/projects/${scope.projectId}/${assetId}/`;
}

/**
 * Download URL for a stored source. An absolute address stays as it is,
 * and a picture the workspace already serves keeps its asset id but
 * switches to the download leg.
 */
export function pageAssetDownloadHref(source: string, scope: PageAssetScope): string {
  const stored = normalizeSource(source);
  if (stored === "") return "";
  const served = assetViewPrefix(scope);
  if (stored.startsWith(served) && stored.endsWith("/")) {
    return `${served}download/${stored.slice(served.length)}`;
  }
  if (ABSOLUTE_SOURCE.test(stored)) return stored;
  const assetId = encodeURIComponent(stored);
  return scope.projectId === null
    ? `/api/assets/v2/workspaces/${scope.workspaceSlug}/download/${assetId}/`
    : `/api/assets/v2/workspaces/${scope.workspaceSlug}/projects/${scope.projectId}/download/${assetId}/`;
}

/**
 * The stored source on a picture or attachment node. Both keep it on
 * `src`: a plain image a URL, a stored editor node its asset id.
 */
function imageSourceOf(element: Element): string {
  return normalizeSource(element.getAttribute("src") ?? "");
}

/** An element's text; DOM nodes always carry one, even when it is empty. */
function elementText(element: Element): string {
  return element.textContent as string;
}

function assetRowOf(element: Element, scope: PageAssetScope): PageAssetRow | null {
  const source = imageSourceOf(element);
  if (source === "") return null;
  const image = element.tagName.toLowerCase() !== "attachment-component";
  const alt = (element.getAttribute("alt") ?? "").trim();
  const title = (element.getAttribute("title") ?? "").trim();
  // A stored editor picture is named after its own node id, the readable
  // label the editor writes on upload; only a picture without one falls
  // back to the file name (PAGE-048, PAGE-039).
  const nodeId = image ? (element.getAttribute("id") ?? "").trim() : "";
  const label = alt !== "" ? alt : title !== "" ? title : nodeId !== "" ? `image-${nodeId}` : assetFileName(source);
  const viewHref = pageAssetViewHref(source, scope);
  return {
    source,
    name: label === "" ? (image ? "Image" : "Attachment") : label,
    kind: image ? "image" : "attachment",
    viewHref,
    downloadHref: pageAssetDownloadHref(source, scope),
    // Only a picture has something to paint; other kinds hand their row
    // to the renderer their extension registers (PAGE-048, §9.4).
    thumbnailHref: image ? viewHref : null,
  };
}

/**
 * Outline, statistics and assets for one stored or live document. Headings
 * beyond the third level are excluded (PAGE-047); paragraphs count the
 * non-empty text blocks, as the editor's own document info does.
 */
export function parsePageDocument(html: string, scope: PageAssetScope): PageDocument {
  const document = parseHtml(html);
  if (document === null) return EMPTY_PAGE_DOCUMENT;

  const headings: PageHeading[] = [];
  const perLevel = new Map<PageHeadingLevel, number>();
  let words = 0;
  let characters = 0;
  let paragraphs = 0;

  const selector = "h1, h2, h3, h4, h5, h6, p, li, blockquote, pre";
  for (const block of Array.from(document.body.querySelectorAll(selector))) {
    // Nested blocks (a list item's paragraph, a quote's paragraph) are
    // counted once, at their innermost text block.
    if (block.querySelector(selector) !== null) continue;
    if (elementText(block).trim() === "") continue;
    const text = elementText(block).replace(/\s+/g, " ").trim();
    const tag = block.tagName.toLowerCase();
    const level = Number(tag.slice(1));
    if (tag.startsWith("h") && (level === 1 || level === 2 || level === 3)) {
      const headingLevel = level as PageHeadingLevel;
      const levelIndex = (perLevel.get(headingLevel) ?? 0) + 1;
      perLevel.set(headingLevel, levelIndex);
      headings.push({ level: headingLevel, text, levelIndex });
    }
    // Only prose paragraphs count toward the paragraph card; a heading
    // or a list line is its own block, not a paragraph (PAGE-049).
    if (tag === "p") paragraphs += 1;
    characters += text.length;
    words += text.split(" ").filter((word) => word !== "").length;
  }

  const assets: PageAssetRow[] = [];
  const nodes = "img, image-component, attachment-component";
  for (const element of Array.from(document.body.querySelectorAll(nodes))) {
    const row = assetRowOf(element, scope);
    if (row !== null) assets.push(row);
  }

  return { headings, stats: { words, characters, paragraphs }, assets };
}

/** Estimated reading time in whole seconds. */
export function pageReadTimeSeconds(words: number): number {
  return Math.round((Math.max(0, words) / PAGE_READ_WORDS_PER_MINUTE) * 60);
}

/** Reading time wording: seconds under a minute, minutes above (PAGE-049). */
export function pageReadTimeLabel(words: number): string {
  const seconds = pageReadTimeSeconds(words);
  return seconds < 60 ? `${seconds}s` : `${Math.ceil(seconds / 60)}m`;
}

/** Short relative age ("3m", "2h", "5d") used by the attribution row. */
export function shortRelativeAge(isoTimestamp: string, nowMs: number = Date.now()): string {
  const atMs = Date.parse(isoTimestamp);
  const elapsedMs = Number.isNaN(atMs) ? 0 : Math.max(0, nowMs - atMs);
  const minutes = elapsedMs / 60_000;
  if (minutes < 1) return `${Math.floor(elapsedMs / 1_000)}s`;
  if (minutes < 60) return `${Math.floor(minutes)}m`;
  const hours = minutes / 60;
  if (hours < 24) return `${Math.floor(hours)}h`;
  const days = hours / 24;
  if (days < 30) return `${Math.floor(days)}d`;
  if (days < 365) return `${Math.floor(days / 30)}mo`;
  return `${Math.floor(days / 365)}y`;
}

/** Calendar date for the creator row ("Sep 1, 2026"; PAGE-050). */
export function pageCalendarDate(isoTimestamp: string): string {
  const atMs = Date.parse(isoTimestamp);
  if (Number.isNaN(atMs)) return isoTimestamp;
  return new Date(atMs).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

/** Display name the pane shows for a member it cannot resolve. */
export const PAGE_DEACTIVATED_USER = "Deactivated user";

/** A member row the pane resolves names and avatars from. */
export interface PageActorMember {
  id: string;
  name: string;
  avatarUrl: string | null;
}

/**
 * The directory read as the attribution rows need it: the user id under
 * `member` (the membership id is a different key), the display name and
 * the avatar. Everything else about the membership is irrelevant here.
 */
export function pageActorMembers(members: WorkspaceMemberList | undefined): PageActorMember[] | undefined {
  if (members === undefined) return undefined;
  return members.map((entry) => ({
    id: entry.member.id,
    name: entry.member.display_name,
    avatarUrl: entry.member.avatar_url ?? null,
  }));
}

/** One attribution row (PAGE-050). */
export interface PageActorRow {
  role: "edited" | "created";
  /** Stored user id, or null when the record carries none. */
  userId: string | null;
  name: string;
  /** True when the member is not in the directory any more. */
  deactivated: boolean;
  avatarUrl: string | null;
  /** Relative age for the editor, calendar date for the creator. */
  timeText: string;
  /** Member-profile address, or null when there is nobody to link. */
  href: string | null;
}

export interface PageActorInput {
  workspaceSlug: string;
  updatedBy: string | null;
  updatedAt: string;
  ownedBy: string | null;
  createdAt: string;
  members: readonly PageActorMember[] | undefined;
  nowMs?: number;
}

/** Display name for a stored user id, or the fallback when unlisted. */
export function pageMemberName(members: readonly PageActorMember[] | undefined, userId: string | null): string | null {
  if (userId === null) return null;
  return members?.find((entry) => entry.id === userId)?.name ?? PAGE_DEACTIVATED_USER;
}

/** Member-profile address (spec §13.3: `/{ws}/profile/{userId}/`). */
export function pageMemberProfileHref(workspaceSlug: string, userId: string): string {
  return `/${workspaceSlug}/profile/${userId}/`;
}

function actorIdentity(
  workspaceSlug: string,
  userId: string | null,
  members: readonly PageActorMember[] | undefined
): Pick<PageActorRow, "userId" | "name" | "deactivated" | "avatarUrl" | "href"> {
  const member = userId === null ? undefined : members?.find((entry) => entry.id === userId);
  return {
    userId,
    name: pageMemberName(members, userId) ?? PAGE_DEACTIVATED_USER,
    deactivated: member === undefined,
    avatarUrl: member?.avatarUrl ?? null,
    href: userId === null ? null : pageMemberProfileHref(workspaceSlug, userId),
  };
}

/** The two attribution rows, always both present (PAGE-050). */
export function pageActorRows(input: PageActorInput): PageActorRow[] {
  const editor = actorIdentity(input.workspaceSlug, input.updatedBy, input.members);
  const creator = actorIdentity(input.workspaceSlug, input.ownedBy, input.members);
  return [
    { ...editor, role: "edited", timeText: `${shortRelativeAge(input.updatedAt, input.nowMs)} ago` },
    { ...creator, role: "created", timeText: pageCalendarDate(input.createdAt) },
  ];
}

/**
 * Which shipped empty-state illustration to use. The pane's prompts follow
 * the applied theme: dark and dark-contrast paint the dark art, and a
 * custom theme follows its palette's darkness.
 */
export function pageArtScheme(scheme: string, customDark = false): "light" | "dark" {
  if (scheme === "custom") return customDark ? "dark" : "light";
  return scheme === "dark" || scheme === "dark-contrast" ? "dark" : "light";
}

/** Bars the collapsed preview draws, one per heading (PAGE-046). */
export interface PagePreviewBar {
  level: PageHeadingLevel;
  widthPx: number;
}

export function pagePreviewBars(headings: readonly PageHeading[]): PagePreviewBar[] {
  return headings.map((heading) => ({ level: heading.level, widthPx: PAGE_PREVIEW_BAR_WIDTH_PX[heading.level] }));
}
