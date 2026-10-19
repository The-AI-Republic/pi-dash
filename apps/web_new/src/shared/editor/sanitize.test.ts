// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import { isStoredHtmlEmpty, isStoredHtmlEmptyIgnoring, sanitizeHtml, storedTextContent } from "./sanitize.js";

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

  it("reads text from documents whose body holds no text", () => {
    const parse = vi.fn(() => ({ body: { textContent: null } }));
    vi.stubGlobal("DOMParser", function StubParser() {
      return { parseFromString: parse };
    });
    expect(storedTextContent("<p>hi</p>")).toBe("");
    expect(parse).toHaveBeenCalledWith("<p>hi</p>", "text/html");
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

describe("isStoredHtmlEmptyIgnoring", () => {
  it("keeps the word rule", () => {
    expect(isStoredHtmlEmptyIgnoring("<p></p>", ["img"])).toBe(true);
    expect(isStoredHtmlEmptyIgnoring("<p>hi</p>", ["img"])).toBe(false);
  });

  it("treats an allowed tag as content, in any letter case", () => {
    expect(isStoredHtmlEmptyIgnoring('<p><img src="/a.png"></p>', ["img"])).toBe(false);
    expect(isStoredHtmlEmptyIgnoring('<p><IMG src="/a.png"></p>', ["IMG"])).toBe(false);
    expect(isStoredHtmlEmptyIgnoring("<p><span> </span></p>", ["img"])).toBe(true);
    expect(isStoredHtmlEmptyIgnoring('<p><img src="/a.png"></p>', [])).toBe(true);
  });

  it("reports empty where no DOM parser exists", () => {
    vi.stubGlobal("DOMParser", undefined);
    expect(isStoredHtmlEmptyIgnoring('<p><img src="/a.png"></p>', ["img"])).toBe(true);
  });
});
