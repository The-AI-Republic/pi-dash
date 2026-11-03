// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { widgetPrefsKeys, widgetPrefsQueryOptions } from "./widgetPrefs.js";

function stubClient(body: unknown) {
  const getJson = vi.fn(async () => body);
  const client = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
  return { client, getJson };
}

describe("widgetPrefsKeys", () => {
  it("scopes the map under the workspace", () => {
    expect(widgetPrefsKeys.all("acme")).toEqual(["ws", "acme", "home", "widget-prefs"]);
    expect(widgetPrefsKeys.list("acme")).toEqual(["ws", "acme", "home", "widget-prefs", "list"]);
  });
});

describe("widgetPrefsQueryOptions", () => {
  it("reads with list freshness and no silent refetch", () => {
    const options = widgetPrefsQueryOptions(stubClient([]).client, "acme");
    expect(options.queryKey).toEqual(["ws", "acme", "home", "widget-prefs", "list"]);
    expect(options.staleTime).toBe(30_000);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
  });

  it("lists the map through the contract", async () => {
    const { client, getJson } = stubClient([{ key: "recents" }]);
    const options = widgetPrefsQueryOptions(client, "acme");
    await expect(options.queryFn?.({} as never)).resolves.toEqual([{ key: "recents" }]);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/home-preferences/");
  });
});
