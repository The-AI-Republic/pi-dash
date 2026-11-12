// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Labels route composition: the route mounts the labels settings
// screen with its params. No loader prefetch by design — the list
// loads client-side so the skeleton renders while pending (ISS-230).
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
import { labelsQueryOptions } from "../../../../../../features/issues/index.js";
import { Route } from "./labels.js";

const PARAMS = { ws: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001" };

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("labels route", () => {
  it("carries no loader prefetch", () => {
    expect(Route.options.beforeLoad).toBeUndefined();
  });

  it("mounts the settings screen with the route params", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue(PARAMS);
    const apiClient = createAppClient("https://api.test", (() => {
      throw new Error("unexpected network fetch in a seeded render");
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(labelsQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId).queryKey, []);
    queryClient.setQueryData(projectQueryOptions(apiClient, PARAMS.ws, PARAMS.projectId).queryKey, {
      id: PARAMS.projectId,
      name: "Website relaunch",
      identifier: "WEB",
      logo_props: {},
    } as never);
    queryClient.setQueryData(projectRolesQueryOptions(apiClient, PARAMS.ws).queryKey, {
      [PARAMS.projectId]: 20,
    });
    queryClient.setQueryData(membershipQueryOptions(apiClient, PARAMS.ws).queryKey, {
      role: 20,
    } as never);
    const Component = Route.options.component;
    if (!Component) throw new Error("labels route has no component");
    render(
      <QueryClientProvider client={queryClient}>
        {renderInRouter(`/acme/settings/projects/${PARAMS.projectId}/`, <Component />)}
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByRole("heading", { level: 1, name: "Labels" })).toBeInTheDocument());
    expect(screen.getByTestId("project-settings-page")).toBeInTheDocument();
    expect(screen.getByText("No labels yet")).toBeInTheDocument();
    expect(screen.getByTestId("label-add")).toBeInTheDocument();
  });
});
