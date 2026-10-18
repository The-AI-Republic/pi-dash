// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Automations route composition: the route mounts the automations
// settings screen with its params inside the gated shell. No loader
// prefetch — the page reads its own data, so the skeleton paints.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../../core/api/client.js";
import { membershipQueryOptions, projectRolesQueryOptions } from "../../../../../../features/auth/index.js";
import { automationStatesQueryOptions } from "../../../../../../features/automations/index.js";
import type { ProjectDetails } from "@pidash/api-client";

import { projectDetailsQueryOptions } from "../../../../../../features/projects/index.js";
import { Route } from "./automations.js";

const PARAMS = { ws: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001" };

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("automations route", () => {
  it("carries no loader prefetch", () => {
    expect(Route.options.beforeLoad).toBeUndefined();
  });

  it("mounts the automations screen with the route params", () => {
    vi.spyOn(Route, "useParams").mockReturnValue(PARAMS);
    const apiClient = createAppClient("https://api.test", (() => {
      throw new Error("unexpected network fetch in a seeded render");
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(projectDetailsQueryOptions(apiClient, PARAMS.ws).queryKey, [
      { id: PARAMS.projectId, archive_in: 0, close_in: 0, default_state: null } as ProjectDetails,
    ]);
    queryClient.setQueryData(automationStatesQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId).queryKey, []);
    queryClient.setQueryData(projectRolesQueryOptions(apiClient, PARAMS.ws).queryKey, { [PARAMS.projectId]: 20 });
    queryClient.setQueryData(membershipQueryOptions(apiClient, PARAMS.ws).queryKey, { role: 20 } as never);
    const Component = Route.options.component;
    if (!Component) throw new Error("automations route has no component");
    render(
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    );
    expect(screen.getByRole("heading", { name: "Automations" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 4, name: "Auto-archive closed work items" })).toBeInTheDocument();
    expect(screen.getByTestId("automations-extension")).toBeEmptyDOMElement();
  });
});
