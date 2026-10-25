// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page extension set (parity/specs/pages.md §10.1, PAGE-028–032): the
// description core plus underline, paragraph/heading alignment and
// checkable lists. Stored page markup must survive parse and serialize,
// because saving and reloading is how a formatting choice persists.

import type { AnyExtension } from "@tiptap/core";
import { describe, expect, it } from "vitest";

import { loadDescriptionExtensions } from "./extensions.js";
import { loadPageExtensions } from "./pageExtensions.js";

interface Fixture {
  name: string;
  html: string;
  text: string;
  keeps: string[];
}

const FIXTURES: Fixture[] = [
  { name: "plain text", html: "<p>Plain.</p>", text: "Plain.", keeps: ["<p>"] },
  {
    name: "heading levels",
    html: "<h1>One</h1><h2>Two</h2><h6>Six</h6>",
    text: "OneTwoSix",
    keeps: ["<h1>", "<h2>", "<h6>"],
  },
  {
    name: "text marks",
    html: "<p><strong>bold</strong> <em>italic</em> <u>underline</u> <s>strike</s> <code>code</code></p>",
    text: "bold italic underline strike code",
    keeps: ["<strong>", "<em>", "<u>", "<s>", "<code>"],
  },
  {
    name: "alignment",
    html: '<p style="text-align: center">middle</p><h2 style="text-align: right">right</h2>',
    text: "middleright",
    keeps: ["text-align: center", "text-align: right"],
  },
  { name: "ordered list", html: "<ol><li>one</li></ol>", text: "one", keeps: ["<ol>"] },
  { name: "bullet list", html: "<ul><li>one</li></ul>", text: "one", keeps: ["<ul>"] },
  {
    name: "checkable list",
    html: '<ul data-type="taskList"><li data-type="taskItem" data-checked="true"><label><input type="checkbox" checked="checked"><span></span></label><div><p>done</p></div></li></ul>',
    text: "done",
    keeps: ['data-type="taskList"', 'data-type="taskItem"', 'data-checked="true"', 'type="checkbox"'],
  },
  { name: "quote", html: "<blockquote><p>quoted</p></blockquote>", text: "quoted", keeps: ["<blockquote>"] },
];

function textOf(html: string): string {
  return new DOMParser().parseFromString(html, "text/html").body.textContent ?? "";
}

describe("page extension set", () => {
  it("adds underline, alignment and checkable lists to the description core", async () => {
    const base = (await loadDescriptionExtensions()) as unknown as AnyExtension[];
    const page = await loadPageExtensions();
    expect(page).toHaveLength(base.length + 4);
    const names = page.map((extension) => extension.name);
    expect(names).toContain("underline");
    expect(names).toContain("textAlign");
    expect(names).toContain("taskList");
    expect(names).toContain("taskItem");
    expect(names).toContain("starterKit");
  });

  it.each(FIXTURES.map((fixture) => [fixture.name, fixture] as const))(
    "%s survives parse and serialize",
    async (_name, fixture) => {
      const { Editor } = await import("@tiptap/core");
      const editor = new Editor({ extensions: await loadPageExtensions(), content: fixture.html });
      try {
        const serialized = editor.getHTML();
        expect(textOf(serialized)).toBe(fixture.text);
        for (const tag of fixture.keeps) expect(serialized).toContain(tag);
      } finally {
        editor.destroy();
      }
    }
  );

  it("only offers left, centre and right alignment", async () => {
    const { Editor } = await import("@tiptap/core");
    const editor = new Editor({
      extensions: await loadPageExtensions(),
      content: '<p style="text-align: justify">justified</p>',
    });
    try {
      // Plain text is left-aligned; an unsupported alignment is not kept.
      expect(editor.getHTML()).not.toContain("justify");
    } finally {
      editor.destroy();
    }
  });

  it("restyles a paragraph as a heading and back", async () => {
    const { Editor } = await import("@tiptap/core");
    const editor = new Editor({ extensions: await loadPageExtensions(), content: "<p>words</p>" });
    try {
      editor.commands.setTextSelection(1);
      editor.commands.toggleHeading({ level: 2 });
      expect(editor.getHTML()).toContain("<h2");
      editor.commands.setParagraph();
      expect(editor.getHTML()).toContain("<p>words</p>");
    } finally {
      editor.destroy();
    }
  });
});
