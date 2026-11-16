// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived work-items tab drops a peek carried over from another
// project when the project changes (parity/specs/archives.md §2.2/§2.5).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { ProjectDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const navigate = vi.hoisted(() => vi.fn());
const params = vi.hoisted(() => ({ current: { ws: "acme", projectId: "" } }));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => navigate,
}));
vi.mock("../../../../../../../features/archives/components/ArchivesShell.js", () => ({
  ArchivesShell: ({ children }: { children?: React.ReactNode }) => <div>{children}</div>,
}));
vi.mock("../../../../../../../features/issues/contexts/ArchivedIssuesScreen.js", () => ({
  ArchivedIssuesScreen: () => <div data-testid="archived-screen" />,
}));

import { createAppClient, resetAppClient } from "../../../../../../../core/api/client.js";
import { authKeys } from "../../../../../../../features/auth/index.js";
import { Route } from "./index.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER = "323e4567-e89b-12d3-a456-426614174002";
const UUID = "123e4567-e89b-12d3-a456-426614174000";

function detailRow(): ProjectDetail {
  return {
    id: PROJECT,
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
  };
}

beforeEach(() => {
  navigate.mockClear();
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function renderTab(search: Record<string, unknown>): { rerender: () => void } {
  vi.spyOn(Route, "useParams").mockImplementation(() => params.current as never);
  vi.spyOn(Route, "useSearch").mockReturnValue(search as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(authKeys.project("acme", PROJECT), detailRow());
  // The route component stays private: the router's code splitter leaves a
  // route component in the eager tree when the module exports it, which
  // pulls the archived list into the initial JS (NEWFRONT-910). Render the
  // definition's component the way the router does.
  const Component = Route.options.component;
  if (!Component) throw new Error("archived issues tab has no component");
  const tree = () => (
    <QueryClientProvider client={queryClient}>
      <Component />
    </QueryClientProvider>
  );
  const { rerender } = render(tree());
  return { rerender: () => rerender(tree()) };
}

describe("archived work-items tab project switch", () => {
  it("clears a carried-over peek", async () => {
    params.current = { ws: "acme", projectId: PROJECT };
    const { rerender } = renderTab({ peekIssueId: UUID });
    await screen.findByTestId("archived-screen");
    expect(navigate).not.toHaveBeenCalled();
    params.current = { ws: "acme", projectId: OTHER };
    rerender();
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ to: ".", search: {}, replace: true }));
  });

  it("leaves the address alone when there is no peek to drop", async () => {
    params.current = { ws: "acme", projectId: PROJECT };
    const { rerender } = renderTab({ layout: "compact" });
    await screen.findByTestId("archived-screen");
    params.current = { ws: "acme", projectId: OTHER };
    rerender();
    await waitFor(() => expect(screen.getByTestId("archived-screen")).toBeInTheDocument());
    expect(navigate).not.toHaveBeenCalled();
  });
});
