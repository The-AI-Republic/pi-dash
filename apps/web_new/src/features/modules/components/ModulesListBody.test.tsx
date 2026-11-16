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
import { fireEvent, render, screen, within } from "@testing-library/react";
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
import { jsonResponse, makeDirectoryEntry, MODULES_LEAD_ID, MODULES_PROJECT_ID, makeModuleRow } from "../testing.js";
import { ModuleListProvider, type ModuleListContextValue } from "./moduleListContext.js";
import { ModulesListBody, renderModule } from "./ModulesListBody.js";

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
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ModuleListProvider value={value}>
        <ModulesListBody
          workspaceSlug="acme"
          projectId={MODULES_PROJECT_ID}
          layout="list"
          modules={modules}
          {...props}
        />
      </ModuleListProvider>
    </QueryClientProvider>
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
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    rerender(
      <QueryClientProvider client={queryClient}>
        <ModuleListProvider value={context()}>
          <ModulesListBody workspaceSlug="acme" projectId={MODULES_PROJECT_ID} layout="list" modules={ROWS} />
        </ModuleListProvider>
      </QueryClientProvider>
    );
    expect(names()).toEqual(["Alpha", "Module 2", "Module 10"]);
  });

  it("narrows the board grid while the peek is open", () => {
    renderWithContext(context(), ROWS, {
      layout: "board",
      peekModuleId: "323e4567-e89b-12d3-a456-426614174002",
    });
    const root = screen.getByTestId("modules-list");
    expect(root).toHaveAttribute("data-peek-open", "true");
    expect(root.className).toContain("xl:grid-cols-2");
  });

  it("keeps the wide board grid while the peek is closed", () => {
    renderWithContext(context(), ROWS, { layout: "board" });
    const root = screen.getByTestId("modules-list");
    expect(root).toHaveAttribute("data-peek-open", "false");
    expect(root.className).toContain("xl:grid-cols-3");
  });

  it("hands every row the peek writer and the row menu slot", () => {
    const onOpenPeek = vi.fn();
    renderWithContext(context(), ROWS, {
      onOpenPeek,
      alwaysShowActions: true,
      rowMenu: (module) => <button type="button">{`Menu ${module.name}`}</button>,
    });
    // Rows are in natural name order, so the first one is Alpha.
    fireEvent.click(screen.getAllByTestId("modules-row-peek")[0]!);
    expect(onOpenPeek).toHaveBeenCalledWith("323e4567-e89b-12d3-a456-426614174003");
    expect(screen.getAllByTestId("modules-row-menu")).toHaveLength(3);
    expect(screen.getAllByTestId("modules-row-peek")[0]?.className).not.toContain("opacity-0");
  });

  it("mounts the desktop quick menu beside the narrow-screen one", () => {
    renderWithContext(context(), ROWS, {
      rowMenu: (module) => <button type="button">{`Menu ${module.name}`}</button>,
    });
    const desktop = screen.getAllByTestId("modules-row-menu-desktop");
    expect(desktop).toHaveLength(3);
    expect(desktop[0]?.className).toContain("hidden");
    expect(desktop[0]?.className).toContain("md:flex");
    // The desktop trigger reveals on hover like the peek control.
    expect(desktop[0]?.className).toContain("group-hover:opacity-100");
    expect(within(desktop[0]!).getByRole("button", { name: "Menu Alpha" })).toBeInTheDocument();
    // The narrow-screen mount stays beside it; the breakpoint shows one.
    expect(screen.getAllByTestId("modules-row-menu")).toHaveLength(3);
  });

  it("keeps the desktop quick menu visible when the host asks", () => {
    renderWithContext(context(), ROWS, {
      alwaysShowActions: true,
      rowMenu: (module) => <button type="button">{`Menu ${module.name}`}</button>,
    });
    expect(screen.getAllByTestId("modules-row-menu-desktop")[0]?.className).not.toContain("opacity-0");
  });

  it("mounts no quick menu without the slot", () => {
    renderWithContext(context(), ROWS);
    expect(screen.queryAllByTestId("modules-row-menu-desktop")).toHaveLength(0);
    expect(screen.queryAllByTestId("modules-row-menu")).toHaveLength(0);
  });

  it("hands every board card the quick menu slot", () => {
    renderWithContext(context(), ROWS, {
      layout: "board",
      rowMenu: (module) => <button type="button">{`Menu ${module.name}`}</button>,
    });
    const slots = screen.getAllByTestId("modules-card-menu");
    expect(slots).toHaveLength(3);
    expect(within(slots[0]!).getByRole("button", { name: "Menu Alpha" })).toBeInTheDocument();
  });

  it("leaves board cards menu-free without the slot", () => {
    renderWithContext(context(), ROWS, { layout: "board" });
    for (const slot of screen.getAllByTestId("modules-card-menu")) {
      expect(slot).toBeEmptyDOMElement();
    }
  });

  it("renders archived rows in archived mode and leaves cards alone", () => {
    const onOpenPeek = vi.fn();
    renderWithContext(context(), ROWS, { onOpenPeek, archived: true });
    expect(screen.getAllByTestId("modules-row")[0]).toHaveAttribute("data-module-archived", "true");
    fireEvent.click(screen.getAllByTestId("modules-row-progress")[0]!);
    expect(onOpenPeek).toHaveBeenCalledWith("323e4567-e89b-12d3-a456-426614174003");
  });

  it("resolves a board card's lead against the workspace directory", () => {
    renderWithContext(context(), [makeModuleRow({ lead_id: MODULES_LEAD_ID, name: "Led" })], {
      layout: "board",
      members: [makeDirectoryEntry()],
    });
    const lead = screen.getByTestId("modules-card-lead");
    expect(lead).toHaveAttribute("data-lead-kind", "avatar");
    expect(within(lead).getByRole("img", { name: "Ada Lovelace" })).toBeInTheDocument();
  });

  it("shows the no-lead mark when the directory has no entry for the module's lead", () => {
    renderWithContext(context(), [makeModuleRow({ lead_id: MODULES_LEAD_ID, name: "Orphan lead" })], {
      layout: "board",
    });
    expect(screen.getByTestId("modules-card-lead")).toHaveAttribute("data-lead-kind", "none");
  });

  it("keeps an empty project out of the no-match container", () => {
    renderWithContext(
      context({ query: "zzz", filters: toggleModuleFilterValue(emptyModuleFilters(), "favorites") }),
      []
    );
    expect(screen.queryByTestId("modules-no-match")).not.toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Modules" })).toBeEmptyDOMElement();
  });

  it("fills the row accessory slots for an editor", () => {
    renderWithContext(context(), ROWS, { memberRole: 15 });
    const first = screen.getAllByTestId("modules-row")[0]!;
    expect(within(first).getByTestId("modules-row-accessories")).toBeInTheDocument();
    expect(within(first).getByTestId("modules-row-dates-trigger")).toBeEnabled();
    expect(within(first).getByTestId("modules-row-status")).toBeEnabled();
    expect(within(first).getByTestId("modules-row-lead")).toBeInTheDocument();
    expect(within(first).getByTestId("modules-row-star")).toBeInTheDocument();
  });

  it("locks the row accessories for a guest", () => {
    renderWithContext(context(), ROWS, { memberRole: 5 });
    const first = screen.getAllByTestId("modules-row")[0]!;
    expect(within(first).getByTestId("modules-row-dates-trigger")).toBeDisabled();
    expect(within(first).getByTestId("modules-row-status")).toBeDisabled();
    expect(within(first).getByTestId("modules-row-lead")).toBeInTheDocument();
    expect(within(first).queryByTestId("modules-row-star")).not.toBeInTheDocument();
  });

  it("locks the row accessories on archived rows", () => {
    renderWithContext(context(), ROWS, { memberRole: 15, archived: true });
    const first = screen.getAllByTestId("modules-row")[0]!;
    expect(within(first).getByTestId("modules-row-dates-trigger")).toBeDisabled();
    expect(within(first).getByTestId("modules-row-status")).toBeDisabled();
    expect(within(first).queryByTestId("modules-row-star")).not.toBeInTheDocument();
  });

  it("fills the card accessory slots for an editor", () => {
    renderWithContext(context(), ROWS, { layout: "board", memberRole: 15 });
    const first = screen.getAllByTestId("modules-board-card")[0]!;
    expect(within(first).getByTestId("modules-card-status-pill")).toBeEnabled();
    expect(within(first).getByTestId("modules-card-dates-control-trigger")).toBeEnabled();
    expect(within(first).getByTestId("modules-card-star")).toBeInTheDocument();
  });

  it("locks the card accessories for a guest", () => {
    renderWithContext(context(), ROWS, { layout: "board", memberRole: 5 });
    const first = screen.getAllByTestId("modules-board-card")[0]!;
    expect(within(first).getByTestId("modules-card-status-pill")).toBeDisabled();
    expect(within(first).getByTestId("modules-card-dates-control-trigger")).toBeDisabled();
    expect(within(first).queryByTestId("modules-card-star")).not.toBeInTheDocument();
  });

  it("renders a read-only row when no options arrive", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        {renderModule("list", "acme", MODULES_PROJECT_ID, ROWS[0]!)}
      </QueryClientProvider>
    );
    expect(screen.getByTestId("modules-row-dates-trigger")).toBeDisabled();
    expect(screen.getByTestId("modules-row-status")).toBeDisabled();
    expect(screen.queryByTestId("modules-row-star")).not.toBeInTheDocument();
  });

  it("renders a read-only card when no options arrive", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        {renderModule("board", "acme", MODULES_PROJECT_ID, ROWS[0]!)}
      </QueryClientProvider>
    );
    expect(screen.getByTestId("modules-card-status-pill")).toBeDisabled();
    expect(screen.getByTestId("modules-card-dates-control-trigger")).toBeDisabled();
    expect(screen.queryByTestId("modules-card-star")).not.toBeInTheDocument();
  });
});
