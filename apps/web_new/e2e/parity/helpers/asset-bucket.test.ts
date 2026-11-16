// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The object-storage bridge (NEWFRONT-550): the bucket path is continued
// against the parity proxy with the page's own host kept in the header.
import type { Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import {
  bridgeParityAssetBucket,
  PARITY_ASSET_BUCKET,
  PARITY_ASSET_BUCKET_PATTERN,
  parityAssetProxyOrigin,
} from "./asset-bucket.js";

type Handler = (route: Route) => Promise<void>;

function capturingPage(): { page: Page; route: () => Handler; pattern: () => unknown } {
  const captured: { handler?: Handler; pattern?: unknown } = {};
  const page = {
    route: vi.fn(async (pattern: unknown, handler: Handler) => {
      captured.pattern = pattern;
      captured.handler = handler;
    }),
  } as unknown as Page;
  return {
    page,
    pattern: () => captured.pattern,
    route: () => {
      if (!captured.handler) throw new Error("no route handler installed");
      return captured.handler;
    },
  };
}

function fakeRoute(url: string): { route: Route; continued: { url?: string; headers?: Record<string, string> } } {
  const continued: { url?: string; headers?: Record<string, string> } = {};
  const route = {
    request: () => ({
      url: () => url,
      headers: () => ({ authorization: "sig", cookie: "session=1" }),
    }),
    continue: vi.fn(async (options: { url?: string; headers?: Record<string, string> }) => {
      Object.assign(continued, options);
    }),
  } as unknown as Route;
  return { route, continued };
}

describe("parity asset bucket pattern", () => {
  it("matches the bucket root, children and query strings", () => {
    expect(PARITY_ASSET_BUCKET_PATTERN.test(`/${PARITY_ASSET_BUCKET}`)).toBe(true);
    expect(PARITY_ASSET_BUCKET_PATTERN.test(`/${PARITY_ASSET_BUCKET}/`)).toBe(true);
    expect(PARITY_ASSET_BUCKET_PATTERN.test(`/${PARITY_ASSET_BUCKET}?X-Amz-Signature=abc`)).toBe(true);
    expect(PARITY_ASSET_BUCKET_PATTERN.test(`/${PARITY_ASSET_BUCKET}/7f/logo.png`)).toBe(true);
    expect(PARITY_ASSET_BUCKET_PATTERN.test(`/${PARITY_ASSET_BUCKET}/7f/logo.png?X=1`)).toBe(true);
  });

  it("does not match a sibling path that merely shares the prefix", () => {
    expect(PARITY_ASSET_BUCKET_PATTERN.test(`/${PARITY_ASSET_BUCKET}-other/logo.png`)).toBe(false);
    expect(PARITY_ASSET_BUCKET_PATTERN.test("/api/assets/logo.png")).toBe(false);
    expect(PARITY_ASSET_BUCKET_PATTERN.test("/")).toBe(false);
  });
});

describe("parityAssetProxyOrigin", () => {
  it("uses the oracle origin, which the proxy serves under", () => {
    expect(parityAssetProxyOrigin({ PARITY_ORACLE_URL: "http://localhost:13050" })).toBe("http://localhost:13050");
  });

  it("keeps only the origin when the oracle URL carries a path", () => {
    expect(parityAssetProxyOrigin({ PARITY_ORACLE_URL: "http://localhost:13050/god-mode/" })).toBe(
      "http://localhost:13050"
    );
  });

  it("derives the namespaced oracle port when no URL is set", () => {
    expect(parityAssetProxyOrigin({ PARITY_NS: "550" })).toBe("http://localhost:13050");
  });
});

describe("bridgeParityAssetBucket", () => {
  it("installs one route on the bucket pattern", async () => {
    const capture = capturingPage();
    await bridgeParityAssetBucket(capture.page, "http://proxy.test");
    expect(capture.page.route).toHaveBeenCalledTimes(1);
    expect(capture.pattern()).toBe(PARITY_ASSET_BUCKET_PATTERN);
  });

  it("continues the request against the proxy with the page's host header", async () => {
    const capture = capturingPage();
    await bridgeParityAssetBucket(capture.page, "http://proxy.test:13050");
    const { route, continued } = fakeRoute("http://localhost:3010/parity19-assets/7f/logo.png?X-Amz-Signature=abc");
    await capture.route()(route);
    expect(continued.url).toBe("http://proxy.test:13050/parity19-assets/7f/logo.png?X-Amz-Signature=abc");
    expect(continued.headers?.["host"]).toBe("localhost:3010");
    expect(continued.headers?.["authorization"]).toBe("sig");
    expect(continued.headers?.["cookie"]).toBe("session=1");
  });

  it("swallows a continue that raced the page away", async () => {
    const capture = capturingPage();
    await bridgeParityAssetBucket(capture.page, "http://proxy.test:13050");
    const { route } = fakeRoute("http://localhost:3010/parity19-assets/7f/logo.png");
    (route.continue as unknown as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("route gone"));
    await expect(capture.route()(route)).resolves.toBeUndefined();
  });
});
