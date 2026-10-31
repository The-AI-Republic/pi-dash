// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page body read seam (bug NEWFRONT-360): the live read wins only when
// it carries content, and a blank fallback stays authoritative so an
// emptied page never resurrects stored text.

import { describe, expect, it } from "vitest";

import { pageBodyHasContent, preferLiveBodyHtml } from "./pageBodyRead.js";

describe("pageBodyHasContent", () => {
  it("rejects blank serializations", () => {
    expect(pageBodyHasContent("")).toBe(false);
    expect(pageBodyHasContent("   ")).toBe(false);
    expect(pageBodyHasContent("<p></p>")).toBe(false);
    expect(pageBodyHasContent("<p>  </p>")).toBe(false);
  });

  it("accepts words", () => {
    expect(pageBodyHasContent("<p>Stored words</p>")).toBe(true);
  });

  it("accepts a picture-only body", () => {
    expect(pageBodyHasContent('<p><img src="asset-1" alt="shot"></p>')).toBe(true);
    expect(pageBodyHasContent('<p><image-component src="asset-1"></image-component></p>')).toBe(true);
  });

  it("accepts a mention-only body", () => {
    expect(pageBodyHasContent('<p><mention-component data-name="Ada"></mention-component></p>')).toBe(true);
  });

  it("accepts a rule-only body", () => {
    expect(pageBodyHasContent("<hr>")).toBe(true);
  });
});

describe("preferLiveBodyHtml", () => {
  it("prefers a live read that carries content", () => {
    expect(preferLiveBodyHtml("<p>live</p>", "<p>fallback</p>")).toBe("<p>live</p>");
    expect(preferLiveBodyHtml("<p>live</p>", "")).toBe("<p>live</p>");
  });

  it("prefers a picture-only live read over fallback words", () => {
    expect(preferLiveBodyHtml('<p><img src="asset-1" alt="shot"></p>', "<p>fallback</p>")).toBe(
      '<p><img src="asset-1" alt="shot"></p>'
    );
  });

  it("falls back when the live read is blank", () => {
    expect(preferLiveBodyHtml("", "<p>fallback</p>")).toBe("<p>fallback</p>");
    expect(preferLiveBodyHtml("<p></p>", "<p>fallback</p>")).toBe("<p>fallback</p>");
  });

  it("falls back when no handle is mounted", () => {
    expect(preferLiveBodyHtml(null, "<p>fallback</p>")).toBe("<p>fallback</p>");
    expect(preferLiveBodyHtml(undefined, "<p>fallback</p>")).toBe("<p>fallback</p>");
  });

  it("keeps a blank fallback authoritative instead of reaching past it", () => {
    expect(preferLiveBodyHtml("", "")).toBe("");
    expect(preferLiveBodyHtml("<p></p>", "<p></p>")).toBe("<p></p>");
    expect(preferLiveBodyHtml(null, "")).toBe("");
  });
});
