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
        { type: "heading", attrs: { level: 2 }, content: [text("Middle")] },
        { type: "heading", attrs: { level: 9 }, content: [text("Deep")] },
        { type: "heading", attrs: {}, content: [text("Plain")] }
      ),
      description_html: "",
    });
    expect(html).toBe("<h2>Middle</h2><h6>Deep</h6><h1>Plain</h1>");
  });

  it("paints the marks it knows and keeps the text of one it does not", () => {
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
    expect(html).toBe("<p><strong>bold</strong> <em><s>italic strike</s></em> <code>code</code> under</p>");
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
