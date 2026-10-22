// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prerender entry (CLOUD-005): each public page renders to a complete
// document body with one top-level heading, and the root canonicalizes to
// the marketing home.
import { afterEach, describe, expect, it, vi } from "vitest";

import { canonicalFor, PRERENDER_PATHS, renderAllMarketingPages, renderMarketingPage } from "./prerender.js";

afterEach(() => {
  vi.unstubAllGlobals();
});

function headings(html: string): number {
  return (html.match(/<h1[\s>]/g) ?? []).length;
}

describe("prerender", () => {
  it("writes the root plus the four marketing pages", () => {
    expect(PRERENDER_PATHS).toEqual(["/", "/home", "/docs", "/downloads", "/pricing"]);
    expect(renderAllMarketingPages().map((page) => page.path)).toEqual([...PRERENDER_PATHS]);
  });

  it("gives every page exactly one top-level heading and the shared frame", () => {
    for (const page of renderAllMarketingPages()) {
      expect(headings(page.markup), page.path).toBe(1);
      expect(page.markup).toContain("<header");
      expect(page.markup).toContain("<footer");
      expect(page.markup).toContain("<main");
    }
  });

  it("canonicalizes the root to the marketing home", () => {
    expect(canonicalFor("/")).toBe("https://pidash.airepublic.com/home");
    expect(canonicalFor("/home", "https://site.test/")).toBe("https://site.test/home");
    expect(renderMarketingPage("/").canonical).toBe("https://pidash.airepublic.com/home");
  });

  it("canonicalizes with the origin the build baked in", () => {
    vi.stubGlobal("__PIDASH_PUBLIC_SITE_ORIGIN__", "https://example.org/");
    expect(canonicalFor("/home")).toBe("https://example.org/home");
    expect(renderMarketingPage("/docs").canonical).toBe("https://example.org/docs");
  });

  it("renders the home sections in order", () => {
    const home = renderMarketingPage("/home");
    expect((home.markup.match(/<section/g) ?? []).length).toBe(6);
    expect(home.markup).toContain('href="/login"');
    expect(home.markup).toContain("https://github.com/The-AI-Republic/pi-dash");
  });

  it("refuses an address with no prerender body", () => {
    expect(() => renderMarketingPage("/nowhere")).toThrow(/No prerender body/);
  });
});
