// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Whole-body Markdown copy (parity/specs/pages.md §6.1, PAGE-044): the
// clipboard carries the Markdown form, and pasting it back yields blocks.

import { describe, expect, it } from "vitest";

import { pageBodyMarkdown } from "./pageMarkup.js";

describe("pageBodyMarkdown", () => {
  it("renders the pinned heading-and-paragraph form", () => {
    expect(pageBodyMarkdown("<h1>Copy Title</h1><p>copy body text</p>")).toBe("# Copy Title\n\ncopy body text\n");
  });

  it("returns an empty string for an empty body or without a DOM parser", () => {
    expect(pageBodyMarkdown("")).toBe("");
    expect(pageBodyMarkdown("   ")).toBe("");
  });

  it("converts marks, links, lists and code blocks", () => {
    const html = [
      "<p><strong>bold</strong> <em>italic</em> <s>gone</s> <u>under</u> <code>inline</code></p>",
      "<ol><li>one</li><li>two</li></ol>",
      '<ul><li data-checked="true">done</li><li data-checked="false">todo</li></ul>',
      '<p><a href="https://example.com">site</a></p>',
      '<pre><code class="language-js">const x = 1;</code></pre>',
    ].join("");
    const markup = pageBodyMarkdown(html);
    expect(markup).toContain("**bold**");
    expect(markup).toContain("_italic_");
    expect(markup).toContain("~~gone~~");
    expect(markup).toContain("<u>under</u>");
    expect(markup).toContain("`inline`");
    expect(markup).toContain("1. one");
    expect(markup).toContain("2. two");
    expect(markup).toContain("- [x] done");
    expect(markup).toContain("- [ ] todo");
    expect(markup).toContain("[site](https://example.com)");
    expect(markup).toContain("```js\nconst x = 1;\n```");
  });

  it("quotes, rules, tables and callouts keep their structure", () => {
    const html = [
      "<blockquote><p>quoted</p></blockquote>",
      "<hr>",
      "<table><tbody><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></tbody></table>",
      "<callout-component><p>notice</p></callout-component>",
    ].join("");
    const markup = pageBodyMarkdown(html);
    expect(markup).toContain("> quoted");
    expect(markup).toContain("---");
    expect(markup).toContain("| a | b |");
    expect(markup).toContain("| --- | --- |");
    expect(markup).toContain("| 1 | 2 |");
    expect(markup).toContain("> notice");
  });

  it("carries mentions, pictures and embeds in readable form", () => {
    const html = [
      '<p>hi <mention-component id="u-1" entity_identifier="u-1" entity_name="user_mention" data-name="Ada"></mention-component></p>',
      '<image-component src="asset-1" alt="shot"></image-component>',
      '<issue-embed-component entity_name="Embedded issue"></issue-embed-component>',
    ].join("");
    const markup = pageBodyMarkdown(html);
    expect(markup).toContain("@Ada");
    expect(markup).toContain("![shot](asset-1)");
    expect(markup).toContain("[Embedded issue]");
  });

  it("falls back to the body text when there are no block elements", () => {
    expect(pageBodyMarkdown("plain words")).toBe("plain words\n");
  });
});

describe("pageBodyMarkdown edges", () => {
  it("keeps hard breaks and styled inline spans", () => {
    expect(pageBodyMarkdown('<p>one<br>two <span data-text-color="pink">pink</span></p>')).toBe(
      'one  \ntwo <span data-text-color="pink">pink</span>\n'
    );
  });

  it("drops anchors and mentions with no target", () => {
    expect(pageBodyMarkdown("<p><a>plain</a><mention-component></mention-component></p>")).toBe("plain\n");
  });

  it("keeps an unnamed embed out of the markup", () => {
    expect(pageBodyMarkdown("<issue-embed-component></issue-embed-component><p>kept</p>")).toBe("kept\n");
  });

  it("indents nested list blocks under their item", () => {
    expect(pageBodyMarkdown("<ul><li>outer<ul><li>inner</li></ul></li></ul>")).toBe("- outer\n  - inner\n");
  });

  it("keeps an empty list out and falls back to bare text", () => {
    expect(pageBodyMarkdown("<ul><li></li></ul>")).toBe("");
    expect(pageBodyMarkdown("<p>   </p>")).toBe("");
  });
});

describe("unknown inline elements", () => {
  it("keeps the children of an unfamiliar inline element", () => {
    expect(pageBodyMarkdown("<p>kept <mark>words</mark></p>")).toBe("kept words\n");
  });
});

describe("markup fallback branches", () => {
  it("keeps a bare pre with no code child", () => {
    expect(pageBodyMarkdown("<pre><span>raw</span></pre>")).toBe("```\nraw\n```\n");
  });

  it("keeps an empty table out and an unnamed anchor's text", () => {
    expect(pageBodyMarkdown("<table></table>")).toBe("");
    expect(pageBodyMarkdown('<p><a href="">plain</a></p>')).toBe("plain\n");
  });

  it("turns an empty body into an empty string", () => {
    expect(pageBodyMarkdown("<div></div>")).toBe("");
  });

  it("ignores non-element nodes in the inline walk", () => {
    expect(pageBodyMarkdown("<p>a<!-- note -->b</p>")).toBe("ab\n");
  });

  it("treats a text-only body with an element wrapper as block text", () => {
    expect(pageBodyMarkdown("<section>only text</section>")).toBe("only text\n");
  });
});

describe("empty inline marks", () => {
  it("leaves an empty mark unwrapped", () => {
    expect(pageBodyMarkdown("<p><strong>   </strong>x</p>")).toBe("   x\n");
  });

  it("reads an emoji span and its colour sibling by their attributes", () => {
    expect(pageBodyMarkdown('<p><span data-type="emoji">🎉</span><span data-text-color="pink">p</span></p>')).toBe(
      '🎉<span data-text-color="pink">p</span>\n'
    );
  });

  it("keeps a text node's own text", () => {
    expect(pageBodyMarkdown("<p>plain text</p>")).toBe("plain text\n");
  });

  it("drops an image with no source", () => {
    expect(pageBodyMarkdown("<p><img></p>")).toBe("");
  });
});
