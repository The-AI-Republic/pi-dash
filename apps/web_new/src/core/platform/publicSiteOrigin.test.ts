// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One site origin for crawlable marketing metadata (CLOUD-005): the value
// the build bakes into the bundle must be the value the prerender step
// writes, so a deployment's canonical and og:url survive hydration.
import { afterEach, describe, expect, it, vi } from "vitest";

import { bakedPublicSiteOrigin, DEFAULT_PUBLIC_SITE_ORIGIN, publicSiteOriginFromEnv } from "./publicSiteOrigin.js";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("publicSiteOriginFromEnv", () => {
  it("uses the hosted product address when nothing is configured", () => {
    expect(DEFAULT_PUBLIC_SITE_ORIGIN).toBe("https://pidash.airepublic.com");
    expect(publicSiteOriginFromEnv({})).toBe(DEFAULT_PUBLIC_SITE_ORIGIN);
    expect(publicSiteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: undefined })).toBe("https://pidash.airepublic.com");
  });

  it("takes the configured deployment origin and drops trailing slashes", () => {
    expect(publicSiteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: "https://site.test///" })).toBe("https://site.test");
    expect(publicSiteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: "  https://site.test/  " })).toBe("https://site.test");
  });

  it("treats a blank value as absent", () => {
    expect(publicSiteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: "" })).toBe("https://pidash.airepublic.com");
    expect(publicSiteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: "   " })).toBe("https://pidash.airepublic.com");
  });
});

describe("bakedPublicSiteOrigin", () => {
  it("falls back to the default when the bundle carries no define", () => {
    expect(bakedPublicSiteOrigin()).toBe("https://pidash.airepublic.com");
  });

  it("reads the origin the build baked in", () => {
    vi.stubGlobal("__PIDASH_PUBLIC_SITE_ORIGIN__", "https://example.org");
    expect(bakedPublicSiteOrigin()).toBe("https://example.org");
  });
});
