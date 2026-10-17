// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import { isStoredHtmlEmpty, sanitizeHtml, storedTextContent } from "./sanitize.js";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("sanitizeHtml", () => {
  it("keeps formatting while stripping scripts and handlers", () => {
    const clean = sanitizeHtml(
      '<p>Hello <strong>world</strong> <a href="https://example.com">link</a></p><script>alert(1)</script><img src=x onerror=alert(2)>'
    );
    expect(clean).toContain("<strong>world</strong>");
    expect(clean).toContain('href="https://example.com"');
    expect(clean).not.toContain("<script>");
    expect(clean).not.toContain("onerror");
  });

  it("drops javascript: links", () => {
    expect(sanitizeHtml('<a href="javascript:alert(1)">x</a>')).not.toContain("javascript:");
  });

  it("passes empty input through untouched", () => {
    expect(sanitizeHtml("")).toBe("");
  });
});

describe("storedTextContent", () => {
  it("reads the visible text out of stored markup", () => {
    expect(storedTextContent("<h2>Title</h2><p>one <em>two</em></p>")).toBe("Titleone two");
  });

  it("returns an empty string for empty markup", () => {
    expect(storedTextContent("")).toBe("");
    expect(storedTextContent("<p></p>")).toBe("");
  });

  it("returns an empty string where no DOM parser exists", () => {
    vi.stubGlobal("DOMParser", undefined);
    expect(storedTextContent("<p>hi</p>")).toBe("");
  });
});

describe("isStoredHtmlEmpty", () => {
  it("treats whitespace-only markup as empty", () => {
    expect(isStoredHtmlEmpty("")).toBe(true);
    expect(isStoredHtmlEmpty("<p></p>")).toBe(true);
    expect(isStoredHtmlEmpty("<p>   </p>")).toBe(true);
  });

  it("treats markup with text as non-empty", () => {
    expect(isStoredHtmlEmpty("<p>hi</p>")).toBe(false);
  });
});
