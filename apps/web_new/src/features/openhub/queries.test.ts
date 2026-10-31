// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { keepPreviousData } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { LIST_STALE_TIME_MS } from "../../core/query/client.js";
import { openhubKeys } from "./api/keys.js";
import {
  APPS_CATALOG_PAGE_SIZE,
  APPS_SEARCH_DEBOUNCE_MS,
  marketplacePageQueryOptions,
  openhubStatusQueryOptions,
} from "./api/queries.js";

function fakeClient() {
  const getJson = vi.fn(async (_path: string, _options?: unknown): Promise<unknown> => ({}));
  const client = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
  return { client, getJson };
}

describe("openhub keys", () => {
  it("roots the area under the workspace scope", () => {
    expect(openhubKeys.all("acme")).toEqual(["ws", "acme", "openhub"]);
    expect(openhubKeys.status("acme")).toEqual(["ws", "acme", "openhub", "status"]);
    expect(openhubKeys.marketplace("acme")).toEqual(["ws", "acme", "openhub", "marketplace"]);
  });

  it("keys each window by its settled query plus cursor", () => {
    expect(openhubKeys.marketplacePage("acme", "", null)).toEqual(["ws", "acme", "openhub", "marketplace", "", null]);
    expect(openhubKeys.marketplacePage("acme", "notes", "cursor-2")).toEqual([
      "ws",
      "acme",
      "openhub",
      "marketplace",
      "notes",
      "cursor-2",
    ]);
  });
});

describe("openhub status query", () => {
  it("reads the catalog gate", async () => {
    const status = { connected: true, has_gateway_scopes: true, hub_url: "https://hub.test" };
    const getJson = vi.fn(async () => status);
    const client = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = openhubStatusQueryOptions(client, "acme");
    expect(options.queryKey).toEqual(openhubKeys.status("acme"));
    expect(options.enabled).toBe(true);
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    expect(options.retry).toBe(false);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(status);
    expect(getJson).toHaveBeenCalledWith("/api/openhub/status/");
  });

  it("stays disabled without a workspace", () => {
    expect(openhubStatusQueryOptions(fakeClient().client, "").enabled).toBe(false);
  });
});

describe("marketplace page query", () => {
  it("pins the catalog window size and the search debounce", () => {
    expect(APPS_CATALOG_PAGE_SIZE).toBe(24);
    expect(APPS_SEARCH_DEBOUNCE_MS).toBe(300);
  });

  it("reads the first window with only the page size (CLOUD-023)", async () => {
    const { client, getJson } = fakeClient();
    const options = marketplacePageQueryOptions(client, "acme", { query: "", cursor: null });
    expect(options.queryKey).toEqual(openhubKeys.marketplacePage("acme", "", null));
    expect(options.enabled).toBe(true);
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    expect(options.retry).toBe(false);
    expect(options.placeholderData).toBe(keepPreviousData);
    await options.queryFn?.({} as never);
    expect(getJson).toHaveBeenCalledWith("/api/openhub/apps/marketplace/", {
      query: { q: undefined, cursor: undefined, limit: 24, resourceKind: undefined },
    });
  });

  it("carries the settled query and cursor into later windows (CLOUD-023)", async () => {
    const { client, getJson } = fakeClient();
    const options = marketplacePageQueryOptions(client, "acme", { query: "notes", cursor: "cursor-2" });
    expect(options.queryKey).toEqual(openhubKeys.marketplacePage("acme", "notes", "cursor-2"));
    await options.queryFn?.({} as never);
    expect(getJson).toHaveBeenCalledWith("/api/openhub/apps/marketplace/", {
      query: { q: "notes", cursor: "cursor-2", limit: 24, resourceKind: undefined },
    });
  });

  it("stays disabled without a workspace", () => {
    expect(marketplacePageQueryOptions(fakeClient().client, "", { query: "", cursor: null }).enabled).toBe(false);
  });
});
