// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { Project, ProjectDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { authKeys } from "../../auth/index.js";
import { projectKeys } from "../../projects/index.js";
import { useArchivesProject } from "./project.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_ID = "323e4567-e89b-12d3-a456-426614174002";
const UUID = "123e4567-e89b-12d3-a456-426614174000";

function detailRow(overrides: Partial<ProjectDetail> = {}): ProjectDetail {
  return {
    id: PROJECT_ID,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    archived_at: null,
    workspace: UUID,
    cycle_view: true,
    issue_views_view: false,
    module_view: true,
    page_view: true,
    inbox_view: false,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

function listRow(overrides: Partial<Project> = {}): Project {
  return {
    id: PROJECT_ID,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: UUID,
    cycle_view: true,
    issue_views_view: false,
    module_view: true,
    page_view: true,
    inbox_view: false,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

afterEach(() => {
  resetAppClient();
});

describe("useArchivesProject", () => {
  it("reads null while the project row resolves", () => {
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useArchivesProject("acme", PROJECT_ID), {
      wrapper: wrapper(queryClient),
    });

    expect(result.current).toBeNull();
  });

  it("pairs the row with the joined-projects switcher entries", () => {
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(authKeys.project("acme", PROJECT_ID), detailRow());
    queryClient.setQueryData(projectKeys.list("acme"), [
      listRow(),
      listRow({ id: OTHER_ID, name: "Docs", member_role: null }),
      listRow({ id: OTHER_ID, name: "Mobile", member_role: 10 }),
    ]);
    const { result } = renderHook(() => useArchivesProject("acme", PROJECT_ID), {
      wrapper: wrapper(queryClient),
    });

    expect(result.current?.project.name).toBe("Web");
    expect(result.current?.joined).toEqual([
      { id: PROJECT_ID, name: "Web" },
      { id: OTHER_ID, name: "Mobile" },
    ]);
  });

  it("offers no switcher entries while the projects list resolves", () => {
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(authKeys.project("acme", PROJECT_ID), detailRow());
    const { result } = renderHook(() => useArchivesProject("acme", PROJECT_ID), {
      wrapper: wrapper(queryClient),
    });

    expect(result.current?.project.name).toBe("Web");
    expect(result.current?.joined).toEqual([]);
  });
});
