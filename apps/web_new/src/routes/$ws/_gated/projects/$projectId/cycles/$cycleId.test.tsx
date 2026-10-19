// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  navigate: vi.fn(),
  cyclesKey: ["picker", "cycles"] as const,
  page: vi.fn(),
  prefs: vi.fn(),
}));

vi.mock("../../../../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => mocks.navigate,
}));
vi.mock("../../../../../../features/issues/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../../../features/issues/index.js")>()),
  prefsQueryOptions: mocks.prefs,
}));
vi.mock("../../../../../../features/issues/list/pages.js", () => ({ listPageQueryOptions: mocks.page }));
vi.mock("../../../../../../features/issues/list/EntityListScreen.js", () => ({
  EntityListScreen: (props: Record<string, unknown>) => (
    <div
      data-testid="entity-screen"
      data-title={props.title as string}
      data-cycle={String(props.cycleId ?? "")}
      data-completed={String(props.completedCycle ?? false)}
      data-peek={String(props.peekKey ?? "")}
      data-compact={String(props.compact ?? false)}
    >
      <button type="button" onClick={() => (props.onPeekChange as (key: string | null) => void)("WEB-1")}>
        open
      </button>
      <button type="button" onClick={() => (props.onPeekChange as (key: string | null) => void)(null)}>
        close
      </button>
    </div>
  ),
}));
vi.mock("../../../../../../shared/pickers/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../../../shared/pickers/index.js")>()),
  pickerCyclesQueryOptions: () => ({ queryKey: mocks.cyclesKey, queryFn: async () => [] }),
}));

import { Route } from "./$cycleId.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";

function cycle(status = "current") {
  return { id: CYCLE, name: "Sprint 1", status };
}

function renderRoute(search: Record<string, unknown> = {}, rows: Array<ReturnType<typeof cycle>> = [cycle()]) {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT, cycleId: CYCLE } as never);
  vi.spyOn(Route, "useSearch").mockReturnValue(search as never);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
  });
  queryClient.setQueryData(mocks.cyclesKey, rows);
  return render(
    <QueryClientProvider client={queryClient}>
      {React.createElement(Route.options.component as React.ComponentType) as never}
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.page.mockReturnValue({
    queryKey: ["page"],
    queryFn: async () => ({
      total_count: 0,
      results: [],
      count: 0,
      grouped_by: null,
      sub_grouped_by: null,
      next_cursor: "",
      prev_cursor: "",
      next_page_results: false,
      prev_page_results: false,
      total_results: 0,
    }),
  });
  mocks.prefs.mockReturnValue({
    queryKey: ["prefs"],
    queryFn: async () => ({ filters: {}, display_filters: {}, display_properties: {}, rich_filters: {} }),
  });
});

describe("cycle issues route", () => {
  it("mounts the shared list for the cycle with its name", async () => {
    renderRoute();
    const node = await screen.findByTestId("entity-screen");
    await waitFor(() => expect(node).toHaveAttribute("data-title", "Sprint 1"));
    expect(await screen.findByTestId("entity-screen")).toHaveAttribute("data-cycle", CYCLE);
    expect(await screen.findByTestId("entity-screen")).toHaveAttribute("data-completed", "false");
    expect(await screen.findByTestId("entity-screen")).toHaveAttribute("data-peek", "");
    expect(await screen.findByTestId("entity-screen")).toHaveAttribute("data-compact", "false");
  });

  it("falls back to the generic title while cycles load", async () => {
    renderRoute({}, []);
    expect(await screen.findByTestId("entity-screen")).toHaveAttribute("data-title", "Cycle");
  });

  it("marks a completed cycle read-only", async () => {
    renderRoute({}, [cycle("completed")]);
    expect(await screen.findByTestId("entity-screen")).toHaveAttribute("data-completed", "true");
  });

  it("passes peek and compact from the search state", async () => {
    renderRoute({ peekIssueId: "WEB-7", layout: "compact" });
    const screenNode = await screen.findByTestId("entity-screen");
    expect(screenNode).toHaveAttribute("data-peek", "WEB-7");
    expect(screenNode).toHaveAttribute("data-compact", "true");
  });

  it("writes and clears the peek param with replace", async () => {
    renderRoute({ peekIssueId: "WEB-7", layout: "compact" });
    fireEvent.click(await screen.findByRole("button", { name: "open" }));
    expect(mocks.navigate).toHaveBeenLastCalledWith({
      to: ".",
      search: { peekIssueId: "WEB-1", layout: "compact" },
      replace: true,
    });
    fireEvent.click(await screen.findByRole("button", { name: "close" }));
    expect(mocks.navigate).toHaveBeenLastCalledWith({
      to: ".",
      search: { layout: "compact" },
      replace: true,
    });
  });

  it("prefetches the cycle list page and preferences with the cycle entity", async () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("missing beforeLoad");
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    await beforeLoad({
      context: { queryClient, apiClient: { stub: true } },
      params: { ws: "acme", projectId: PROJECT, cycleId: CYCLE },
    } as never);
    expect(mocks.page).toHaveBeenCalledWith(expect.anything(), "acme", {
      entity: { scope: "cycle", projectId: PROJECT, cycleId: CYCLE },
      groupBy: null,
      orderBy: "sort_order",
    });
    expect(mocks.prefs).toHaveBeenCalledWith(expect.anything(), "acme", {
      scope: "cycle",
      projectId: PROJECT,
      cycleId: CYCLE,
    });
  });

  it("renders the list skeleton while pending", () => {
    const Pending = Route.options.pendingComponent;
    if (!Pending) throw new Error("missing pendingComponent");
    render(React.createElement(Pending));
    expect(screen.getByTestId("issues-skeleton-list")).toBeInTheDocument();
  });
});
