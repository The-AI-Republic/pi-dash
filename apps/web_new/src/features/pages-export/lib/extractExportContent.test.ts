// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export content extraction (parity/specs/exports.md §5, EXP-018):
// paragraphs keep text and order, pictures are listed, excluded nodes drop.
import { describe, expect, it } from "vitest";

import { extractExportContent } from "./extractExportContent.js";

describe("extractExportContent", () => {
  it("reads paragraphs and picture URLs in document order", () => {
    expect(
      extractExportContent(
        '<p>first marker</p><p><img src="https://example.com/a.png">second marker</p><h2>heading marker</h2>'
      )
    ).toEqual({
      paragraphs: ["first marker", "second marker", "heading marker"],
      imageUrls: ["https://example.com/a.png"],
    });
  });

  it("reads list items and nested blocks once, skipping empty ones", () => {
    expect(
      extractExportContent("<ul><li>item one</li><li>  </li></ul><p></p><blockquote><p>quoted marker</p></blockquote>")
    ).toEqual({
      paragraphs: ["item one", "quoted marker"],
      imageUrls: [],
    });
  });

  it("resolves mentions to display names and drops excluded nodes", () => {
    expect(
      extractExportContent(
        '<p>hello <span data-type="mention" data-name="Ada">@Ada</span></p><div data-export="exclude"><p>card text</p><img src="https://example.com/card.png"></div><script>var x = 1;</script>'
      )
    ).toEqual({ paragraphs: ["hello @Ada"], imageUrls: [] });
  });

  it("skips pictures without a usable source", () => {
    expect(extractExportContent('<p>text</p><img><img src=""><img src="/api/assets/v2/x">')).toEqual({
      paragraphs: ["text"],
      imageUrls: ["/api/assets/v2/x"],
    });
  });

  it("falls back to the body text when no block matches", () => {
    expect(extractExportContent("bare marker <b>bold</b>")).toEqual({
      paragraphs: ["bare marker bold"],
      imageUrls: [],
    });
  });

  it("reads nothing from empty markup", () => {
    expect(extractExportContent("")).toEqual({ paragraphs: [], imageUrls: [] });
    expect(extractExportContent("<div><span>   </span></div>")).toEqual({ paragraphs: [], imageUrls: [] });
  });
});
