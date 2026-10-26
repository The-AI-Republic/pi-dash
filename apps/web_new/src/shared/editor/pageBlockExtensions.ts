// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page block nodes (parity/specs/pages.md §6.1–§6.4, PAGE-037–044).
// Callouts, inline body emoji and the two atom nodes the page body stores
// as custom elements — work-item embeds and entity references. They live
// in their own dynamic loader so Tiptap and ProseMirror stay out of the
// eager closure; each node changes only parse/render, never app data.

import { UpgradePill } from "@pidash/kit";
import { createElement } from "react";
import { createRoot } from "react-dom/client";

import { edition } from "../../core/edition/index.js";
import { requestEntityPeek } from "../shell/entityPeek.js";
import { parseEntityPeekAttributes } from "./entityPeekAttributes.js";

type Core = typeof import("@tiptap/core");
type MergeAttributes = Core["mergeAttributes"];

/** Stored element names shared with the read surface. */
export const PAGE_CALLOUT_TAG = "div";
export const PAGE_CALLOUT_BLOCK_TYPE = "callout-component";
export const PAGE_CALLOUT_BLOCK_TYPE_ATTRIBUTE = "data-block-type";
export const PAGE_ENTITY_REFERENCE_TAG = "entity-reference-component";
export const PAGE_EMOJI_TAG = "span";

/** Attributes an entity reference stores (PAGE-043, §13.2). */
export interface PageEntityReferenceAttributes {
  entity_name: string;
  entity_identifier: string;
  project_identifier: string;
  workspace_identifier: string;
  /** Actual project id, so the shell peek can select the right project. */
  project_id?: string;
  "data-label": string;
}

/** Embed attributes an edition resolver receives (PAGE-040). */
export interface PageEmbedNodeAttributes {
  id: string;
  entity_identifier: string;
  project_identifier: string;
  workspace_identifier: string;
  entity_name: string;
}

function attribute(name: string) {
  return {
    default: null,
    parseHTML: (element: HTMLElement) => element.getAttribute(name),
    renderHTML: (attributes: Record<string, unknown>) => {
      const value = attributes[name];
      return typeof value === "string" && value !== "" ? { [name]: value } : {};
    },
  };
}

/**
 * One callout: a block container stored in the form the live editor and
 * the server's allowlist already understand — `div[data-block-type=
 * "callout-component"]` — rather than a bespoke custom tag. The read
 * surface paints the same element as an aside.
 */
function createCallout(Node: Core["Node"], mergeAttributes: MergeAttributes) {
  return Node.create({
    name: "pageCallout",
    group: "block",
    content: "block+",
    defining: true,
    addAttributes() {
      return {
        "data-block-type": { default: PAGE_CALLOUT_BLOCK_TYPE },
        "data-background": attribute("data-background"),
        "data-icon-name": attribute("data-icon-name"),
        "data-icon-color": attribute("data-icon-color"),
        "data-emoji-unicode": attribute("data-emoji-unicode"),
        "data-emoji-url": attribute("data-emoji-url"),
        "data-logo-in-use": attribute("data-logo-in-use"),
      };
    },
    parseHTML() {
      return [
        { tag: `div[${PAGE_CALLOUT_BLOCK_TYPE_ATTRIBUTE}="${PAGE_CALLOUT_BLOCK_TYPE}"]` },
        { tag: 'aside[data-type="callout"]' },
      ];
    },
    renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
      return [PAGE_CALLOUT_TAG, mergeAttributes(HTMLAttributes), 0];
    },
  });
}

/**
 * Divider as the stored wrapper every other part expects: the old editor
 * serialises `div[data-type="horizontalRule"]`, the API markdown converter
 * reads that wrapper, and a bare `<hr>` is dropped on the way through.
 * StarterKit's horizontal rule renders `<hr>`, so the page set replaces it
 * with this node of the same name.
 */
function createHorizontalRule(Node: Core["Node"], mergeAttributes: MergeAttributes) {
  return Node.create({
    name: "horizontalRule",
    group: "block",
    atom: true,
    selectable: true,
    parseHTML() {
      return [{ tag: 'div[data-type="horizontalRule"]' }, { tag: "hr" }];
    },
    renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
      return ["div", mergeAttributes({ "data-type": "horizontalRule" }, HTMLAttributes), ["div", {}]];
    },
  });
}

/** One inline body emoji; the glyph is the node's text content. */
function createEmoji(Node: Core["Node"], mergeAttributes: MergeAttributes) {
  return Node.create({
    name: "pageEmoji",
    group: "inline",
    inline: true,
    atom: true,
    selectable: true,
    addAttributes() {
      return {
        glyph: {
          default: "",
          parseHTML: (element: HTMLElement) => element.textContent ?? "",
          renderHTML: () => ({}),
        },
      };
    },
    parseHTML() {
      return [{ tag: 'span[data-type="emoji"]' }];
    },
    renderHTML({
      node,
      HTMLAttributes,
    }: {
      node: { attrs: Record<string, unknown> };
      HTMLAttributes: Record<string, unknown>;
    }) {
      const glyph = typeof node.attrs["glyph"] === "string" ? node.attrs["glyph"] : "";
      return ["span", mergeAttributes({ "data-type": "emoji" }, HTMLAttributes), glyph];
    },
    renderText({ node }: { node: { attrs: Record<string, unknown> } }) {
      return typeof node.attrs["glyph"] === "string" ? node.attrs["glyph"] : "";
    },
  });
}

/**
 * The embed node is atomic and selectable as one unit. Its renderer is a
 * placeholder card: community shows the upgrade prompt and a licensed
 * edition replaces the slot (the edition owns resolution, §13.4).
 */
function createEmbed(Node: Core["Node"], mergeAttributes: MergeAttributes) {
  return Node.create({
    name: "pageEmbed",
    group: "block",
    atom: true,
    selectable: true,
    isolating: true,
    addAttributes() {
      return {
        id: attribute("id"),
        entity_identifier: attribute("entity_identifier"),
        project_identifier: attribute("project_identifier"),
        workspace_identifier: attribute("workspace_identifier"),
        entity_name: attribute("entity_name"),
      };
    },
    parseHTML() {
      return [{ tag: "issue-embed-component" }];
    },
    renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
      return ["issue-embed-component", mergeAttributes(HTMLAttributes)];
    },
    addNodeView() {
      return ({ node }: { node: { attrs: Record<string, unknown> } }) => {
        const attributes = pageEmbedAttributesOf(node.attrs);
        const dom = document.createElement("issue-embed-component");
        dom.setAttribute("contenteditable", "false");
        for (const [name, value] of Object.entries(node.attrs)) {
          if (typeof value === "string" && value !== "") dom.setAttribute(name, value);
        }
        // The edition seam (§13.4): a licensed build fills the slot with a
        // resolver; community leaves it absent and paints the upgrade card.
        const Resolver = edition.slots?.["page.embed.workItem"];
        const mount = document.createElement("div");
        mount.setAttribute("contenteditable", "false");
        dom.append(mount);
        dom.setAttribute("data-edition", Resolver === undefined ? "community" : "licensed");
        const root = createRoot(mount);
        root.render(Resolver === undefined ? communityEmbedCard(attributes) : createElement(Resolver, { attributes }));
        return { dom, destroy: () => root.unmount() };
      };
    },
  });
}

/**
 * A recognised entity reference renders as an inline preview affordance
 * and stores the normalised identifiers the shell's peek receives. The
 * node never resolves data itself (§13.2).
 */
function createEntityReference(Node: Core["Node"], mergeAttributes: MergeAttributes) {
  return Node.create({
    name: "pageEntityReference",
    group: "inline",
    inline: true,
    atom: true,
    selectable: true,
    addAttributes() {
      return {
        entity_name: attribute("entity_name"),
        entity_identifier: attribute("entity_identifier"),
        project_identifier: attribute("project_identifier"),
        workspace_identifier: attribute("workspace_identifier"),
        project_id: attribute("project_id"),
        "data-label": attribute("data-label"),
      };
    },
    parseHTML() {
      return [{ tag: PAGE_ENTITY_REFERENCE_TAG }];
    },
    renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
      return [PAGE_ENTITY_REFERENCE_TAG, mergeAttributes(HTMLAttributes)];
    },
    renderText({ node }: { node: { attrs: Record<string, unknown> } }) {
      return typeof node.attrs["data-label"] === "string" ? node.attrs["data-label"] : "";
    },
    addNodeView() {
      // A recognised reference paints as a preview card and hands the
      // entity to the shell's peek on click; the node itself stays atomic
      // and serialises the identifiers (PAGE-043, §13.2).
      return ({ node }: { node: { attrs: Record<string, unknown> } }) => {
        const label = typeof node.attrs["data-label"] === "string" ? node.attrs["data-label"] : "";
        const dom = document.createElement(PAGE_ENTITY_REFERENCE_TAG);
        dom.setAttribute("contenteditable", "false");
        for (const [name, value] of Object.entries(node.attrs)) {
          if (typeof value === "string" && value !== "") dom.setAttribute(name, value);
        }
        const card = document.createElement("span");
        card.setAttribute("data-entity-preview", "");
        card.setAttribute("role", "link");
        card.setAttribute("tabindex", "0");
        card.textContent = label;
        const open = (): void => {
          const request = parseEntityPeekAttributes(node.attrs, label);
          if (request !== null) requestEntityPeek(request);
        };
        card.addEventListener("click", open);
        card.addEventListener("keydown", (event) => {
          if (event.key !== "Enter" && event.key !== " ") return;
          event.preventDefault();
          open();
        });
        dom.append(card);
        return { dom };
      };
    },
  });
}

/** The five attributes an embed stores, narrowed from a node's attrs. */
export function pageEmbedAttributesOf(attrs: Record<string, unknown>): PageEmbedNodeAttributes {
  const stringOf = (name: string): string => (typeof attrs[name] === "string" ? (attrs[name] as string) : "");
  return {
    id: stringOf("id"),
    entity_identifier: stringOf("entity_identifier"),
    project_identifier: stringOf("project_identifier"),
    workspace_identifier: stringOf("workspace_identifier"),
    entity_name: stringOf("entity_name"),
  };
}

/** The kit's outbound-link styling, reused rather than restyled locally. */
const OUTBOUND_LINK_CLASS = "text-(--accent) underline underline-offset-2 hover:text-(--accent-hover)";

/** Community's upgrade card, rendered through kit's UpgradePill (§6.2). */
function communityEmbedCard(attributes: PageEmbedNodeAttributes) {
  return createElement(
    "div",
    { "data-embed-upgrade": "", "data-testid": "page-embed-upgrade", className: "page-embed-upgrade" },
    createElement(
      "p",
      { className: "flex items-center gap-(--space-2) font-medium" },
      "Work-item embeds are a Pro feature",
      createElement(UpgradePill)
    ),
    createElement("p", null, "Upgrade to show live work items inside a page."),
    createElement(
      "a",
      {
        href: "https://airepublic.com/pricing",
        target: "_blank",
        rel: "noopener noreferrer",
        className: OUTBOUND_LINK_CLASS,
      },
      "Upgrade",
      " ",
      attributes.entity_name
    )
  );
}

/**
 * The five page block nodes, built only after the Tiptap chunk resolves.
 */
export async function loadPageBlockExtensions(): Promise<unknown[]> {
  const [{ Node, mergeAttributes }] = await Promise.all([import("@tiptap/core")]);
  return [
    createCallout(Node, mergeAttributes),
    createHorizontalRule(Node, mergeAttributes),
    createEmoji(Node, mergeAttributes),
    createEmbed(Node, mergeAttributes),
    createEntityReference(Node, mergeAttributes),
  ];
}
