// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor compatibility fixtures (UI kit and editor page; Quality gates).
// Stored description HTML written by any client must survive a trip
// through this slice's extension set: parse it, serialize it back, and
// the text plus the supported structure come out intact. The samples
// below are representative shapes, written for this suite.

import type { AnyExtension } from "@tiptap/core";
import { describe, expect, it } from "vitest";

import { loadCommentExtensions, loadDescriptionExtensions } from "./extensions.js";
import { prepareEditorHtml } from "./sanitize.js";

interface Fixture {
  name: string;
  html: string;
  text: string;
  keeps: string[];
}

const FIXTURES: Fixture[] = [
  {
    name: "plain paragraph",
    html: "<p>Plain paragraph.</p>",
    text: "Plain paragraph.",
    keeps: ["<p>"],
  },
  {
    name: "heading with inline marks",
    html: "<h2>Heading</h2><p>Body with <strong>bold</strong>, <em>italic</em>, <s>strike</s> and <code>code</code>.</p>",
    text: "HeadingBody with bold, italic, strike and code.",
    keeps: ["<h2>", "<strong>", "<em>", "<s>", "<code>"],
  },
  {
    name: "bullet and ordered lists",
    html: "<ul><li>one</li><li>two</li></ul><ol><li>first</li></ol>",
    text: "onetwofirst",
    keeps: ["<ul>", "<ol>", "<li>"],
  },
  {
    name: "quote and code block",
    html: "<blockquote><p>quoted</p></blockquote><pre><code>const x = 1;</code></pre>",
    text: "quotedconst x = 1;",
    keeps: ["<blockquote>", "<pre>", "<code>"],
  },
  {
    name: "link",
    html: '<p>See the <a href="https://example.com">example</a> for details.</p>',
    text: "See the example for details.",
    keeps: ['<a target="_blank" rel="noopener noreferrer nofollow" href="https://example.com">'],
  },
  {
    name: "empty document",
    html: "<p></p>",
    text: "",
    keeps: [],
  },
];

function textOf(html: string): string {
  return new DOMParser().parseFromString(html, "text/html").body.textContent ?? "";
}

describe("stored description round-trip", () => {
  it.each(FIXTURES.map((fixture) => [fixture.name, fixture] as const))(
    "%s survives parse and serialize",
    async (_name, fixture) => {
      const { Editor } = await import("@tiptap/core");
      // The loader's inferred extension union trips the Editor options
      // variance; the runtime values are exactly what the editor runs.
      const extensions = (await loadDescriptionExtensions()) as unknown as AnyExtension[];
      const editor = new Editor({ extensions, content: fixture.html });
      try {
        const serialized = editor.getHTML();
        expect(textOf(serialized)).toBe(fixture.text);
        for (const tag of fixture.keeps) {
          expect(serialized).toContain(tag);
        }
      } finally {
        editor.destroy();
      }
    }
  );
});

describe("comment editor extensions", () => {
  it("adds the stored-image and mention nodes to the description set", async () => {
    const description = (await loadDescriptionExtensions()) as unknown as AnyExtension[];
    const comment = (await loadCommentExtensions()) as unknown as AnyExtension[];
    expect(comment).toHaveLength(description.length + 2);
    const names = comment.map((extension) => extension.name).sort();
    expect(names).toContain("image");
    expect(names).toContain("mention");
  });

  it("keeps a plain image a plain image", async () => {
    const { Editor } = await import("@tiptap/core");
    const extensions = (await loadCommentExtensions()) as unknown as AnyExtension[];
    const editor = new Editor({
      extensions,
      content: '<h2>Shot</h2><p>Look</p><img src="https://cdn.test/shot.png">',
    });
    try {
      const serialized = editor.getHTML();
      expect(serialized).toContain('<img src="https://cdn.test/shot.png">');
      expect(serialized).not.toContain("image-component");
      expect(textOf(serialized)).toBe("ShotLook");
    } finally {
      editor.destroy();
    }
  });

  it("round-trips a stored image and a stored mention (CMT-009/019)", async () => {
    const { Editor } = await import("@tiptap/core");
    const extensions = (await loadCommentExtensions()) as unknown as AnyExtension[];
    // The edit form resolves bare asset ids for display before the
    // editor parses, so the stored id survives the round trip.
    const content = prepareEditorHtml(
      '<p>Thanks <mention-component id="u-1" entity_identifier="u-1" entity_name="user_mention" data-name="Ada"></mention-component></p>' +
        '<image-component src="asset-9" status="uploaded"></image-component>',
      (src) => `/api/assets/v2/workspaces/acme/projects/p-1/${src}/`
    );
    const editor = new Editor({ extensions, content });
    try {
      const serialized = editor.getHTML();
      expect(serialized).toContain("mention-component");
      expect(serialized).toContain('entity_identifier="u-1"');
      expect(serialized).toContain("image-component");
      expect(serialized).toContain('src="asset-9"');
      expect(serialized).toContain('status="uploaded"');
      expect(textOf(serialized)).toContain("Thanks");
    } finally {
      editor.destroy();
    }
  });

  it("drops the mention node from the serialized body once it is removed (CMT-021)", async () => {
    const { Editor } = await import("@tiptap/core");
    const extensions = (await loadCommentExtensions()) as unknown as AnyExtension[];
    const editor = new Editor({
      extensions,
      content:
        '<p>draft note <mention-component id="u-1" entity_identifier="u-1" entity_name="user_mention" data-name="Ada"></mention-component></p>',
    });
    try {
      expect(editor.getHTML()).toContain("mention-component");
      // Locate the parsed mention atom and delete it, as the edit
      // form's draft does once the user removes the chip; the save
      // then stores a body with no mention left for the server to keep.
      let range: { from: number; to: number } | null = null;
      editor.state.doc.descendants((node, pos) => {
        if (node.type.name === "mention") range = { from: pos, to: pos + node.nodeSize };
        return range === null;
      });
      expect(range).not.toBeNull();
      if (range === null) throw new Error("expected a parsed mention node");
      editor.chain().focus().deleteRange(range).run();
      const serialized = editor.getHTML();
      expect(serialized).not.toContain("mention-component");
      expect(serialized).not.toContain('entity_identifier="u-1"');
      expect(textOf(serialized)).toContain("draft note");
    } finally {
      editor.destroy();
    }
  });
});
