// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Crawlable metadata for a marketing page (CLOUD-005): index/follow,
// canonical, description and social preview data.
import { render } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MARKETING_META } from "../content.js";
import {
  canonicalUrl,
  MarketingHead,
  marketingTags,
  SOCIAL_PREVIEW_PATH,
  SOCIAL_SITE_HANDLE,
} from "./MarketingHead.js";

afterEach(() => {
  document.head.innerHTML = "";
  vi.unstubAllGlobals();
});

function meta(selector: string): string | null {
  return document.querySelector(selector)?.getAttribute("content") ?? null;
}

describe("MarketingHead", () => {
  it("writes index/follow, canonical, description and social preview data", () => {
    render(<MarketingHead path="/pricing" />);
    expect(meta('meta[name="robots"]')).toBe("index, follow");
    expect(document.querySelector('link[rel="canonical"]')?.getAttribute("href")).toBe(
      "https://pidash.airepublic.com/pricing"
    );
    expect(meta('meta[name="description"]')?.length).toBeGreaterThan(10);
    expect(meta('meta[property="og:url"]')).toBe("https://pidash.airepublic.com/pricing");
    expect(meta('meta[property="og:image"]')).toBe(`https://pidash.airepublic.com${SOCIAL_PREVIEW_PATH}`);
    expect(meta('meta[property="og:image:width"]')).toBe("1200");
    expect(meta('meta[property="og:image:height"]')).toBe("630");
    expect(meta('meta[property="og:image:alt"]')?.length).toBeGreaterThan(0);
    expect(meta('meta[name="twitter:site"]')).toBe(SOCIAL_SITE_HANDLE);
  });

  it("falls back to the home metadata for an unknown path", () => {
    render(<MarketingHead path="/elsewhere" />);
    expect(meta('meta[name="description"]')).toContain("agents");
  });

  it("builds the full tag set for an address, with one canonical and its own og:url", () => {
    const meta = MARKETING_META["/docs"]!;
    const tags = marketingTags("/docs", meta);
    expect(tags.map((tag) => tag.tag)).toContain("link");
    const byKey = (key: string): string | undefined =>
      tags.find((tag) => tag.attributes["property"] === key || tag.attributes["name"] === key)?.attributes["content"];
    expect(byKey("og:url")).toBe("https://pidash.airepublic.com/docs");
    expect(byKey("og:image")).toBe("https://pidash.airepublic.com/social-card.svg");
    expect(byKey("og:title")).toBe(meta.title);
    expect(byKey("description")).toBe(meta.description);
    expect(tags.filter((tag) => tag.attributes["rel"] === "canonical")).toHaveLength(1);
  });

  it("keeps the origin the build baked in after hydration (CLOUD-005)", () => {
    vi.stubGlobal("__PIDASH_PUBLIC_SITE_ORIGIN__", "https://example.org");
    render(<MarketingHead path="/pricing" />);
    expect(document.querySelector('link[rel="canonical"]')?.getAttribute("href")).toBe("https://example.org/pricing");
    expect(meta('meta[property="og:url"]')).toBe("https://example.org/pricing");
    expect(meta('meta[property="og:image"]')).toBe("https://example.org/social-card.svg");
  });

  it("reports one twitter:card and one canonical across a remount", () => {
    const first = render(<MarketingHead path="/docs" />);
    first.unmount();
    render(<MarketingHead path="/docs" />);
    expect(document.querySelectorAll('meta[name="twitter:card"]')).toHaveLength(1);
    expect(document.querySelectorAll('link[rel="canonical"]')).toHaveLength(1);
  });

  it("strips a trailing slash from the origin when building the canonical address", () => {
    expect(canonicalUrl("/docs", "https://site.test///")).toBe("https://site.test/docs");
    expect(canonicalUrl("/docs")).toBe("https://pidash.airepublic.com/docs");
  });
});
