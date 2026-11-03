// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views reads never poll, stream or refetch behind the viewer's back
// (VIEW-043, from parity/specs/views.md §13): one fetch per mount,
// another user's writes visible only after a reload. Window focus and
// reconnect stay quiet through the shared client default (pinned in
// core/query/query.test.ts); these pins hold the views-owned options.
import { describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", () => ({
  getProjectView: vi.fn(),
  getWorkspaceView: vi.fn(),
  listProjectViews: vi.fn(),
  listWorkspaceViews: vi.fn(),
}));

import { projectViewDetailQueryOptions, workspaceViewDetailQueryOptions } from "./detail.js";
import { projectViewsQueryOptions, workspaceViewsQueryOptions } from "./queries.js";

const CLIENT = { name: "stub-client" } as never;

/** Every views-owned read, built over the same stub client. */
function everyViewsRead() {
  return {
    projectList: projectViewsQueryOptions(CLIENT, "acme", "project-1"),
    workspaceList: workspaceViewsQueryOptions(CLIENT, "acme"),
    projectDetail: projectViewDetailQueryOptions(CLIENT, "acme", "project-1", "view-9"),
    workspaceDetail: workspaceViewDetailQueryOptions(CLIENT, "acme", "view-9"),
  };
}

describe("views reads never go live (VIEW-043)", () => {
  it("sets no interval, stream or refetch override on any read", () => {
    for (const [name, options] of Object.entries(everyViewsRead())) {
      expect(options.refetchInterval, `${name}.refetchInterval`).toBeUndefined();
      expect(options.refetchIntervalInBackground, `${name}.refetchIntervalInBackground`).toBeUndefined();
      expect(options.refetchOnMount, `${name}.refetchOnMount`).toBeUndefined();
      expect(options.refetchOnWindowFocus, `${name}.refetchOnWindowFocus`).toBeUndefined();
      expect(options.refetchOnReconnect, `${name}.refetchOnReconnect`).toBeUndefined();
      expect(typeof options.queryFn, `${name}.queryFn`).toBe("function");
    }
  });

  it("reads the detail records through the same one-shot options", async () => {
    const { getProjectView, getWorkspaceView } = await import("@pidash/api-client");
    vi.mocked(getProjectView).mockResolvedValue({} as never);
    vi.mocked(getWorkspaceView).mockResolvedValue({} as never);
    const project = projectViewDetailQueryOptions(CLIENT, "acme", "project-1", "view-9");
    const workspace = workspaceViewDetailQueryOptions(CLIENT, "acme", "view-9");
    expect(project.queryKey).toEqual(["ws", "acme", "issues", "saved-view", "project-1", "view-9"]);
    expect(workspace.queryKey).toEqual(["ws", "acme", "issues", "saved-view", "workspace", "view-9"]);
    if (!project.queryFn || !workspace.queryFn) throw new Error("missing queryFn");
    await project.queryFn({} as never);
    await workspace.queryFn({} as never);
    expect(getProjectView).toHaveBeenCalledWith(CLIENT, "acme", "project-1", "view-9");
    expect(getWorkspaceView).toHaveBeenCalledWith(CLIENT, "acme", "view-9");
  });
});
