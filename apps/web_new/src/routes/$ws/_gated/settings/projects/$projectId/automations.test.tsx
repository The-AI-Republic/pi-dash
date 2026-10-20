// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Automations route composition: the route mounts the automations
// settings screen with its params inside the gated shell. No loader
// prefetch — the page reads its own data, so the skeleton paints.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { renderInRouter } from "../../../../../../features/project-settings/testing.js";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../../core/api/client.js";
import {
  membershipQueryOptions,
  projectQueryOptions,
  projectRolesQueryOptions,
} from "../../../../../../features/auth/index.js";
import { automationStatesQueryOptions } from "../../../../../../features/automations/index.js";
import type { ProjectDetails } from "@pidash/api-client";

import { projectDetailsQueryOptions } from "../../../../../../features/projects/index.js";
import { Route } from "./automations.js";

const PARAMS = { ws: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001" };

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

/** Seed every read the shell and the tab need for one caller. */
function seed(
  queryClient: QueryClient,
  apiClient: ReturnType<typeof createAppClient>,
  options: {
    projectRole: number | null;
    workspaceRole: number;
  }
): void {
  queryClient.setQueryData(projectDetailsQueryOptions(apiClient, PARAMS.ws).queryKey, [
    { id: PARAMS.projectId, archive_in: 0, close_in: 0, default_state: null } as ProjectDetails,
  ]);
  queryClient.setQueryData(automationStatesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId).queryKey, []);
  queryClient.setQueryData(projectQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId).queryKey, {
    id: PARAMS.projectId,
    name: "Website relaunch",
    identifier: "WEB",
    logo_props: {},
  } as never);
  queryClient.setQueryData(
    projectRolesQueryOptions(apiClient, PARAMS.ws).queryKey,
    options.projectRole === null ? {} : { [PARAMS.projectId]: options.projectRole }
  );
  queryClient.setQueryData(membershipQueryOptions(apiClient, PARAMS.ws).queryKey, {
    role: options.workspaceRole,
  } as never);
}

function renderRoute(queryClient: QueryClient): void {
  const Component = Route.options.component;
  if (!Component) throw new Error("automations route has no component");
  render(
    <QueryClientProvider client={queryClient}>
      {renderInRouter(`/acme/settings/projects/${PARAMS.projectId}/`, <Component />)}
    </QueryClientProvider>
  );
}

describe("automations route", () => {
  it("carries no loader prefetch", () => {
    expect(Route.options.beforeLoad).toBeUndefined();
  });

  it("mounts the automations screen with the route params", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue(PARAMS);
    const apiClient = createAppClient("https://api.test", (() => {
      throw new Error("unexpected network fetch in a seeded render");
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    seed(queryClient, apiClient, { projectRole: 20, workspaceRole: 20 });
    renderRoute(queryClient);
    await waitFor(() => expect(screen.getByRole("heading", { level: 1, name: "Automations" })).toBeInTheDocument());
    expect(screen.getByTestId("project-settings-page")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 4, name: "Auto-archive closed work items" })).toBeInTheDocument();
    expect(screen.getByTestId("automations-extension")).toBeEmptyDOMElement();
  });

  it("renders the feature's own denial for a project member and keeps the rail", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue(PARAMS);
    const apiClient = createAppClient("https://api.test", (() => {
      throw new Error("unexpected network fetch in a seeded render");
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    seed(queryClient, apiClient, { projectRole: 15, workspaceRole: 15 });
    renderRoute(queryClient);

    await waitFor(() => expect(screen.getByTestId("automations-not-authorized")).toBeInTheDocument());
    // The generic gate must not shadow the screen's refusal, and the
    // rail stays beside it (PSET-053/054's denial scenario).
    expect(screen.queryByTestId("project-settings-denied")).not.toBeInTheDocument();
    expect(screen.getByTestId("project-settings-sidebar")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 4, name: "Auto-archive closed work items" })).not.toBeInTheDocument();
  });

  it("admits a workspace admin whose project role reads member", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue(PARAMS);
    const apiClient = createAppClient("https://api.test", (() => {
      throw new Error("unexpected network fetch in a seeded render");
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    seed(queryClient, apiClient, { projectRole: 15, workspaceRole: 20 });
    renderRoute(queryClient);

    await waitFor(() =>
      expect(screen.getByRole("heading", { level: 4, name: "Auto-archive closed work items" })).toBeInTheDocument()
    );
    expect(screen.queryByTestId("automations-not-authorized")).not.toBeInTheDocument();
  });
});
