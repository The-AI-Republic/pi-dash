// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";
import type { MiddlewareContext, TransportResponse } from "../client.js";
import { ApiError } from "../errors.js";
import { CLOUD_REFRESH_PATH, createCloudRefreshMiddleware, isRefreshCall } from "./cloud-refresh.js";

type Next = (request: MiddlewareContext) => Promise<TransportResponse>;

function answer(status: number, url = "https://api.test/api/issues/"): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => "",
  };
}

const call = (url = "https://api.test/api/issues/"): MiddlewareContext => ({ method: "GET", url, headers: {} });

describe("isRefreshCall", () => {
  it("matches the rotation path regardless of query", () => {
    expect(isRefreshCall(`https://api.test${CLOUD_REFRESH_PATH}`)).toBe(true);
    expect(isRefreshCall(`https://api.test${CLOUD_REFRESH_PATH}?next=/x`)).toBe(true);
  });

  it("rejects other paths and unparseable values", () => {
    expect(isRefreshCall("https://api.test/api/issues/")).toBe(false);
    expect(isRefreshCall("https://api.test/api/auth/refresh")).toBe(false);
    expect(isRefreshCall("::::")).toBe(false);
  });
});

describe("createCloudRefreshMiddleware", () => {
  it("passes non-401 answers through without rotating", async () => {
    const next = vi.fn(async () => answer(200));
    const middleware = createCloudRefreshMiddleware();
    const response = await middleware(call(), next as Next);
    expect(response.status).toBe(200);
    expect(next).toHaveBeenCalledTimes(1);
  });

  it("rotates once and replays the original request after a 401", async () => {
    const seen: MiddlewareContext[] = [];
    const next = vi.fn(async (request: MiddlewareContext) => {
      seen.push(request);
      if (request.url.endsWith(CLOUD_REFRESH_PATH)) return answer(200, request.url);
      return seen.filter((entry) => !entry.url.endsWith(CLOUD_REFRESH_PATH)).length > 1
        ? answer(200, request.url)
        : answer(401, request.url);
    });
    const middleware = createCloudRefreshMiddleware();
    const response = await middleware(call(), next as Next);
    expect(response.status).toBe(200);
    // Original, rotation, replay — and the rotation posts without a body.
    expect(seen).toHaveLength(3);
    expect(seen[1]).toMatchObject({ method: "POST", url: `https://api.test${CLOUD_REFRESH_PATH}` });
    expect(seen[1]?.body).toBeUndefined();
    expect(seen[2]).toMatchObject({ method: "GET", url: "https://api.test/api/issues/" });
  });

  it("returns the original 401 without replaying when the rotation is rejected", async () => {
    for (const status of [401, 403]) {
      const next = vi.fn(async (request: MiddlewareContext) =>
        request.url.endsWith(CLOUD_REFRESH_PATH) ? answer(status, request.url) : answer(401, request.url)
      );
      const middleware = createCloudRefreshMiddleware();
      const response = await middleware(call(), next as Next);
      expect(response.status).toBe(401);
      expect(next).toHaveBeenCalledTimes(2);
    }
  });

  it("throws a transient error when the rotation answers outside 2xx/401/403", async () => {
    for (const status of [150, 500]) {
      const next = vi.fn(async (request: MiddlewareContext) =>
        request.url.endsWith(CLOUD_REFRESH_PATH) ? answer(status, request.url) : answer(401, request.url)
      );
      const middleware = createCloudRefreshMiddleware();
      const error = await middleware(call(), next as Next).catch((failure: unknown) => failure);
      expect(error).toBeInstanceOf(ApiError);
      expect((error as ApiError).status).toBe(0);
      expect(next).toHaveBeenCalledTimes(2);
    }
  });

  it("propagates a thrown rotation untouched", async () => {
    const failure = new Error("socket hang up");
    const next = vi.fn(async (request: MiddlewareContext) => {
      if (request.url.endsWith(CLOUD_REFRESH_PATH)) throw failure;
      return answer(401, request.url);
    });
    const middleware = createCloudRefreshMiddleware();
    await expect(middleware(call(), next as Next)).rejects.toBe(failure);
  });

  it("never rotates or replays the rotation call itself", async () => {
    const next = vi.fn(async (request: MiddlewareContext) => answer(401, request.url));
    const middleware = createCloudRefreshMiddleware();
    const response = await middleware(call(`https://api.test${CLOUD_REFRESH_PATH}`), next as Next);
    expect(response.status).toBe(401);
    expect(next).toHaveBeenCalledTimes(1);
  });

  it("shares one rotation across concurrent 401s and replays each once", async () => {
    let rotations = 0;
    let replays = 0;
    const next = vi.fn(async (request: MiddlewareContext) => {
      if (request.url.endsWith(CLOUD_REFRESH_PATH)) {
        rotations += 1;
        await new Promise((resolve) => setTimeout(resolve, 5));
        return answer(200, request.url);
      }
      if (rotations > 0) {
        replays += 1;
        return answer(200, request.url);
      }
      return answer(401, request.url);
    });
    const middleware = createCloudRefreshMiddleware();
    const [first, second] = await Promise.all([
      middleware(call("https://api.test/api/a/"), next as Next),
      middleware(call("https://api.test/api/b/"), next as Next),
    ]);
    expect(first.status).toBe(200);
    expect(second.status).toBe(200);
    expect(rotations).toBe(1);
    expect(replays).toBe(2);
  });

  it("returns a rejected replay as-is without a second rotation", async () => {
    let rotations = 0;
    const next = vi.fn(async (request: MiddlewareContext) => {
      if (request.url.endsWith(CLOUD_REFRESH_PATH)) {
        rotations += 1;
        return answer(200, request.url);
      }
      return answer(401, request.url);
    });
    const middleware = createCloudRefreshMiddleware();
    const response = await middleware(call(), next as Next);
    expect(response.status).toBe(401);
    expect(rotations).toBe(1);
    expect(next).toHaveBeenCalledTimes(3);
  });

  it("leaves the verdict to the outer layers when no origin can be derived", async () => {
    const next = vi.fn(async (request: MiddlewareContext) => answer(401, request.url));
    const middleware = createCloudRefreshMiddleware();
    const response = await middleware(call("::::"), next as Next);
    expect(response.status).toBe(401);
    expect(next).toHaveBeenCalledTimes(1);
  });
});
