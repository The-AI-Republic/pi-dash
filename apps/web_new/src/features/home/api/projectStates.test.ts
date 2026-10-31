// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { projectStatesKeys, projectStatesQueryOptions } from "./projectStates.js";

function stubClient(body: unknown) {
  const getJson = vi.fn(async () => body);
  const client = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
  return { client, getJson };
}

describe("projectStatesKeys", () => {
  it("scopes each project's states under the workspace", () => {
    expect(projectStatesKeys.all("acme")).toEqual(["ws", "acme", "home", "states"]);
    expect(projectStatesKeys.list("acme", "project-1")).toEqual(["ws", "acme", "home", "states", "project-1"]);
  });
});

describe("projectStatesQueryOptions", () => {
  it("reads with reference freshness and no silent refetch", () => {
    const options = projectStatesQueryOptions(stubClient([]).client, "acme", "project-1");
    expect(options.queryKey).toEqual(["ws", "acme", "home", "states", "project-1"]);
    expect(options.staleTime).toBe(5 * 60_000);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
  });

  it("lists the states through the contract", async () => {
    const { client, getJson } = stubClient([{ id: "state-1" }]);
    const options = projectStatesQueryOptions(client, "acme", "project-1");
    await expect(options.queryFn?.({} as never)).resolves.toEqual([{ id: "state-1" }]);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/projects/project-1/states/");
  });
});
