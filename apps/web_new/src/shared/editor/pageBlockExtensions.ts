// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page block nodes (parity/specs/pages.md §6.1–§6.4, PAGE-037–044).
// Callouts, inline body emoji and the two atom nodes the page body stores
// as custom elements — work-item embeds and entity references. They live
// in their own dynamic loader so Tiptap and ProseMirror stay out of the
// eager closure; each node changes only parse/render, never app data.

type Core = typeof import("@tiptap/core");
type MergeAttributes = Core["mergeAttributes"];

/** Stored element names shared with the read surface. */
export const PAGE_CALLOUT_TAG = "callout-component";
export const PAGE_ENTITY_REFERENCE_TAG = "entity-reference-component";
export const PAGE_EMOJI_TAG = "span";

/** Attributes an entity reference stores (PAGE-043, §13.2). */
export interface PageEntityReferenceAttributes {
  entity_name: string;
  entity_identifier: string;
  project_identifier: string;
  workspace_identifier: string;
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

/** One callout: a block container that serialises as a stored element. */
function createCallout(Node: Core["Node"], mergeAttributes: MergeAttributes) {
  return Node.create({
    name: "pageCallout",
    group: "block",
    content: "block+",
    defining: true,
    parseHTML() {
      return [{ tag: PAGE_CALLOUT_TAG }, { tag: 'aside[data-type="callout"]' }];
    },
    renderHTML({ HTMLAttributes }: { HTMLAttributes: Record<string, unknown> }) {
      return [PAGE_CALLOUT_TAG, mergeAttributes({ "data-type": "callout" }, HTMLAttributes), 0];
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
        const dom = document.createElement("issue-embed-component");
        dom.setAttribute("contenteditable", "false");
        dom.setAttribute("data-edition", "community");
        for (const [name, value] of Object.entries(node.attrs)) {
          if (typeof value === "string" && value !== "") dom.setAttribute(name, value);
        }
        const card = document.createElement("div");
        card.setAttribute("data-embed-upgrade", "");
        const title = document.createElement("p");
        title.textContent = "Work-item embeds are a Pro feature";
        const body = document.createElement("p");
        body.textContent = "Upgrade to show live work items inside a page.";
        const link = document.createElement("a");
        link.setAttribute("href", "https://airepublic.com/pricing");
        link.setAttribute("target", "_blank");
        link.setAttribute("rel", "noreferrer");
        link.textContent = "Upgrade";
        card.append(title, body, link);
        dom.append(card);
        return { dom };
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
        dom.append(card);
        return { dom };
      };
    },
  });
}

/**
 * The four page block nodes, built only after the Tiptap chunk resolves.
 */
export async function loadPageBlockExtensions(): Promise<unknown[]> {
  const [{ Node, mergeAttributes }] = await Promise.all([import("@tiptap/core")]);
  return [
    createCallout(Node, mergeAttributes),
    createEmoji(Node, mergeAttributes),
    createEmbed(Node, mergeAttributes),
    createEntityReference(Node, mergeAttributes),
  ];
}
