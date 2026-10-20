// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor extension sets (UI kit and editor page). Tiptap arrives only
// through dynamic import() (Stack: heavy dependencies never join the
// eager closure), so each surface exposes an async loader. The
// description set below covers the detail core (ISS-147); the composer
// adds inline images with the comment set (ISS-197), and the slices
// that own mentions, task lists and highlighting extend it further.
// The compatibility fixtures (compat.test.ts) load this same set, so
// what the tests pin is exactly what the editor runs.

import { MENTION_ENTITY, MENTION_TAG, mentionLabel } from "./mention.js";

/** Description editing: paragraphs, headings, lists, code, links. */
export async function loadDescriptionExtensions() {
  const [{ default: StarterKit }, { default: Link }] = await Promise.all([
    import("@tiptap/starter-kit"),
    import("@tiptap/extension-link"),
  ]);
  // Links stay inert while editing: a click moves the caret instead of
  // navigating away mid-edit. Serialized links keep the extension's
  // default target/rel (pinned in compat.test.ts).
  return [StarterKit, Link.configure({ openOnClick: false })];
}

/**
 * Work-item description editing: the description set plus inline images
 * and person mentions (ISS-127). Images arrive through the modal's own
 * upload control and mentions through its lookup, so this set only has
 * to parse and serialize what those controls insert.
 */
export async function loadIssueDescriptionExtensions() {
  const [{ default: Image }, { Node, mergeAttributes }, base] = await Promise.all([
    import("@tiptap/extension-image"),
    import("@tiptap/core"),
    loadDescriptionExtensions(),
  ]);
  // The mention node is built here, inside the lazy loader, because the
  // boundary check allows Tiptap only through import() (ISS-127).
  const MentionNode = Node.create({
    name: "mention",
    group: "inline",
    inline: true,
    atom: true,
    selectable: true,
    addAttributes() {
      return {
        id: { default: null },
        entity_identifier: { default: null },
        entity_name: { default: MENTION_ENTITY },
        "data-name": { default: null },
      };
    },
    parseHTML() {
      return [{ tag: MENTION_TAG }];
    },
    renderHTML({ HTMLAttributes }) {
      return [MENTION_TAG, mergeAttributes(HTMLAttributes)];
    },
    addNodeView() {
      // An atom node's inside never renders by itself, so the editable
      // surface shows the same chip the read surface does: the stored
      // element stays empty (as every client stores it) and the name is
      // painted by this view, never serialized (ISS-127).
      return ({ node }) => {
        const attrs = node.attrs as {
          "data-name"?: string | null;
          entity_identifier?: string | null;
          id?: string | null;
        };
        const dom = document.createElement(MENTION_TAG);
        dom.setAttribute("contenteditable", "false");
        for (const [name, value] of Object.entries(attrs)) {
          if (typeof value === "string" && value !== "") dom.setAttribute(name, value);
        }
        const chip = document.createElement("span");
        chip.className = "mention-chip";
        chip.textContent = mentionLabel(attrs["data-name"], attrs.entity_identifier ?? attrs.id);
        dom.append(chip);
        return { dom };
      };
    },
    renderText({ node }) {
      const attrs = node.attrs as {
        "data-name"?: string | null;
        entity_identifier?: string | null;
        id?: string | null;
      };
      return mentionLabel(attrs["data-name"], attrs.entity_identifier ?? attrs.id);
    },
  });
  // Stored bodies written by earlier clients hold images as an
  // `image-component` element; the editor reads both spellings so an
  // old body opens with its pictures (ISS-127). The element keeps its own
  // name and attributes on the way out, and a stored asset id travels in
  // `data-stored-src` so an edit never rewrites the stored id to a URL.
  const DescriptionImage = Image.extend({
    parseHTML() {
      return [{ tag: "img" }, { tag: "image-component" }];
    },
    addAttributes() {
      return {
        ...this.parent?.(),
        width: { default: null },
        height: { default: null },
        alignment: { default: null },
        // The old editor records the picture's measured ratio when it
        // resizes an image. HTML attribute names are case-insensitive, so
        // the stored `aspectRatio` and this spelling are the same
        // attribute; keeping its value through a save is what matters
        // (ISS-127).
        aspectratio: { default: null },
        "data-stored-src": {
          default: null,
          parseHTML: (element: HTMLElement) => element.getAttribute("data-stored-src"),
          renderHTML: (attributes: Record<string, unknown>) =>
            attributes["data-stored-src"] === null || attributes["data-stored-src"] === undefined
              ? {}
              : { "data-stored-src": String(attributes["data-stored-src"]) },
        },
        status: { default: null },
        id: { default: null },
      };
    },
    renderHTML({ node, HTMLAttributes }) {
      const stored = node.attrs["data-stored-src"] as string | null;
      const attrs = { ...HTMLAttributes };
      delete attrs["data-stored-src"];
      if (stored !== null) attrs.src = stored;
      return [stored === null ? "img" : "image-component", attrs];
    },
    addNodeView() {
      // The stored element keeps a legacy asset id; the surface paints the
      // resolved picture inside it while the node still serializes the
      // stored id and its own attributes (ISS-127).
      return ({ node }) => {
        const attrs = node.attrs as Record<string, unknown>;
        const stored = attrs["data-stored-src"];
        const dom = document.createElement("image-component");
        dom.setAttribute("contenteditable", "false");
        for (const [name, value] of Object.entries(attrs)) {
          if (name === "src" || name === "data-stored-src" || typeof value !== "string" || value === "") continue;
          dom.setAttribute(name, value);
        }
        const image = document.createElement("img");
        image.setAttribute("src", typeof attrs["src"] === "string" ? attrs["src"] : "");
        for (const name of ["alt", "title", "width", "height", "aspectratio"]) {
          const value = attrs[name];
          if (typeof value === "string" && value !== "") image.setAttribute(name, value);
        }
        dom.append(image);
        if (typeof stored === "string") dom.setAttribute("data-stored-src", stored);
        return { dom };
      };
    },
  }).configure({ inline: false, allowBase64: false });
  return [...base, DescriptionImage, MentionNode];
}

/** Comment editing: the description set plus inline images (ISS-197). */
export async function loadCommentExtensions() {
  const [{ default: Image }, base] = await Promise.all([
    import("@tiptap/extension-image"),
    loadDescriptionExtensions(),
  ]);
  return [...base, Image.configure({ inline: false, allowBase64: false })];
}
