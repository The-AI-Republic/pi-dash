// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../core/query/client.js";
import {
  pickerCyclesQueryOptions,
  pickerKeys,
  pickerModulesQueryOptions,
  pickerProjectMembersQueryOptions,
  pickerProjectsQueryOptions,
  pickerStatesQueryOptions,
  pickerWorkspaceCyclesQueryOptions,
  pickerWorkspaceMembersQueryOptions,
  pickerWorkspaceModulesQueryOptions,
  pickerWorkspaceStatesQueryOptions,
} from "./reference.js";

const USER = "11111111-1111-4111-8111-111111111111";
const PROJECT = "22222222-2222-4222-8222-222222222222";
const WORKSPACE = "33333333-3333-4333-8333-333333333333";
const OTHER = "44444444-4444-4444-8444-444444444444";

const STATE = {
  id: OTHER,
  project_id: PROJECT,
  workspace_id: WORKSPACE,
  name: "Todo",
  color: "#5e6ad2",
  group: "unstarted",
  default: true,
  description: "",
  sequence: 1,
};

const MEMBER = {
  id: OTHER,
  role: 15,
  member: USER,
  project: PROJECT,
  original_role: 15,
  created_at: "2026-01-01T00:00:00.000Z",
};

const DIRECTORY_ENTRY = {
  id: OTHER,
  member: {
    id: USER,
    first_name: "Parity",
    last_name: "User",
    avatar: "",
    avatar_url: null,
    display_name: "Parity User",
    is_bot: false,
  },
  role: 15,
  is_active: true,
};

const PROJECT_ROW = {
  id: PROJECT,
  name: "Seed",
  identifier: "SEED",
  sort_order: null,
  logo_props: {},
  member_role: 15,
  intake_count: 0,
  archived_at: null,
  workspace: WORKSPACE,
  cycle_view: false,
  issue_views_view: false,
  module_view: false,
  page_view: false,
  inbox_view: false,
  is_default: false,
  guest_view_all_features: false,
  project_lead: null,
  network: 0,
  created_at: "2026-01-01T00:00:00.000Z",
  updated_at: "2026-01-01T00:00:00.000Z",
  created_by: null,
  updated_by: null,
};

function harness(routes: Record<string, unknown>) {
  const calls: string[] = [];
  const transport = async (url: string) => {
    calls.push(url);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => '{"csrf_token":"csrf"}',
      };
    }
    const match = Object.entries(routes).find(([path]) => url.includes(path));
    if (match === undefined) throw new Error(`unexpected url ${url}`);
    return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(match[1]) };
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return { queryClient, calls };
}

afterEach(() => {
  resetAppClient();
});

describe("pickerKeys", () => {
  it("scopes every read under the workspace", () => {
    expect(pickerKeys.states("acme", PROJECT)).toEqual(["ws", "acme", "pickers", "states", PROJECT]);
    expect(pickerKeys.projectMembers("acme", PROJECT)).toEqual(["ws", "acme", "pickers", "project-members", PROJECT]);
    expect(pickerKeys.workspaceMembers("acme")).toEqual(["ws", "acme", "pickers", "workspace-members"]);
    expect(pickerKeys.projects("acme")).toEqual(["ws", "acme", "pickers", "projects"]);
    expect(pickerKeys.cycles("acme", PROJECT)).toEqual(["ws", "acme", "pickers", "cycles", PROJECT]);
    expect(pickerKeys.modules("acme", PROJECT)).toEqual(["ws", "acme", "pickers", "modules", PROJECT]);
    expect(pickerKeys.workspaceStates("acme")).toEqual(["ws", "acme", "pickers", "workspace-states"]);
    expect(pickerKeys.workspaceCycles("acme")).toEqual(["ws", "acme", "pickers", "workspace-cycles"]);
    expect(pickerKeys.workspaceModules("acme")).toEqual(["ws", "acme", "pickers", "workspace-modules"]);
  });
});

describe("picker reference reads", () => {
  it("reads project states", async () => {
    const { queryClient } = harness({ "/states/": [STATE] });
    const options = pickerStatesQueryOptions(getAppClient(), "acme", PROJECT);
    expect(options.staleTime).toBe(REFERENCE_STALE_TIME_MS);
    const rows = await queryClient.fetchQuery(options);
    expect(rows).toHaveLength(1);
    expect(rows[0]?.name).toBe("Todo");
  });

  it("reads project memberships", async () => {
    const { queryClient } = harness({ "/members/": [MEMBER] });
    const rows = await queryClient.fetchQuery(pickerProjectMembersQueryOptions(getAppClient(), "acme", PROJECT));
    expect(rows).toEqual([
      {
        id: OTHER,
        role: 15,
        member: USER,
        project: PROJECT,
        original_role: 15,
        created_at: "2026-01-01T00:00:00.000Z",
      },
    ]);
  });

  it("reads the workspace member directory with suspended flags", async () => {
    const suspended = { ...DIRECTORY_ENTRY, is_active: false };
    const { queryClient } = harness({ "/members/": [DIRECTORY_ENTRY, suspended] });
    const rows = await queryClient.fetchQuery(pickerWorkspaceMembersQueryOptions(getAppClient(), "acme"));
    expect(rows.map((row) => row.is_active)).toEqual([true, false]);
  });

  it("reads joined projects", async () => {
    const { queryClient } = harness({ "/projects/": [PROJECT_ROW] });
    const rows = await queryClient.fetchQuery(pickerProjectsQueryOptions(getAppClient(), "acme"));
    expect(rows.map((row) => row.name)).toEqual(["Seed"]);
  });

  it("reads thin cycle rows", async () => {
    const { queryClient } = harness({
      "/cycles/": [{ id: OTHER, project_id: PROJECT, name: "Current", is_favorite: false, status: "ACTIVE" }],
    });
    const rows = await queryClient.fetchQuery(pickerCyclesQueryOptions(getAppClient(), "acme", PROJECT));
    expect(rows.map((row) => row.status)).toEqual(["ACTIVE"]);
  });

  it("reads thin module rows", async () => {
    const { queryClient } = harness({
      "/modules/": [{ id: OTHER, project_id: PROJECT, name: "Alpha", status: "planned", is_favorite: false }],
    });
    const rows = await queryClient.fetchQuery(pickerModulesQueryOptions(getAppClient(), "acme", PROJECT));
    expect(rows.map((row) => row.name)).toEqual(["Alpha"]);
  });

  it("reads workspace states without a project segment", async () => {
    const { queryClient, calls } = harness({ "/states/": [STATE] });
    const options = pickerWorkspaceStatesQueryOptions(getAppClient(), "acme");
    expect(options.staleTime).toBe(REFERENCE_STALE_TIME_MS);
    const rows = await queryClient.fetchQuery(options);
    expect(rows.map((row) => row.name)).toEqual(["Todo"]);
    expect(calls.some((url) => url.endsWith("/api/workspaces/acme/states/"))).toBe(true);
  });

  it("reads workspace cycles in the narrower projection", async () => {
    const { queryClient, calls } = harness({
      "/cycles/": [
        {
          id: OTHER,
          workspace_id: WORKSPACE,
          project_id: PROJECT,
          name: "Current",
          description: "",
          start_date: null,
          end_date: null,
          owned_by_id: null,
          view_props: {},
          sort_order: 1,
          external_source: null,
          external_id: null,
          progress_snapshot: null,
          logo_props: {},
          total_issues: 2,
          cancelled_issues: 0,
          completed_issues: 1,
          started_issues: 1,
          unstarted_issues: 0,
          backlog_issues: 0,
        },
      ],
    });
    const rows = await queryClient.fetchQuery(pickerWorkspaceCyclesQueryOptions(getAppClient(), "acme"));
    expect(rows.map((row) => row.name)).toEqual(["Current"]);
    expect(calls.some((url) => url.endsWith("/api/workspaces/acme/cycles/"))).toBe(true);
  });

  it("reads workspace modules without the favorite flag", async () => {
    const { queryClient, calls } = harness({
      "/modules/": [{ id: OTHER, project_id: PROJECT, name: "Alpha", status: "planned" }],
    });
    const rows = await queryClient.fetchQuery(pickerWorkspaceModulesQueryOptions(getAppClient(), "acme"));
    expect(rows.map((row) => row.name)).toEqual(["Alpha"]);
    expect(calls.some((url) => url.endsWith("/api/workspaces/acme/modules/"))).toBe(true);
  });
});
