// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived tab route (ISS-071): its loader fills the project list the
// body reads, and the pending and body it mounts stay thin.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { createAppClient } from "../../../../../../core/api/client.js";
import { projectsQueryOptions } from "../../../../../../features/projects/index.js";
import { Route } from "./issues.js";

vi.mock("../../../../../../features/issues/contexts/ArchivedIssuesScreen.js", () => ({
  ArchivedIssuesScreen: (props: { workspaceSlug: string; projectId: string }) => (
    <div data-testid="archived-issues-screen" data-ws={props.workspaceSlug} data-project={props.projectId} />
  ),
}));

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

describe("archived issues route", () => {
  it("loads the workspace project list before the body renders", async () => {
    const seen: string[] = [];
    const apiClient = createAppClient("https://api.test", (async (url: string) => {
      seen.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify([
            {
              id: PROJECT,
              name: "Website",
              identifier: "WEB",
              sort_order: 1,
              logo_props: {},
              member_role: 20,
              intake_count: 0,
              archived_at: null,
              workspace: "123e4567-e89b-12d3-a456-426614174000",
              cycle_view: false,
              issue_views_view: true,
              module_view: false,
              page_view: false,
              inbox_view: false,
              is_default: false,
              guest_view_all_features: false,
              project_lead: null,
              network: 2,
              created_at: "2024-01-02T03:04:05Z",
              updated_at: "2024-02-03T04:05:06Z",
              created_by: null,
              updated_by: null,
            },
          ]),
      };
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const loader = Route.options.loader as unknown as (input: unknown) => Promise<unknown>;
    await loader({ context: { queryClient, apiClient }, params: { ws: WS, projectId: PROJECT } });
    expect(queryClient.getQueryData(projectsQueryOptions(apiClient, WS).queryKey)).toHaveLength(1);
    expect(seen.length).toBeGreaterThan(0);
  });

  it("mounts the screen without a route-level pending placeholder", () => {
    // The screen rides its own chunk; a pending component would pin it (and
    // the skeleton module) into the eager route tree (Quality gates).
    expect(Route.options.pendingComponent).toBeUndefined();
  });

  it("mounts the archived screen for the route's project", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: WS, projectId: PROJECT });
    const Component = Route.options.component as unknown as React.ComponentType;
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("archived-issues-screen")).toHaveAttribute("data-project", PROJECT);
  });
});
