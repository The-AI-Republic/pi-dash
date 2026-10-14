// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { normalizeQuickLinkAddress, quickLinkDisplayName, validateQuickLinkForm } from "./quicklinks.js";

describe("normalizeQuickLinkAddress", () => {
  it("trims the address", () => {
    expect(normalizeQuickLinkAddress("  https://example.com/docs  ")).toBe("https://example.com/docs");
  });

  it("leaves an empty address for validation to reject", () => {
    expect(normalizeQuickLinkAddress("   ")).toBe("");
  });

  it("keeps an explicit scheme untouched", () => {
    expect(normalizeQuickLinkAddress("http://example.com")).toBe("http://example.com");
  });

  it("completes a missing scheme", () => {
    expect(normalizeQuickLinkAddress("example.com/docs")).toBe("https://example.com/docs");
  });
});

describe("validateQuickLinkForm", () => {
  it("rejects a blank address", () => {
    expect(validateQuickLinkForm({ address: "   ", name: "Docs" })).toEqual({
      ok: false,
      error: "Enter the link address.",
    });
  });

  it("rejects an unparseable address", () => {
    expect(validateQuickLinkForm({ address: "not a url", name: "" })).toEqual({
      ok: false,
      error: "Enter a valid link address.",
    });
  });

  it("rejects a non-web scheme", () => {
    expect(validateQuickLinkForm({ address: "javascript:alert(1)", name: "" })).toEqual({
      ok: false,
      error: "Enter a valid link address.",
    });
  });

  it("accepts a full address and trims the name", () => {
    expect(validateQuickLinkForm({ address: "https://example.com/docs", name: "  Docs  " })).toEqual({
      ok: true,
      value: { url: "https://example.com/docs", title: "Docs" },
    });
  });

  it("accepts a plain http address untouched", () => {
    expect(validateQuickLinkForm({ address: "http://example.com/docs", name: "" })).toEqual({
      ok: true,
      value: { url: "http://example.com/docs", title: "" },
    });
  });

  it("accepts a bare host with a completed scheme", () => {
    expect(validateQuickLinkForm({ address: "example.com", name: "" })).toEqual({
      ok: true,
      value: { url: "https://example.com", title: "" },
    });
  });
});

describe("quickLinkDisplayName", () => {
  it("prefers the display name", () => {
    expect(quickLinkDisplayName({ title: "Docs", url: "https://example.com" })).toBe("Docs");
  });

  it("falls back to the address when the name is blank", () => {
    expect(quickLinkDisplayName({ title: "  ", url: "https://example.com" })).toBe("https://example.com");
  });

  it("falls back to the address when no name was stored", () => {
    expect(quickLinkDisplayName({ title: null, url: "https://example.com" })).toBe("https://example.com");
  });
});
