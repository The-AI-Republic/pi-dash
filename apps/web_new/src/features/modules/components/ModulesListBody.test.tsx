// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One module set, three renditions (MOD-001/011): rows for the list, cards
// for the board, the timeline chart for the timeline, each in the default
// name order and each absent from the other layouts. Slice B narrows that
// set through the header's search, filters and ordering
// (MOD-004/005/006/010) — from the list context or explicit props — and
// swaps the rows for the no-match container when modules exist but nothing
// matches, without touching the underlying set.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
  useNavigate: () => vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { emptyModuleFilters, toggleModuleFilterValue } from "../lib/moduleListControls.js";
import { jsonResponse, MODULES_PROJECT_ID, makeModuleRow } from "../testing.js";
import { ModuleListProvider, type ModuleListContextValue } from "./moduleListContext.js";
import { ModulesListBody } from "./ModulesListBody.js";

const ROWS = [
  makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174002", name: "Module 10" }),
  makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174003", name: "Alpha" }),
  makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174004", name: "Module 2" }),
];

function renderBody(layout: "list" | "board" | "timeline", modules = ROWS): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ModulesListBody workspaceSlug="acme" projectId={MODULES_PROJECT_ID} layout={layout} modules={modules} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  createAppClient("https://test.invalid", async (url) => jsonResponse(url, 404, {}));
});

afterEach(() => {
  resetAppClient();
});

function context(overrides: Partial<ModuleListContextValue> = {}): ModuleListContextValue {
  return {
    query: "",
    setQuery: vi.fn(),
    sort: { key: "name", direction: "ascending" },
    setSort: vi.fn(),
    filters: emptyModuleFilters(),
    setFilters: vi.fn(),
    filtersActive: false,
    ...overrides,
  };
}

function renderWithContext(
  value: ModuleListContextValue,
  modules = ROWS,
  props: Partial<React.ComponentProps<typeof ModulesListBody>> = {}
): ReturnType<typeof render> {
  return render(
    <ModuleListProvider value={value}>
      <ModulesListBody workspaceSlug="acme" projectId={MODULES_PROJECT_ID} layout="list" modules={modules} {...props} />
    </ModuleListProvider>
  );
}

function names(): Array<string | null> {
  return screen.getAllByTestId("modules-row-name").map((node) => node.textContent);
}

describe("ModulesListBody", () => {
  it("renders one linked row per module in natural name order", () => {
    renderBody("list");
    const root = screen.getByTestId("modules-list");
    expect(root).toHaveAttribute("data-layout", "list");
    expect(screen.getAllByTestId("modules-row-name").map((node) => node.textContent)).toEqual([
      "Alpha",
      "Module 2",
      "Module 10",
    ]);
    expect(screen.queryAllByTestId("modules-board-card")).toHaveLength(0);
    expect(screen.queryAllByTestId("modules-timeline")).toHaveLength(0);
  });

  it("renders cards for the board layout", () => {
    renderBody("board");
    expect(screen.getByTestId("modules-list")).toHaveAttribute("data-layout", "board");
    expect(screen.getAllByTestId("modules-board-card-name").map((node) => node.textContent)).toEqual([
      "Alpha",
      "Module 2",
      "Module 10",
    ]);
    expect(screen.queryAllByTestId("modules-row")).toHaveLength(0);
  });

  it("renders the timeline chart for the timeline layout", () => {
    renderBody("timeline");
    expect(screen.getByTestId("modules-list")).toHaveAttribute("data-layout", "timeline");
    expect(screen.getByTestId("modules-timeline")).toBeInTheDocument();
    expect(
      screen.getAllByTestId("gantt-sidebar-row").map((row) => within(row).getByRole("link").getAttribute("aria-label"))
    ).toEqual(["Alpha", "Module 2", "Module 10"]);
    expect(screen.queryAllByTestId("modules-row")).toHaveLength(0);
  });

  it("renders an empty list element for no modules", () => {
    renderBody("list", []);
    expect(screen.getByRole("list", { name: "Modules" })).toBeEmptyDOMElement();
    expect(screen.queryAllByTestId("modules-row")).toHaveLength(0);
  });

  it("narrows by the name search through the list context", () => {
    renderWithContext(context({ query: "module" }));
    expect(names()).toEqual(["Module 2", "Module 10"]);
  });

  it("orders the narrowed set through the context sort", () => {
    renderWithContext(context({ sort: { key: "name", direction: "descending" } }));
    expect(names()).toEqual(["Module 10", "Module 2", "Alpha"]);
  });

  it("takes explicit props over the context", () => {
    renderWithContext(context({ query: "module" }), ROWS, {
      query: "alpha",
      sort: { key: "name", direction: "ascending" },
      filters: emptyModuleFilters(),
    });
    expect(names()).toEqual(["Alpha"]);
  });

  it("shows the no-match container when a search hides every module", () => {
    renderWithContext(context({ query: "zzz" }));
    expect(screen.getByTestId("modules-no-match")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-list")).not.toBeInTheDocument();
  });

  it("shows the no-match container when a filter hides every module", () => {
    renderWithContext(context({ filters: toggleModuleFilterValue(emptyModuleFilters(), "status", "completed") }));
    expect(screen.getByTestId("modules-no-match")).toBeInTheDocument();
  });

  it("restores every module when the search and filters clear", () => {
    const filters = toggleModuleFilterValue(emptyModuleFilters(), "status", "completed");
    const { rerender } = renderWithContext(context({ query: "zzz", filters }));
    expect(screen.getByTestId("modules-no-match")).toBeInTheDocument();
    rerender(
      <ModuleListProvider value={context()}>
        <ModulesListBody workspaceSlug="acme" projectId={MODULES_PROJECT_ID} layout="list" modules={ROWS} />
      </ModuleListProvider>
    );
    expect(names()).toEqual(["Alpha", "Module 2", "Module 10"]);
  });

  it("keeps an empty project out of the no-match container", () => {
    renderWithContext(
      context({ query: "zzz", filters: toggleModuleFilterValue(emptyModuleFilters(), "favorites") }),
      []
    );
    expect(screen.queryByTestId("modules-no-match")).not.toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Modules" })).toBeEmptyDOMElement();
  });
});
