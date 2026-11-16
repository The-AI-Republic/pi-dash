// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The feature barrel is the only surface routes import (CLOUD-003/004,
// CLOUD-006–009): one import must reach the frame, the pages, the head
// and the site origin the crawl metadata is built from.
import { describe, expect, it } from "vitest";

import * as marketing from "./index.js";

describe("marketing barrel", () => {
  it("exposes the frame, pages, head and their content", () => {
    expect(marketing.MarketingFrame).toBeTypeOf("function");
    expect(marketing.MarketingHome).toBeTypeOf("function");
    expect(marketing.MarketingDocs).toBeTypeOf("function");
    expect(marketing.MarketingDownloads).toBeTypeOf("function");
    expect(marketing.MarketingPricing).toBeTypeOf("function");
    expect(marketing.MarketingHead).toBeTypeOf("function");
    expect(marketing.marketingTags).toBeTypeOf("function");
    expect(marketing.canonicalUrl("/docs")).toBe(`${marketing.bakedPublicSiteOrigin()}/docs`);
    expect(marketing.metaFor("/home")).toBe(marketing.MARKETING_META["/home"]);
    expect(marketing.SOCIAL_PREVIEW_PATH).toBe("/social-card.svg");
    expect(marketing.SOCIAL_PREVIEW_ALT.length).toBeGreaterThan(0);
    expect(marketing.SOCIAL_SITE_HANDLE.startsWith("@")).toBe(true);
  });

  it("hands on the one site-origin rule", () => {
    expect(marketing.DEFAULT_PUBLIC_SITE_ORIGIN).toBe(marketing.bakedPublicSiteOrigin());
    expect(marketing.publicSiteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: "https://x.test/" })).toBe("https://x.test");
  });
});
