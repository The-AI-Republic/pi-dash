// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mockNavigate = vi.fn();

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => mockNavigate,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { DefaultTabRedirect } from "./DefaultTabRedirect.js";

const P1 = "111e4567-e89b-12d3-a456-426614174001";
const UUID = "123e4567-e89b-12d3-a456-426614174000";

function projectRow(overrides: Record<string, unknown> = {}) {
  return {
    id: P1,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: UUID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: false,
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

function propsPayload(navigation: Record<string, unknown>) {
  return {
    id: UUID,
    workspace: UUID,
    project: P1,
    user: UUID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    preferences: { navigation },
    sort_order: 1,
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

interface StubOptions {
  navigation?: Record<string, unknown>;
  propsStatus?: number;
  projects?: unknown[];
  projectsStatus?: number;
  roles?: Record<string, number>;
  rolesStatus?: number;
  hang?: boolean;
}

function setup(options: StubOptions = {}) {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (options.hang === true) return new Promise<never>(() => undefined);
    if (url.includes("/project-roles/")) {
      const status = options.rolesStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => (status === 200 ? JSON.stringify(options.roles ?? { [P1]: 20 }) : "{}"),
      };
    }
    if (url.includes(`/projects/${P1}/user-properties/`)) {
      const status = options.propsStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () =>
          status === 200
            ? JSON.stringify(propsPayload(options.navigation ?? { default_tab: "cycles", hide_in_more_menu: [] }))
            : "{}",
      };
    }
    if (url.endsWith("/projects/")) {
      const status = options.projectsStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => (status === 200 ? JSON.stringify(options.projects ?? [projectRow()]) : "{}"),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <DefaultTabRedirect workspaceSlug="acme" projectId={P1} />
    </QueryClientProvider>
  );
  return queryClient;
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("DefaultTabRedirect", () => {
  it("rewrites the bare address to the stored default tab", async () => {
    setup();
    await waitFor(() =>
      expect(mockNavigate).toHaveBeenCalledWith({ to: `/acme/projects/${P1}/cycles`, replace: true })
    );
  });

  it("falls back to tracking for unknown stored keys", async () => {
    setup({ navigation: { default_tab: "nope", hide_in_more_menu: [] } });
    await waitFor(() =>
      expect(mockNavigate).toHaveBeenCalledWith({ to: `/acme/projects/${P1}/issues`, replace: true })
    );
  });

  it("falls back to tracking when the stored tab is gated away", async () => {
    setup({ projects: [projectRow({ cycle_view: false })] });
    await waitFor(() =>
      expect(mockNavigate).toHaveBeenCalledWith({ to: `/acme/projects/${P1}/issues`, replace: true })
    );
  });

  it("falls back to tracking without project data", async () => {
    setup({ projects: [] });
    await waitFor(() =>
      expect(mockNavigate).toHaveBeenCalledWith({ to: `/acme/projects/${P1}/issues`, replace: true })
    );
  });

  it("falls back to tracking when the prefs read fails", async () => {
    setup({ propsStatus: 500 });
    await waitFor(() =>
      expect(mockNavigate).toHaveBeenCalledWith({ to: `/acme/projects/${P1}/issues`, replace: true })
    );
  });

  it("falls back to tracking when the projects read fails", async () => {
    setup({ projectsStatus: 500 });
    await waitFor(() =>
      expect(mockNavigate).toHaveBeenCalledWith({ to: `/acme/projects/${P1}/issues`, replace: true })
    );
  });

  it("reads guest-level tabs when the standing read fails", async () => {
    setup({ rolesStatus: 500 });
    // Cycles is member-plus, so the guest fallback lands on tracking.
    await waitFor(() =>
      expect(mockNavigate).toHaveBeenCalledWith({ to: `/acme/projects/${P1}/issues`, replace: true })
    );
  });

  it("reads guest-level tabs without a project role", async () => {
    setup({ roles: {} });
    await waitFor(() =>
      expect(mockNavigate).toHaveBeenCalledWith({ to: `/acme/projects/${P1}/issues`, replace: true })
    );
  });

  it("waits for every read before navigating", async () => {
    const queryClient = setup({ hang: true });
    await waitFor(() => expect(queryClient.getQueryCache().getAll()).toHaveLength(3));
    expect(mockNavigate).not.toHaveBeenCalled();
  });
});
