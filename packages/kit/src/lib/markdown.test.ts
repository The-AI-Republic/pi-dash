// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { markdownText, parseInline, parseMarkdown, safeHref, type MarkdownBlock } from "./markdown";

function inlineText(source: string): string {
  return markdownText([{ type: "paragraph", spans: parseInline(source) }]);
}

describe("safeHref", () => {
  it("keeps relative, fragment and browsing targets", () => {
    expect(safeHref("/runs/1")).toBe("/runs/1");
    expect(safeHref("#top")).toBe("#top");
    expect(safeHref("https://example.com/a")).toBe("https://example.com/a");
    expect(safeHref("HTTPS://EXAMPLE.COM")).toBe("HTTPS://EXAMPLE.COM");
    expect(safeHref("mailto:a@b.c")).toBe("mailto:a@b.c");
    expect(safeHref("docs/page")).toBe("docs/page");
  });

  it("refuses executable and local schemes, and blanks", () => {
    expect(safeHref("javascript:alert(1)")).toBeNull();
    expect(safeHref("data:text/html,x")).toBeNull();
    expect(safeHref("file:///etc/passwd")).toBeNull();
    expect(safeHref("   ")).toBeNull();
  });
});

describe("parseMarkdown blocks", () => {
  it("reads paragraphs, separating them on blank lines and wrapping soft lines", () => {
    const blocks = parseMarkdown("one line\ncontinued\n\nsecond");
    expect(blocks).toEqual([
      { type: "paragraph", spans: [{ type: "text", text: "one line continued" }] },
      { type: "paragraph", spans: [{ type: "text", text: "second" }] },
    ]);
  });

  it("keeps a hard break as a break span", () => {
    const spans = parseInline("first  \nsecond");
    expect(spans).toEqual([{ type: "text", text: "first  " }, { type: "break" }, { type: "text", text: "second" }]);
  });

  it("reads ATX and setext headings", () => {
    expect(parseMarkdown("## Title")).toEqual([
      { type: "heading", level: 2, spans: [{ type: "text", text: "Title" }] },
    ]);
    expect(parseMarkdown("Title\n=====")).toEqual([
      { type: "heading", level: 1, spans: [{ type: "text", text: "Title" }] },
    ]);
    expect(parseMarkdown("Sub\n---")).toEqual([{ type: "heading", level: 2, spans: [{ type: "text", text: "Sub" }] }]);
  });

  it("reads fenced and indented code literally", () => {
    expect(parseMarkdown("```ts\nconst a = 1;\n```")).toEqual([{ type: "code", language: "ts", text: "const a = 1;" }]);
    expect(parseMarkdown("~~~~\nraw *text*\n~~~~")).toEqual([{ type: "code", language: "", text: "raw *text*" }]);
    expect(parseMarkdown("    indented\n    code")).toEqual([{ type: "code", language: "", text: "indented\ncode" }]);
  });

  it("reads rules, quotes and ordered/unordered lists", () => {
    expect(parseMarkdown("***")).toEqual([{ type: "rule" }]);
    expect(parseMarkdown("> quoted\n> more")).toEqual([
      { type: "quote", blocks: [{ type: "paragraph", spans: [{ type: "text", text: "quoted more" }] }] },
    ]);
    expect(parseMarkdown("- a\n- b")).toEqual([
      {
        type: "list",
        ordered: false,
        start: 1,
        items: [
          { blocks: [{ type: "paragraph", spans: [{ type: "text", text: "a" }] }] },
          { blocks: [{ type: "paragraph", spans: [{ type: "text", text: "b" }] }] },
        ],
      },
    ]);
    expect(parseMarkdown("3. c\n4. d")).toMatchObject([{ type: "list", ordered: true, start: 3 }]);
  });

  it("keeps nested list content inside its entry", () => {
    const blocks = parseMarkdown("- outer\n  - inner\n- next");
    const list = blocks[0] as Extract<MarkdownBlock, { type: "list" }>;
    expect(list.items).toHaveLength(2);
    expect(list.items[0]!.blocks).toHaveLength(2);
    expect(list.items[0]!.blocks[1]).toMatchObject({ type: "list" });
    expect(markdownText(blocks)).toBe("outer\ninner\nnext");
  });

  it("keeps a paragraph line inside a list entry", () => {
    const blocks = parseMarkdown("- first\n  continued\n- second");
    expect(markdownText(blocks)).toBe("first continued\nsecond");
  });

  it("ends a list at a blank line followed by ordinary text", () => {
    const blocks = parseMarkdown("- item\n\ntext");
    expect(blocks).toHaveLength(2);
    expect(blocks[1]).toMatchObject({ type: "paragraph" });
  });

  it("ends a list when the next item is less indented than the list", () => {
    const blocks = parseMarkdown("  - item\n- outside");
    expect(blocks).toHaveLength(2);
    expect((blocks[1] as Extract<MarkdownBlock, { type: "list" }>).items).toHaveLength(1);
  });

  it("stops a paragraph at a following block marker", () => {
    const blocks = parseMarkdown("para\n## head\n- item\n> quote\n---");
    expect(blocks.map((block) => block.type)).toEqual(["paragraph", "heading", "list", "quote", "rule"]);
  });

  it("reads a fenced block that never closes, and a fence at the document end", () => {
    expect(parseMarkdown("```\nopen")).toEqual([{ type: "code", language: "", text: "open" }]);
    expect(parseMarkdown("```\n\n")).toEqual([{ type: "code", language: "", text: "\n" }]);
  });

  it("reads a quote that runs to the end and one whose marker has no space", () => {
    expect(parseMarkdown(">a\n>b")).toEqual([
      { type: "quote", blocks: [{ type: "paragraph", spans: [{ type: "text", text: "a b" }] }] },
    ]);
    expect(parseMarkdown("> A\n>\n> B")).toMatchObject([{ type: "quote" }]);
  });

  it("keeps an indented code block that runs to the end", () => {
    expect(parseMarkdown("    one\n    two\n")).toEqual([{ type: "code", language: "", text: "one\ntwo" }]);
    expect(parseMarkdown("\tone")).toEqual([{ type: "code", language: "", text: "one" }]);
  });

  it("continues a list across a blank line when the entry is indented deeper", () => {
    const blocks = parseMarkdown("- one\n\n    - nested");
    const list = blocks[0] as Extract<MarkdownBlock, { type: "list" }>;
    expect(list.items).toHaveLength(1);
    expect(list.items[0]!.blocks.map((block) => block.type)).toEqual(["paragraph", "list"]);
  });

  it("continues a list across a blank line before its next entry", () => {
    const blocks = parseMarkdown("- one\n\n- two");
    const list = blocks[0] as Extract<MarkdownBlock, { type: "list" }>;
    expect(list.items).toHaveLength(2);
  });

  it("ends a list at a blank line at the document end", () => {
    expect(markdownText(parseMarkdown("- one\n"))).toBe("one");
  });

  it("keeps a lazy list continuation as text of the entry", () => {
    // A line indented under the marker but not enough to be nested stays
    // with the entry it follows.
    expect(markdownText(parseMarkdown("- one\n lazy"))).toBe("one\nlazy");
  });

  it("stops a paragraph before a setext underline is consumed as a rule", () => {
    expect(parseMarkdown("para\n- - -")).toMatchObject([{ type: "paragraph" }, { type: "rule" }]);
  });

  it("reads an entry whose marker has no text", () => {
    expect(markdownText(parseMarkdown("-\n- b"))).toBe("\nb");
  });

  it("joins wrapped lines into one paragraph", () => {
    expect(parseMarkdown("first\nsecond\n\nthird\nfourth")).toEqual([
      { type: "paragraph", spans: [{ type: "text", text: "first second" }] },
      { type: "paragraph", spans: [{ type: "text", text: "third fourth" }] },
    ]);
  });

  it("keeps a hard break at the end of a wrapped paragraph line", () => {
    expect(inlineText("first  \nsecond")).toBe("first  \nsecond");
  });

  it("treats an empty list entry as blank", () => {
    const blocks = parseMarkdown("- \n- b");
    expect(markdownText(blocks)).toBe("\nb");
  });
});

describe("parseInline", () => {
  it("reads strong, emphasis, strike and code", () => {
    expect(parseInline("**bold** _em_ ~~gone~~ `code`")).toEqual([
      { type: "strong", spans: [{ type: "text", text: "bold" }] },
      { type: "text", text: " " },
      { type: "emphasis", spans: [{ type: "text", text: "em" }] },
      { type: "text", text: " " },
      { type: "strike", spans: [{ type: "text", text: "gone" }] },
      { type: "text", text: " " },
      { type: "code", text: "code" },
    ]);
  });

  it("keeps code span content literal and strips one padding space", () => {
    expect(parseInline("`**not bold**`")).toEqual([{ type: "code", text: "**not bold**" }]);
    expect(parseInline("`` a`b ``")).toEqual([{ type: "code", text: "a`b" }]);
  });

  it("reads links, refusing unsafe targets", () => {
    expect(parseInline('[docs](https://x.test/a "title")')).toEqual([
      {
        type: "link",
        href: "https://x.test/a",
        spans: [{ type: "text", text: "docs" }],
      },
    ]);
    expect(parseInline("[x](javascript:alert(1))")).toEqual([{ type: "text", text: "[x]" }]);
  });

  it("reads an image as its alt text", () => {
    expect(parseInline("![diagram](/a.png) tail")).toEqual([{ type: "text", text: "diagram tail" }]);
  });

  it("honours backslash escapes", () => {
    expect(parseInline("\\*literal\\*")).toEqual([{ type: "text", text: "*literal*" }]);
  });

  it("leaves unmatched markers as text", () => {
    expect(inlineText("**open")).toBe("**open");
    expect(inlineText("_open")).toBe("_open");
    expect(inlineText("~~open")).toBe("~~open");
    expect(inlineText("`open")).toBe("`open");
    expect(inlineText("[label]")).toBe("[label]");
    expect(inlineText("[label](")).toBe("[label](");
  });

  it("keeps emphasis from spanning surrounding spaces", () => {
    expect(parseInline("* a *")).toEqual([{ type: "text", text: "* a *" }]);
  });

  it("skips an escaped character inside emphasis search", () => {
    expect(parseInline("*a\\*b*")).toEqual([{ type: "emphasis", spans: [{ type: "text", text: "a*b" }] }]);
  });

  it("reads nested emphasis and a multiline paragraph", () => {
    expect(parseInline("**a _b_**")).toEqual([
      {
        type: "strong",
        spans: [
          { type: "text", text: "a " },
          { type: "emphasis", spans: [{ type: "text", text: "b" }] },
        ],
      },
    ]);
  });

  it("does not read emphasis markers inside a code span", () => {
    expect(parseInline("`a*b*c`")).toEqual([{ type: "code", text: "a*b*c" }]);
  });

  it("reads an image whose target is missing a paren", () => {
    expect(parseInline("![alt](/x")).toEqual([{ type: "text", text: "![alt](/x" }]);
  });

  it("reads an image whose label never closes", () => {
    expect(parseInline("![alt")).toEqual([{ type: "text", text: "![alt" }]);
  });

  it("keeps an escaped bracket inside a link label", () => {
    expect(parseInline("[a\\]b](https://x.test)")).toEqual([
      { type: "link", href: "https://x.test", spans: [{ type: "text", text: "a]b" }] },
    ]);
  });

  it("reads a link whose target never closes", () => {
    expect(inlineText("[x](https://x.test")).toBe("[x](https://x.test");
  });

  it("finds a closer past an escape and misses one that never comes", () => {
    expect(parseInline("**a\\**b**")).toEqual([{ type: "strong", spans: [{ type: "text", text: "a**b" }] }]);
    expect(inlineText("*a\\*b")).toBe("*a*b");
  });

  it("keeps a strike that never closes as text", () => {
    expect(inlineText("~~a")).toBe("~~a");
  });
});

describe("markdownText", () => {
  it("walks code, quotes, lists, rules and every span kind", () => {
    const text = markdownText([
      { type: "code", language: "ts", text: "const a = 1;" },
      { type: "quote", blocks: [{ type: "paragraph", spans: [{ type: "text", text: "quoted" }] }] },
      {
        type: "list",
        ordered: false,
        start: 1,
        items: [
          { blocks: [{ type: "paragraph", spans: [{ type: "text", text: "first" }] }] },
          { blocks: [{ type: "paragraph", spans: [{ type: "text", text: "second" }] }] },
        ],
      },
      { type: "rule" },
      {
        type: "paragraph",
        spans: [
          { type: "strong", spans: [{ type: "text", text: "s" }] },
          { type: "emphasis", spans: [{ type: "text", text: "e" }] },
          { type: "strike", spans: [{ type: "text", text: "t" }] },
          { type: "link", href: "#x", spans: [{ type: "text", text: "l" }] },
          { type: "break" },
          { type: "code", text: "c" },
        ],
      },
    ]);
    expect(text).toBe("const a = 1;\nquoted\nfirst\nsecond\nsetl\nc");
  });
});
