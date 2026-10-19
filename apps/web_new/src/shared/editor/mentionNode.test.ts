// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Mention node (parity/specs/issues.md §11, ISS-127). The node is built
// inside the description loader so Tiptap never reaches the eager
// closure; these cases drive the real loaded node's parse, render and
// text pair, plus a stored-markup round trip through the real editor.

import type { AnyExtension } from "@tiptap/core";
import { describe, expect, it } from "vitest";

import { MENTION_ENTITY, MENTION_TAG, mentionAttributes } from "./mention.js";
import { loadIssueDescriptionExtensions } from "./extensions.js";

type LoadedNode = Awaited<ReturnType<typeof loadIssueDescriptionExtensions>>[number];

async function loadMentionNode(): Promise<LoadedNode> {
  const extensions = await loadIssueDescriptionExtensions();
  const node = extensions.find((extension) => extension.name === "mention");
  if (node === undefined) throw new Error("the description set carries no mention node");
  return node;
}

describe("MentionNode", () => {
  it("is an atomic inline node named mention", async () => {
    const node = await loadMentionNode();
    expect(node.config.name).toBe("mention");
    expect(node.config.group).toBe("inline");
    expect(node.config.inline).toBe(true);
    expect(node.config.atom).toBe(true);
  });

  it("parses the shared mention-component element", async () => {
    const node = await loadMentionNode();
    expect(node.config.parseHTML?.call({} as never)).toEqual([{ tag: MENTION_TAG }]);
    expect(MENTION_TAG).toBe("mention-component");
  });

  it("renders the stored attributes onto that element", async () => {
    const node = await loadMentionNode();
    const attributes = mentionAttributes({ id: "u-1", name: "Ada" });
    const rendered = node.config.renderHTML?.call({} as never, {
      node: {} as never,
      HTMLAttributes: attributes,
    });
    expect(rendered).toEqual([MENTION_TAG, attributes]);
  });

  it("renders a readable @identifier as its text", async () => {
    const node = await loadMentionNode();
    const text = node.config.renderText?.call(
      {} as never,
      {
        node: { attrs: { entity_identifier: "u-1" } },
      } as never
    );
    expect(text).toBe("@u-1");
  });

  it("falls back to the plain id when the identifier is missing", async () => {
    const node = await loadMentionNode();
    const text = node.config.renderText?.call({} as never, { node: { attrs: { id: "u-9" } } } as never);
    expect(text).toBe("@u-9");
  });

  it("keeps a stored mention through a real parse and serialize", async () => {
    const { Editor } = await import("@tiptap/core");
    // The loader's inferred union trips the Editor options variance; the
    // runtime values are exactly what the editor runs (compat.test.ts).
    const extensions = (await loadIssueDescriptionExtensions()) as unknown as AnyExtension[];
    const stored = `<p>Thanks <${MENTION_TAG} id="u-1" entity_identifier="u-1" entity_name="${MENTION_ENTITY}"></${MENTION_TAG}>.</p>`;
    const editor = new Editor({ extensions, content: stored });
    try {
      const serialized = editor.getHTML();
      expect(serialized).toContain(MENTION_TAG);
      expect(serialized).toContain('entity_identifier="u-1"');
      expect(serialized).toContain("Thanks");
    } finally {
      editor.destroy();
    }
  });
});
