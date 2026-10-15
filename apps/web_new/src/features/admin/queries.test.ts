// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { adminCsrfQueryOptions, adminKeys, instanceBootstrapQueryOptions } from "./queries.js";

describe("admin keys", () => {
  it("roots the admin subtree outside the member roots", () => {
    expect(adminKeys.root).toEqual(["admin"]);
    expect(adminKeys.bootstrap).toEqual(["admin", "instance", "bootstrap"]);
    expect(adminKeys.csrf).toEqual(["admin", "csrf"]);
  });
});

describe("instance bootstrap query", () => {
  it("fetches the bootstrap through the client exactly once", async () => {
    const bootstrap = { config: {}, instance: { is_setup_done: true } };
    const getJson = vi.fn(async () => bootstrap);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = instanceBootstrapQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.bootstrap);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(bootstrap);
    expect(getJson).toHaveBeenCalledWith("/api/instances/");
  });

  it("never retries, refetches, or goes stale (ADM-001 single fetch)", () => {
    const options = instanceBootstrapQueryOptions({} as never);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("admin CSRF query", () => {
  it("fetches the forgery token for the native POSTs", async () => {
    const token = { csrf_token: "token-1" };
    const getJson = vi.fn(async () => token);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = adminCsrfQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.csrf);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(token);
    expect(getJson).toHaveBeenCalledWith("/auth/get-csrf-token/", { csrf: false });
  });

  it("holds the token for the page lifetime", () => {
    const options = adminCsrfQueryOptions({} as never);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
    expect(options.refetchOnWindowFocus).toBe(false);
  });
});
