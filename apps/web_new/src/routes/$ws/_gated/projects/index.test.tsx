// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import type { ProjectDetails, Workspace } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the route wiring.
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { params?: Record<string, string>; children?: React.ReactNode }) => (
      <a href={`/acme/projects/${props.params?.["projectId"] ?? ""}/issues`}>{props.children}</a>
    ),
    useNavigate: () => vi.fn(),
  };
});

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { sessionKeys } from "../../../../core/session/index.js";
import { projectKeys } from "../../../../features/projects/api/keys.js";
import { Route } from "./index.js";

function renderList(): void {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
  const Component = Route.options.component;
  if (!Component) throw new Error("projects route has no component");
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(projectKeys.details("acme"), [
    {
      id: "223e4567-e89b-12d3-a456-426614174001",
      created_at: "2024-01-02T03:04:05Z",
      updated_at: "2024-02-03T04:05:06Z",
      created_by: null,
      updated_by: null,
      deleted_at: null,
      workspace: "123e4567-e89b-12d3-a456-426614174000",
      name: "Alpha",
      description: "",
      description_text: null,
      description_html: null,
      network: 2,
      identifier: "ALP",
      default_assignee: null,
      project_lead: null,
      emoji: null,
      icon_prop: null,
      module_view: false,
      cycle_view: false,
      issue_views_view: false,
      page_view: false,
      intake_view: false,
      inbox_view: false,
      is_time_tracking_enabled: false,
      is_issue_type_enabled: false,
      is_default: false,
      guest_view_all_features: false,
      members_can_edit_states: false,
      cover_image: null,
      cover_image_asset: null,
      cover_image_url: null,
      estimate: null,
      archive_in: 0,
      close_in: 0,
      logo_props: {},
      default_state: null,
      archived_at: null,
      timezone: "UTC",
      external_source: null,
      external_id: null,
      repo_url: "",
      base_branch: "",
      is_favorite: false,
      sort_order: null,
      member_role: 20,
      anchor: null,
      members: [],
      next_work_item_sequence: 1,
      agent_executor_options: [
        { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
        { kind: "local_runner", available: true, reason_code: "" },
        { kind: "managed_runner", available: false, reason_code: "managed_runner_disabled" },
      ],
      default_agent_executor: "local_runner",
    } satisfies ProjectDetails,
  ]);
  queryClient.setQueryData(sessionKeys.workspaces, [
    {
      id: "123e4567-e89b-12d3-a456-426614174000",
      name: "Acme",
      slug: "acme",
      logo_url: null,
      total_members: 2,
      role: 20,
    } satisfies Workspace,
  ]);
  render(
    <QueryClientProvider client={queryClient}>
      <Component />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("projects list route", () => {
  it("mounts the connected list for the workspace slug", async () => {
    renderList();

    expect(await screen.findByLabelText("Projects")).not.toBeNull();
    expect(screen.getByRole("heading", { name: "Alpha" })).not.toBeNull();
    expect(screen.getByRole("button", { name: /Project/ })).not.toBeNull();
  });
});
