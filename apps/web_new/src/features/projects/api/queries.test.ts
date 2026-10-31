// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", () => ({
  listProjectDetails: vi.fn(),
  listProjects: vi.fn(),
}));

import { listProjectDetails, listProjects } from "@pidash/api-client";

import { projectKeys } from "./keys.js";
import { projectDetailsQueryOptions, projectsQueryOptions } from "./queries.js";

const CLIENT = { name: "stub-client" } as never;

describe("projectKeys", () => {
  it("scopes the collection reads under the workspace", () => {
    expect(projectKeys.list("acme")).toEqual(["ws", "acme", "projects", "list"]);
    expect(projectKeys.details("acme")).toEqual(["ws", "acme", "projects", "details"]);
  });
});

describe("projectDetailsQueryOptions", () => {
  it("reads the rich collection once per workspace", async () => {
    vi.mocked(listProjectDetails).mockResolvedValue([]);
    const options = projectDetailsQueryOptions(CLIENT, "acme");
    expect(options.queryKey).toEqual(["ws", "acme", "projects", "details"]);
    if (!options.queryFn) throw new Error("missing queryFn");
    await options.queryFn({} as never);
    expect(listProjectDetails).toHaveBeenCalledWith(CLIENT, "acme");
  });
});

describe("projectsQueryOptions", () => {
  it("reads the lightweight list", async () => {
    vi.mocked(listProjects).mockResolvedValue([]);
    const options = projectsQueryOptions(CLIENT, "acme");
    expect(options.queryKey).toEqual(["ws", "acme", "projects", "list"]);
    if (!options.queryFn) throw new Error("missing queryFn");
    await options.queryFn({} as never);
    expect(listProjects).toHaveBeenCalledWith(CLIENT, "acme");
  });
});
