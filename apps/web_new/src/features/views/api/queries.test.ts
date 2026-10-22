// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", () => ({
  listProjectViews: vi.fn(),
  listWorkspaceViews: vi.fn(),
}));

import { listProjectViews, listWorkspaceViews } from "@pidash/api-client";

import { viewKeys } from "./keys.js";
import { projectViewsQueryOptions, workspaceViewsQueryOptions } from "./queries.js";

const CLIENT = { name: "stub-client" } as never;

describe("viewKeys", () => {
  it("scopes the project list under the workspace", () => {
    expect(viewKeys.projectList("acme", "project-1")).toEqual(["ws", "acme", "views", "project", "project-1", "list"]);
  });

  it("scopes the workspace list under the workspace", () => {
    expect(viewKeys.workspaceList("acme")).toEqual(["ws", "acme", "views", "workspace", "list"]);
  });
});

describe("projectViewsQueryOptions", () => {
  it("reads the caller's visible project views", async () => {
    vi.mocked(listProjectViews).mockResolvedValue([]);
    const options = projectViewsQueryOptions(CLIENT, "acme", "project-1");
    expect(options.queryKey).toEqual(["ws", "acme", "views", "project", "project-1", "list"]);
    if (!options.queryFn) throw new Error("missing queryFn");
    await options.queryFn({} as never);
    expect(listProjectViews).toHaveBeenCalledWith(CLIENT, "acme", "project-1");
  });
});

describe("workspaceViewsQueryOptions", () => {
  it("reads the workspace customs newest first", async () => {
    vi.mocked(listWorkspaceViews).mockResolvedValue([]);
    const options = workspaceViewsQueryOptions(CLIENT, "acme");
    expect(options.queryKey).toEqual(["ws", "acme", "views", "workspace", "list"]);
    if (!options.queryFn) throw new Error("missing queryFn");
    await options.queryFn({} as never);
    expect(listWorkspaceViews).toHaveBeenCalledWith(CLIENT, "acme", { orderBy: "-created_at" });
  });
});
