// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Plain-text export scope (parity/specs/exports.md §5, EXP-020; the fix for
// NEWFRONT-356): the image-free scope drops every Markdown image tag the
// editor can write while keeping text, links and literal code untouched.
import { describe, expect, it } from "vitest";

import { applyMarkdownScope, stripMarkdownImages } from "./markdownExport.js";

describe("stripMarkdownImages", () => {
  it("drops inline image tags and keeps the marker text", () => {
    expect(stripMarkdownImages("marker one\n![undefined](https://example.com/shot.png)\nmarker two")).toBe(
      "marker one\n\nmarker two"
    );
  });

  it("drops reference image tags but leaves link definitions", () => {
    expect(stripMarkdownImages("see ![alt][pic] and ![other][]\n\n[pic]: https://example.com/a.png")).toBe(
      "see  and \n\n[pic]: https://example.com/a.png"
    );
  });

  it("drops shortcut image tags while keeping links", () => {
    expect(stripMarkdownImages("a ![logo] image and a [link](https://example.com) link")).toBe(
      "a  image and a [link](https://example.com) link"
    );
  });

  it("drops the whole tag when the alt text carries brackets (NEWFRONT-356)", () => {
    expect(stripMarkdownImages("before ![shot [1]](https://example.com/a.png) after")).toBe("before  after");
  });

  it("drops the whole tag when the address carries parentheses (NEWFRONT-356)", () => {
    expect(stripMarkdownImages("x ![pic](https://cdn.example/a(1).png) y")).toBe("x  y");
  });

  it("drops a tag whose address is angle-bracketed or titled", () => {
    expect(stripMarkdownImages("a ![p](<https://cdn.example/a b.png>) b")).toBe("a  b");
    expect(stripMarkdownImages('a ![p](https://cdn.example/a.png "shot") b')).toBe("a  b");
  });

  it("keeps escaped brackets as literal text", () => {
    expect(stripMarkdownImages("literal ![not escaped](still.png) and !\\[escaped](x.png)")).toBe(
      "literal  and !\\[escaped](x.png)"
    );
  });

  it("keeps image-shaped text inside code spans and fenced blocks", () => {
    const source = [
      "see `![a](b.png)` here",
      "```",
      "![a](b.png)",
      "```",
      "~~~md",
      "![c](d.png)",
      "~~~",
      "end ![e](f.png)",
    ].join("\n");
    expect(stripMarkdownImages(source)).toBe(
      ["see `![a](b.png)` here", "```", "![a](b.png)", "```", "~~~md", "![c](d.png)", "~~~", "end "].join("\n")
    );
  });

  it("leaves an unclosed tag in place rather than cutting the text", () => {
    expect(stripMarkdownImages("a ![oops](b.png")).toBe("a ![oops](b.png");
    expect(stripMarkdownImages("a ![oops")).toBe("a ![oops");
  });

  it("keeps a linked picture's surrounding link address", () => {
    expect(stripMarkdownImages("[![alt](shot.png)](https://example.com/page)")).toBe("[](https://example.com/page)");
  });
});

describe("applyMarkdownScope", () => {
  it("keeps the markup untouched under full fidelity", () => {
    const body = "marker\n![undefined](https://example.com/shot.png)";
    expect(applyMarkdownScope(body, "Everything")).toBe(body);
  });

  it("strips image tags under the image-free scope", () => {
    expect(applyMarkdownScope("marker\n![undefined](https://example.com/shot.png)", "No images")).toBe("marker\n");
  });

  it("keeps the document text when every picture is dropped (NEWFRONT-356)", () => {
    const body = "# Title\n\npara one\n\n![a [b]](https://example.com/ic074-1.png)\n\npara two\n";
    expect(applyMarkdownScope(body, "No images")).toBe("# Title\n\npara one\n\n\n\npara two\n");
  });
});
