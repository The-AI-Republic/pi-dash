// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { recentsQueryOptions, recentsKeys } from "./recents.js";

function stubClient(body: unknown) {
  const getJson = vi.fn(async (_path: string, _options?: unknown) => body);
  const client = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
  return { client, getJson };
}

describe("recentsKeys", () => {
  it("scopes the feed and each filter under the workspace", () => {
    expect(recentsKeys.all("acme")).toEqual(["ws", "acme", "home", "recents"]);
    expect(recentsKeys.list("acme", "issue")).toEqual(["ws", "acme", "home", "recents", "issue"]);
  });
});

describe("recentsQueryOptions", () => {
  it("keys the filter with list freshness and no silent refetch", () => {
    const options = recentsQueryOptions(stubClient([]).client, "acme", "page");
    expect(options.queryKey).toEqual(["ws", "acme", "home", "recents", "page"]);
    expect(options.staleTime).toBe(30_000);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
  });

  it("lists visits through the contract with the filter key", async () => {
    const { client, getJson } = stubClient([{ id: "visit-1" }]);
    const options = recentsQueryOptions(client, "acme", "project");
    await expect(options.queryFn?.({} as never)).resolves.toEqual([{ id: "visit-1" }]);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/recent-visits/", {
      query: { entity_name: "project" },
    });
  });

  it("omits the entity param for the All filter", async () => {
    const { client, getJson } = stubClient([]);
    const options = recentsQueryOptions(client, "acme", "all");
    await options.queryFn?.({} as never);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/recent-visits/", {});
  });
});
