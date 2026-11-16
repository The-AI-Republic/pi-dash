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

describe("extractExportContent stored upload pictures", () => {
  const CONTEXT = { workspaceSlug: "acme", projectId: "project-1" };
  const SERVED = "/api/assets/v2/workspaces/acme/projects/project-1/asset-1/";

  it("resolves the getHTML form's bare asset id to its served stack URL", () => {
    expect(
      extractExportContent('<p>marker</p><image-component src="asset-1" alt="shot"></image-component>', CONTEXT)
    ).toEqual({ paragraphs: ["marker"], imageUrls: [SERVED] });
  });

  it("prefers the kept id over the painted address", () => {
    expect(
      extractExportContent(
        '<image-component data-stored-src="asset-1" src="https://cdn.example/painted.png"></image-component>',
        CONTEXT
      )
    ).toEqual({ paragraphs: [], imageUrls: [SERVED] });
  });

  it("falls back to src when no id is kept, and skips a sourceless element", () => {
    expect(
      extractExportContent(
        '<image-component data-stored-src="" src="https://cdn.example/kept.png"></image-component>',
        CONTEXT
      )
    ).toEqual({ paragraphs: [], imageUrls: ["https://cdn.example/kept.png"] });
    expect(extractExportContent("<image-component></image-component><p>marker</p>", CONTEXT)).toEqual({
      paragraphs: ["marker"],
      imageUrls: [],
    });
  });

  it("reads the live-DOM form's painted inner image once", () => {
    expect(
      extractExportContent(
        `<image-component data-stored-src="asset-1"><img src="${SERVED}" alt="shot"></image-component>`,
        CONTEXT
      )
    ).toEqual({ paragraphs: [], imageUrls: [SERVED] });
  });

  it("keeps an inner image whose stored element carries no source", () => {
    expect(
      extractExportContent('<image-component><img src="https://cdn.example/loose.png"></image-component>', CONTEXT)
    ).toEqual({ paragraphs: [], imageUrls: ["https://cdn.example/loose.png"] });
  });

  it("resolves a legacy bare-id picture and keeps address forms untouched", () => {
    expect(
      extractExportContent(
        '<img src="asset-1"><img src="https://cdn.example/a.png"><img src="/api/assets/v2/x">' +
          '<img src="data:image/png;base64,AA=="><img src="//cdn.example/b.png"><img src="relative/path.png">',
        CONTEXT
      )
    ).toEqual({
      paragraphs: [],
      imageUrls: [
        SERVED,
        "https://cdn.example/a.png",
        "/api/assets/v2/x",
        "data:image/png;base64,AA==",
        "//cdn.example/b.png",
        "relative/path.png",
      ],
    });
  });

  it("leaves stored ids unresolved without both context legs", () => {
    const html = '<image-component src="asset-1"></image-component>';
    const unresolved = { paragraphs: [], imageUrls: ["asset-1"] };
    expect(extractExportContent(html)).toEqual(unresolved);
    expect(extractExportContent(html, {})).toEqual(unresolved);
    expect(extractExportContent(html, { workspaceSlug: "acme" })).toEqual(unresolved);
    expect(extractExportContent(html, { projectId: "project-1" })).toEqual(unresolved);
    expect(extractExportContent(html, { workspaceSlug: "", projectId: "" })).toEqual(unresolved);
  });

  it("keeps document order across stored and pasted pictures", () => {
    expect(
      extractExportContent(
        '<img src="https://cdn.example/first.png"><image-component src="asset-1"></image-component>' +
          '<img src="https://cdn.example/last.png">',
        CONTEXT
      )
    ).toEqual({
      paragraphs: [],
      imageUrls: ["https://cdn.example/first.png", SERVED, "https://cdn.example/last.png"],
    });
  });
});
