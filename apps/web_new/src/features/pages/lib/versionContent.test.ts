// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Snapshot body selection (parity/specs/pages.md §6.9, PAGE-052): the
// structured document wins when the renderer knows every node, otherwise
// the stored markup stands in, and an empty rendering falls back too.

import { describe, expect, it } from "vitest";

import { snapshotBodyHtml } from "./versionContent.js";

function doc(...blocks: unknown[]): Record<string, unknown> {
  return { type: "doc", content: blocks };
}

function paragraph(...content: unknown[]): Record<string, unknown> {
  return { type: "paragraph", content };
}

function text(value: string, marks?: unknown[]): Record<string, unknown> {
  return marks === undefined ? { type: "text", text: value } : { type: "text", text: value, marks };
}

describe("snapshotBodyHtml", () => {
  it("renders the structured document when it is fully known", () => {
    const html = snapshotBodyHtml({
      description_json: doc(paragraph(text("Structured words"))),
      description_html: "<p>Markup words</p>",
    });
    expect(html).toBe("<p>Structured words</p>");
  });

  it("prefers the structured form over divergent markup", () => {
    const html = snapshotBodyHtml({
      description_json: doc(paragraph(text("Snapshot shared words"))),
      description_html: "<p>Markup fallback words</p>",
    });
    expect(html).toContain("Snapshot shared words");
    expect(html).not.toContain("Markup fallback words");
  });

  it("falls back to the stored markup when a node is unknown", () => {
    const html = snapshotBodyHtml({
      description_json: doc({ type: "image", attrs: { src: "a.png" } }),
      description_html: "<p>Markup fallback words</p>",
    });
    expect(html).toBe("<p>Markup fallback words</p>");
  });

  it("falls back when the structured value is not a document", () => {
    const html = snapshotBodyHtml({ description_json: {}, description_html: "<p>Only markup</p>" });
    expect(html).toBe("<p>Only markup</p>");
  });

  it("falls back when the structured document renders nothing", () => {
    const html = snapshotBodyHtml({ description_json: doc(), description_html: "<p>Only markup</p>" });
    expect(html).toBe("<p>Only markup</p>");
  });

  it("paints heading levels, clamped to the six the reader has", () => {
    const html = snapshotBodyHtml({
      description_json: doc(
        { type: "heading", attrs: { level: 2, textAlign: null }, content: [text("Middle")] },
        { type: "heading", attrs: { level: 9 }, content: [text("Deep")] },
        { type: "heading", attrs: {}, content: [text("Plain")] }
      ),
      description_html: "",
    });
    expect(html).toBe("<h2>Middle</h2><h6>Deep</h6><h1>Plain</h1>");
  });

  it("reads the document the live editor actually stores, null defaults and all", () => {
    // Captured from the page editor (StarterKit + underline + alignment)
    // parsing a body with alignment, a list start and nested blocks.
    const html = snapshotBodyHtml({
      description_json: {
        type: "doc",
        content: [
          {
            type: "paragraph",
            attrs: { textAlign: "center" },
            content: [{ type: "text", marks: [{ type: "underline" }], text: "a" }],
          },
          { type: "heading", attrs: { textAlign: null, level: 2 }, content: [{ type: "text", text: "b" }] },
          {
            type: "orderedList",
            attrs: { start: 3 },
            content: [
              {
                type: "listItem",
                content: [{ type: "paragraph", attrs: { textAlign: null }, content: [{ type: "text", text: "c" }] }],
              },
            ],
          },
          {
            type: "paragraph",
            attrs: { textAlign: null },
            content: [{ type: "text", text: "d" }, { type: "hardBreak" }, { type: "text", text: "e" }],
          },
        ],
      },
      description_html: "<p>Markup fallback words</p>",
    });
    expect(html).toBe(
      '<p style="text-align: center"><u>a</u></p><h2>b</h2><ol start="3"><li><p>c</p></li></ol><p>d<br>e</p>'
    );
  });

  it("falls back for a node kind the page editor has but this renderer does not paint", () => {
    const html = snapshotBodyHtml({
      description_json: doc({
        type: "taskList",
        content: [
          {
            type: "taskItem",
            attrs: { checked: false },
            content: [{ type: "paragraph", attrs: { textAlign: null }, content: [text("todo")] }],
          },
        ],
      }),
      description_html: '<ul data-type="taskList"><li>todo</li></ul>',
    });
    expect(html).toBe('<ul data-type="taskList"><li>todo</li></ul>');
  });

  it("paints every mark the page editor writes", () => {
    const html = snapshotBodyHtml({
      description_json: doc(
        paragraph(
          text("bold", [{ type: "bold" }]),
          text(" "),
          text("italic strike", [{ type: "italic" }, { type: "strike" }]),
          text(" "),
          text("code", [{ type: "code" }]),
          text(" "),
          text("under", [{ type: "underline" }])
        )
      ),
      description_html: "",
    });
    expect(html).toBe("<p><strong>bold</strong> <em><s>italic strike</s></em> <code>code</code> <u>under</u></p>");
  });

  it("falls back to the stored markup for a mark the renderer would drop", () => {
    const html = snapshotBodyHtml({
      description_json: doc(paragraph(text("highlighted", [{ type: "highlight" }]))),
      description_html: "<p><mark>highlighted</mark></p>",
    });
    expect(html).toBe("<p><mark>highlighted</mark></p>");
  });

  it("keeps a text block's alignment as the editor stored it", () => {
    const html = snapshotBodyHtml({
      description_json: doc(
        { type: "paragraph", attrs: { textAlign: "center" }, content: [text("aligned")] },
        { type: "heading", attrs: { level: 2, textAlign: "right" }, content: [text("title")] },
        { type: "paragraph", attrs: { textAlign: "left" }, content: [text("normal")] }
      ),
      description_html: "",
    });
    expect(html).toBe(
      '<p style="text-align: center">aligned</p><h2 style="text-align: right">title</h2><p style="text-align: left">normal</p>'
    );
  });

  it("falls back rather than dropping an alignment it cannot paint", () => {
    const html = snapshotBodyHtml({
      description_json: doc({ type: "paragraph", attrs: { textAlign: "justify" }, content: [text("justified")] }),
      description_html: "<p>Markup fallback words</p>",
    });
    expect(html).toBe("<p>Markup fallback words</p>");
  });

  it("falls back rather than dropping an attribute or a heading level it cannot read", () => {
    const unknownBlockAttribute = snapshotBodyHtml({
      description_json: doc({ type: "paragraph", attrs: { colour: "red" }, content: [text("coloured")] }),
      description_html: "<p>Markup fallback words</p>",
    });
    const unreadableHeadingLevel = snapshotBodyHtml({
      description_json: doc({ type: "heading", attrs: { level: "2" }, content: [text("title")] }),
      description_html: "<p>Markup fallback words</p>",
    });
    expect(unknownBlockAttribute).toBe("<p>Markup fallback words</p>");
    expect(unreadableHeadingLevel).toBe("<p>Markup fallback words</p>");
  });

  it("falls back for a link whose address is not text, and for malformed mark data", () => {
    const numericHref = snapshotBodyHtml({
      description_json: doc(paragraph(text("site", [{ type: "link", attrs: { href: 42 } }]))),
      description_html: "<p>Markup fallback words</p>",
    });
    const notAList = snapshotBodyHtml({
      description_json: doc({ type: "paragraph", content: [{ type: "text", text: "odd", marks: { type: "bold" } }] }),
      description_html: "<p>Markup fallback words</p>",
    });
    expect(numericHref).toBe("<p>Markup fallback words</p>");
    expect(notAList).toBe("<p>Markup fallback words</p>");
  });

  it("links only when the mark carries an address", () => {
    const html = snapshotBodyHtml({
      description_json: doc(
        paragraph(
          text("site", [{ type: "link", attrs: { href: "https://example.test/a?b=1&c=2" } }]),
          text(" "),
          text("bare", [{ type: "link", attrs: {} }])
        )
      ),
      description_html: "",
    });
    expect(html).toBe('<p><a href="https://example.test/a?b=1&amp;c=2">site</a> bare</p>');
  });

  it("keeps the stored link attributes the page editor writes", () => {
    // Captured from the page editor (StarterKit + Link): every link mark
    // carries target, rel and class beside its address.
    const html = snapshotBodyHtml({
      description_json: doc(
        paragraph(
          text("one", [
            {
              type: "link",
              attrs: {
                href: "https://example.test/a?b=1&c=2",
                target: "_blank",
                rel: "noopener noreferrer nofollow",
                class: null,
              },
            },
          ]),
          text(" "),
          text("two", [
            { type: "link", attrs: { href: "https://example.test/y", target: "_self", rel: "nofollow", class: "k" } },
          ])
        )
      ),
      description_html: "<p>Markup fallback words</p>",
    });
    expect(html).toBe(
      '<p><a href="https://example.test/a?b=1&amp;c=2" target="_blank" rel="noopener noreferrer nofollow">one</a> ' +
        '<a href="https://example.test/y" target="_self" rel="nofollow" class="k">two</a></p>'
    );
  });

  it("falls back rather than dropping a link attribute it cannot paint", () => {
    const unknownAttribute = snapshotBodyHtml({
      description_json: doc(
        paragraph(
          text("site", [{ type: "link", attrs: { href: "https://example.test", target: "_blank", download: "a.pdf" } }])
        )
      ),
      description_html: "<p>Markup fallback words</p>",
    });
    const numericRel = snapshotBodyHtml({
      description_json: doc(
        paragraph(text("site", [{ type: "link", attrs: { href: "https://example.test", rel: 42 } }]))
      ),
      description_html: "<p>Markup fallback words</p>",
    });
    expect(unknownAttribute).toBe("<p>Markup fallback words</p>");
    expect(numericRel).toBe("<p>Markup fallback words</p>");
  });

  it("escapes an address and its link attributes so stored quotes cannot break out", () => {
    const html = snapshotBodyHtml({
      description_json: doc(
        paragraph(
          text("site", [
            {
              type: "link",
              attrs: { href: 'https://example.test/"onmouseover="x', target: '""', rel: "a&b" },
            },
          ])
        )
      ),
      description_html: "",
    });
    expect(html).toBe(
      '<p><a href="https://example.test/&quot;onmouseover=&quot;x" target="&quot;&quot;" rel="a&amp;b">site</a></p>'
    );
  });

  it("escapes text so stored angle brackets never become tags", () => {
    const html = snapshotBodyHtml({
      description_json: doc(paragraph(text("a < b & c > d"))),
      description_html: "",
    });
    expect(html).toBe("<p>a &lt; b &amp; c &gt; d</p>");
  });

  it("paints lists with their nested lists inside the item", () => {
    const html = snapshotBodyHtml({
      description_json: doc({
        type: "bulletList",
        content: [
          {
            type: "listItem",
            content: [
              paragraph(text("one")),
              { type: "orderedList", content: [{ type: "listItem", content: [paragraph(text("nested"))] }] },
            ],
          },
          { type: "listItem", content: [paragraph(text("two"))] },
        ],
      }),
      description_html: "",
    });
    expect(html).toBe("<ul><li><p>one</p><ol><li><p>nested</p></li></ol></li><li><p>two</p></li></ul>");
  });

  it("keeps an ordered list's starting number only when it is not one", () => {
    const html = snapshotBodyHtml({
      description_json: doc(
        {
          type: "orderedList",
          attrs: { start: 3 },
          content: [{ type: "listItem", content: [paragraph(text("three"))] }],
        },
        { type: "orderedList", attrs: { start: 1 }, content: [{ type: "listItem", content: [paragraph(text("one"))] }] }
      ),
      description_html: "",
    });
    expect(html).toBe('<ol start="3"><li><p>three</p></li></ol><ol><li><p>one</p></li></ol>');
  });

  it("paints nothing for a stored node where only phrasing content belongs", () => {
    const html = snapshotBodyHtml({
      description_json: doc(paragraph(text("kept"), { type: "horizontalRule" }, text(" "), { type: "text" })),
      description_html: "",
    });
    expect(html).toBe("<p>kept </p>");
  });

  it("keeps the text of a mark with no attributes at all", () => {
    const html = snapshotBodyHtml({
      description_json: doc(paragraph(text("plain", [{ type: "link" }, { type: "bold" }]))),
      description_html: "",
    });
    expect(html).toBe("<p><strong>plain</strong></p>");
  });

  it("paints quotes, code blocks, dividers and hard breaks", () => {
    const html = snapshotBodyHtml({
      description_json: doc(
        { type: "blockquote", content: [paragraph(text("quoted"))] },
        { type: "codeBlock", content: [text("a < b")] },
        { type: "horizontalRule" },
        paragraph(text("before"), { type: "hardBreak" }, text("after"))
      ),
      description_html: "",
    });
    expect(html).toBe(
      "<blockquote><p>quoted</p></blockquote><pre><code>a &lt; b</code></pre><hr><p>before<br>after</p>"
    );
  });
});
