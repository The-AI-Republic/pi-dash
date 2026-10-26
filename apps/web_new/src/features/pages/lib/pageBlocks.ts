// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Block catalogue and document markup (parity/specs/pages.md §6.1–§6.4,
// PAGE-037–044). One ordered list describes everything the slash palette
// offers — the typography, lists, table, quote, fenced code, image,
// callout, divider, emoji and the two colour rows — so the palette, the
// drag-handle menu and the tests read the same catalogue. This module is
// pure: it names entries and derives markup, while the editor adapter
// executes them.

import type { PageBlockKind } from "../../../shared/editor/pageFormatting.js";
import { PAGE_COLOR_CHOICES, type PageColorKind } from "./pageColors.js";

/** A typography entry the block catalogue offers. */
export type PageCatalogBlockKind =
  | "text"
  | "heading1"
  | "heading2"
  | "heading3"
  | "heading4"
  | "heading5"
  | "heading6"
  | "numbered"
  | "bulleted"
  | "todo"
  | "table"
  | "quote"
  | "code"
  | "image"
  | "callout"
  | "divider"
  | "emoji"
  | "embed"
  | "entity";

/** What choosing one catalogue entry does. */
export type PageCatalogAction =
  | { type: "block"; block: PageCatalogBlockKind }
  | { type: "color"; kind: PageColorKind; key: string | null }
  | { type: "command"; command: PageSlashCommand };

/** Inline commands a slash query exposes beside the structural catalogue. */
export type PageSlashCommand =
  | "inlineCode"
  | "underline"
  | "alignLeft"
  | "alignCenter"
  | "alignRight"
  | "link"
  | "mention";

/** One slash-palette entry, in the order the oracle reads it. */
export interface PageCatalogEntry {
  /** Visible label; colour rows deliberately repeat names. */
  label: string;
  /** Group heading the entry sits under. */
  group: string;
  action: PageCatalogAction;
}

/** Group headings, in palette order. */
export const PAGE_CATALOG_GROUPS = {
  basic: "Basic blocks",
  media: "Media",
  color: "Colors",
} as const;

const BASIC: readonly PageCatalogEntry[] = [
  { label: "Text", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "text" } },
  { label: "Heading 1", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "heading1" } },
  { label: "Heading 2", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "heading2" } },
  { label: "Heading 3", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "heading3" } },
  { label: "Heading 4", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "heading4" } },
  { label: "Heading 5", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "heading5" } },
  { label: "Heading 6", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "heading6" } },
  { label: "Numbered list", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "numbered" } },
  { label: "Bulleted list", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "bulleted" } },
  { label: "To-do list", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "todo" } },
  { label: "Table", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "table" } },
  { label: "Quote", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "quote" } },
  { label: "Code", group: PAGE_CATALOG_GROUPS.basic, action: { type: "block", block: "code" } },
  { label: "Image", group: PAGE_CATALOG_GROUPS.media, action: { type: "block", block: "image" } },
  { label: "Callout", group: PAGE_CATALOG_GROUPS.media, action: { type: "block", block: "callout" } },
  { label: "Divider", group: PAGE_CATALOG_GROUPS.media, action: { type: "block", block: "divider" } },
  { label: "Emoji", group: PAGE_CATALOG_GROUPS.media, action: { type: "block", block: "emoji" } },
  // Production producers for PAGE-040/043. The pinned oracle catalogue stays
  // at 35 rows; these two commands are reachable by their slash queries and
  // are appended outside the cross-checked list below.
];

/** The two colour rows, in palette order (letter row first). */
function colorEntries(): PageCatalogEntry[] {
  const rows: PageCatalogEntry[] = [];
  for (const kind of ["text", "background"] as const) {
    rows.push({
      label: kind === "text" ? "Default" : "Default background",
      group: PAGE_CATALOG_GROUPS.color,
      action: { type: "color", kind, key: null },
    });
    for (const choice of PAGE_COLOR_CHOICES) {
      rows.push({
        label: choice.label,
        group: PAGE_CATALOG_GROUPS.color,
        action: { type: "color", kind, key: choice.key },
      });
    }
  }
  return rows;
}

/**
 * The complete palette in reading order. The old editor pins 35 entries;
 * the cloud edition contributes none (PAGE-078), so this list is the
 * whole community catalogue.
 */
export const PAGE_SLASH_ENTRIES: readonly PageCatalogEntry[] = [...BASIC, ...colorEntries()];

/**
 * Commands the body interaction layer opens as pickers. They are not part of
 * the oracle's 35-entry catalogue (PAGE-078 pins that list), but a typed
 * `/embed` or `/entity` resolves to them so a user can produce the atomic
 * PAGE-040/043 nodes. The parser is deliberately separate from the palette's
 * structural catalogue.
 */
export const PAGE_PICKER_COMMANDS: readonly PageCatalogEntry[] = [
  { label: "Embed work item", group: PAGE_CATALOG_GROUPS.media, action: { type: "block", block: "embed" } },
  { label: "Entity reference", group: PAGE_CATALOG_GROUPS.media, action: { type: "block", block: "entity" } },
];

/**
 * Inline commands reachable by typing their names. They stay out of the
 * unqueried palette so the pinned community catalogue remains exactly the
 * structural rows plus colours (PAGE-037/PAGE-078); each one drives the
 * same editor command the toolbar or `@` trigger already exposes.
 */
export const PAGE_QUERY_COMMANDS: readonly PageCatalogEntry[] = [
  { label: "Inline code", group: PAGE_CATALOG_GROUPS.basic, action: { type: "command", command: "inlineCode" } },
  { label: "Underline", group: PAGE_CATALOG_GROUPS.basic, action: { type: "command", command: "underline" } },
  { label: "Left align", group: PAGE_CATALOG_GROUPS.basic, action: { type: "command", command: "alignLeft" } },
  { label: "Center align", group: PAGE_CATALOG_GROUPS.basic, action: { type: "command", command: "alignCenter" } },
  { label: "Right align", group: PAGE_CATALOG_GROUPS.basic, action: { type: "command", command: "alignRight" } },
  { label: "Link", group: PAGE_CATALOG_GROUPS.basic, action: { type: "command", command: "link" } },
  { label: "Mention", group: PAGE_CATALOG_GROUPS.basic, action: { type: "command", command: "mention" } },
];

/** Commands matching a typed slash query, including picker producers. */
export function filterPickerCommands(query: string): PageCatalogEntry[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [];
  return PAGE_PICKER_COMMANDS.filter((entry) => entry.label.toLowerCase().includes(needle));
}

/** Catalogue labels, in order, for the palette's flat list. */
export function pageSlashLabels(): string[] {
  return PAGE_SLASH_ENTRIES.map((entry) => entry.label);
}

/** Entries matching a typed slash query, in catalogue order. */
export function filterSlashEntries(query: string): PageCatalogEntry[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...PAGE_SLASH_ENTRIES];
  const structural = PAGE_SLASH_ENTRIES.filter((entry) => entry.label.toLowerCase().includes(needle));
  const producers = PAGE_PICKER_COMMANDS.filter((entry) => entry.label.toLowerCase().includes(needle));
  const commands = PAGE_QUERY_COMMANDS.filter((entry) => entry.label.toLowerCase().includes(needle));
  return [...structural, ...producers, ...commands];
}

/** Drag-handle menu entries, in pinned order (spec §6.1 move actions). */
export const PAGE_BLOCK_MENU_LABELS = ["Delete", "Duplicate", "Move up", "Move down"] as const;
export type PageBlockMenuAction = (typeof PAGE_BLOCK_MENU_LABELS)[number];

/** Placeholder glyphs the body emoji picker offers, in order. */
export const PAGE_BODY_EMOJIS: readonly string[] = [
  "😀",
  "😄",
  "😊",
  "🙂",
  "😉",
  "😍",
  "🤩",
  "😎",
  "🤔",
  "😅",
  "😂",
  "🙃",
  "😴",
  "🥳",
  "😇",
  "🤯",
  "👍",
  "👏",
  "🙌",
  "🙏",
  "💪",
  "👀",
  "🔥",
  "✨",
  "🎉",
  "🚀",
  "✅",
  "❌",
  "⚠️",
  "💡",
  "📌",
  "📝",
];

/** The stored `data-type` an inline body emoji carries. */
export const PAGE_EMOJI_DATA_TYPE = "emoji";

/** Attributes a work-item embed stores (PAGE-040). */
export interface PageEmbedAttributes {
  id: string;
  entity_identifier: string;
  project_identifier: string;
  workspace_identifier: string;
  entity_name: string;
}

/** The stored element an embed serialises to (atomic block). */
export const PAGE_EMBED_TAG = "issue-embed-component";

/** Upgrade address the community embed card links to (§13.4). */
export const PAGE_EMBED_UPGRADE_HREF = "https://airepublic.com/pricing";
export const PAGE_EMBED_UPGRADE_LABEL = "Upgrade";
export const PAGE_EMBED_COMMUNITY_TITLE = "Work-item embeds are a Pro feature";
export const PAGE_EMBED_COMMUNITY_BODY = "Upgrade to show live work items inside a page.";

/** HTML that stores one embed, used by paste handling and tests. */
export function embedHtml(attributes: PageEmbedAttributes): string {
  return `<${PAGE_EMBED_TAG} id="${escapeAttribute(attributes.id)}" entity_identifier="${escapeAttribute(attributes.entity_identifier)}" project_identifier="${escapeAttribute(attributes.project_identifier)}" workspace_identifier="${escapeAttribute(attributes.workspace_identifier)}" entity_name="${escapeAttribute(attributes.entity_name)}"></${PAGE_EMBED_TAG}>`;
}

function escapeAttribute(value: string): string {
  return value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** Markdown placeholder for a body emoji (copy/export keeps the glyph). */
export function emojiHtml(glyph: string): string {
  return `<span data-type="${PAGE_EMOJI_DATA_TYPE}">${glyph}</span>`;
}

/** The typography block a catalogue entry restyles the caret to. */
export function pageCatalogBlockKind(block: PageCatalogBlockKind): PageBlockKind | null {
  switch (block) {
    case "text":
      return "paragraph";
    case "heading1":
    case "heading2":
    case "heading3":
    case "heading4":
    case "heading5":
    case "heading6":
      return block;
    default:
      return null;
  }
}

/** The live slash query before the caret, or null when none is active. */
export function slashQueryAt(textBeforeCaret: string): string | null {
  const slash = textBeforeCaret.lastIndexOf("/");
  if (slash === -1) return null;
  if (slash > 0 && !/\s/.test(textBeforeCaret.slice(slash - 1, slash))) return null;
  const tail = textBeforeCaret.slice(slash + 1);
  if (/\s/.test(tail)) return null;
  return tail;
}
