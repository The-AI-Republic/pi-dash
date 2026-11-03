// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { useProjectProperties, useWorkspaceProperties } from "./tabPreferences.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

const WORKSPACE_PROPS = {
  id: UUID,
  workspace: UUID2,
  user: UUID,
  filters: {},
  display_filters: {},
  display_properties: {},
  rich_filters: {},
  navigation_project_limit: 10,
  navigation_control_preference: "TABBED",
  created_by: null,
  updated_by: null,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-01T00:00:00Z",
};

const PROJECT_PROPS = {
  ...WORKSPACE_PROPS,
  project: UUID2,
  preferences: { navigation: { default_tab: "cycles", hide_in_more_menu: ["pages"] } },
  sort_order: 1,
};

function stubClient() {
  return createAppClient("https://api.test", (async (url: string) => {
    let status = 404;
    let body = "{}";
    if (url.endsWith("/auth/get-csrf-token/")) {
      status = 200;
      body = JSON.stringify({ csrf_token: "csrf" });
    } else if (url.includes(`/projects/${UUID2}/user-properties/`)) {
      status = 200;
      body = JSON.stringify(PROJECT_PROPS);
    } else if (url.endsWith("/user-properties/")) {
      status = 200;
      body = JSON.stringify(WORKSPACE_PROPS);
    }
    return { status, url, headers: { get: () => null }, text: async () => body };
  }) as never);
}

function WorkspaceProbe(): React.ReactElement {
  const query = useWorkspaceProperties("acme");
  if (query.data === undefined) return <p>loading</p>;
  return <p>{JSON.stringify(query.data)}</p>;
}

function ProjectProbe(): React.ReactElement {
  const query = useProjectProperties("acme", UUID2);
  if (query.data === undefined) return <p>loading</p>;
  return <p>{JSON.stringify(query.data)}</p>;
}

function renderProbe(name: "workspace" | "project") {
  stubClient();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      {name === "workspace" ? <WorkspaceProbe /> : <ProjectProbe />}
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("property queries", () => {
  it("reads the workspace display properties", async () => {
    renderProbe("workspace");
    await waitFor(() => expect(screen.getByText(/TABBED/)).not.toBeNull());
  });

  it("reads the project display properties", async () => {
    renderProbe("project");
    await waitFor(() => expect(screen.getByText(/cycles/)).not.toBeNull());
  });
});
