// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Plain-text export scope (parity/specs/exports.md §5, EXP-020): the
// image-free scope drops image tags while links and text survive.
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
});

describe("applyMarkdownScope", () => {
  it("keeps the markup untouched under full fidelity", () => {
    const body = "marker\n![undefined](https://example.com/shot.png)";
    expect(applyMarkdownScope(body, "Everything")).toBe(body);
  });

  it("strips image tags under the image-free scope", () => {
    expect(applyMarkdownScope("marker\n![undefined](https://example.com/shot.png)", "No images")).toBe("marker\n");
  });
});
