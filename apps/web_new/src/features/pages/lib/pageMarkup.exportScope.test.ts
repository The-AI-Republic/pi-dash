// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Markdown export chain (parity/specs/exports.md §5, PAGE-074 / EXP-018; the
// fix for NEWFRONT-356): the page body converter turns pictures into
// Markdown image tags, and the export's image-free scope has to drop those
// tags from what the converter produced. A tag-only strip matched neither
// form, so the picture survived the text-only scope.
import { describe, expect, it } from "vitest";

import { applyMarkdownScope } from "../../pages-export/index.js";
import { pageBodyMarkdown } from "./pageMarkup.js";

/** The editor's stored picture forms: an uploaded asset and a pasted URL. */
const EDITOR_BODY = [
  "<h1>Export title</h1>",
  "<p>words before</p>",
  '<image-component src="asset-074" data-stored-src="asset-074" alt="uploaded shot" status="uploaded"></image-component>',
  '<p>words after</p><p><img src="https://example.com/ic074-shot.png" alt="pasted shot"></p>',
].join("");

describe("page Markdown under the export content scope", () => {
  it("keeps every picture reference under full fidelity", () => {
    const markdown = pageBodyMarkdown(EDITOR_BODY);
    expect(markdown).toContain("![uploaded shot](asset-074)");
    expect(markdown).toContain("![pasted shot](https://example.com/ic074-shot.png)");
    expect(applyMarkdownScope(markdown, "Everything")).toBe(markdown);
  });

  it("drops every picture reference and keeps the prose under the image-free scope", () => {
    const scoped = applyMarkdownScope(pageBodyMarkdown(EDITOR_BODY), "No images");
    expect(scoped).not.toContain("![");
    expect(scoped).not.toContain("asset-074");
    expect(scoped).not.toContain("ic074-shot.png");
    expect(scoped).toContain("# Export title");
    expect(scoped).toContain("words before");
    expect(scoped).toContain("words after");
  });

  it("drops a picture whose alt text carries brackets (NEWFRONT-356)", () => {
    const html = '<p>prose</p><img src="https://example.com/ic074-1.png" alt="shot [1]">';
    const markdown = pageBodyMarkdown(html);
    expect(markdown).toContain("![shot [1]](https://example.com/ic074-1.png)");
    const scoped = applyMarkdownScope(markdown, "No images");
    expect(scoped).not.toContain("ic074-1.png");
    expect(scoped.trim()).toBe("prose");
  });
});
