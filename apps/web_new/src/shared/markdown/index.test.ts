// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant markdown rendering (AGT-047): headings, emphasis, code and
// lists render; raw HTML and unsafe attributes do not; links stay external.
import { describe, expect, it } from "vitest";

import { markdownHtml } from "./index.js";

describe("markdownHtml", () => {
  it("renders headings, emphasis, inline code and lists", () => {
    const html = markdownHtml("# Title\n\n**bold** and *italic* and `code`\n\n- one\n- two");
    expect(html).toContain("<h1>Title</h1>");
    expect(html).toContain("<strong>bold</strong>");
    expect(html).toContain("<em>italic</em>");
    expect(html).toContain("<code>code</code>");
    expect(html).toContain("<li>one</li>");
  });

  it("keeps single newlines as line breaks", () => {
    expect(markdownHtml("first\nsecond")).toContain("<br>");
  });

  it("opens links away from the app and drops script and event handlers", () => {
    const html = markdownHtml("[docs](https://example.test)\n\n<img src=x onerror=alert(1)>");
    expect(html).toContain('href="https://example.test"');
    expect(html).toContain('target="_blank"');
    expect(html).toContain('rel="noopener noreferrer"');
    expect(html).not.toContain("<img");
    expect(html).not.toContain("<img src=x");
  });

  it("never interprets raw HTML blocks", () => {
    const html = markdownHtml("<script>alert(1)</script>");
    expect(html).not.toContain("<script");
  });
});
