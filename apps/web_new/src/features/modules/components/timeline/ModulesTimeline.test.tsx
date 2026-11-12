// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules timeline layout (parity/specs/modules.md §6, MOD-021): sidebar
// rows with a status glyph and detail links, status-colored bars with
// name-plus-dates tooltips, bar move/resize through the module PATCH, row
// reorder under manual placement with edit standing, a manual-sort warning
// for refused attempts, and silent no-ops for guests.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
const mocks = vi.hoisted(() => ({ updateModule: vi.fn() }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerHarness.navigate,
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, updateModule: mocks.updateModule };
});

vi.mock("../../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifyInfo: vi.fn() }));

import { notifyInfo } from "../../../../shared/shell/toaster.js";
import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { authKeys } from "../../../auth/queries.js";
import { DEFAULT_MODULE_SORT, type ModuleSort } from "../../lib/moduleListControls.js";
import { jsonResponse, makeModuleRow, MODULES_PROJECT_ID } from "../../testing.js";
import { ModuleListProvider } from "../moduleListContext.js";
import { MODULE_TIMELINE_REORDER_WARNING_BODY, MODULE_TIMELINE_REORDER_WARNING_TITLE } from "./moduleTimelineModel.js";
import { ModulesTimeline } from "./ModulesTimeline.js";

const PROJECT = MODULES_PROJECT_ID;
const DATED = makeModuleRow({
  id: "m-1",
  name: "Planned Bar",
  status: "planned",
  start_date: "2026-09-01",
  target_date: "2026-09-10",
  sort_order: 0,
});
const DATELESS = makeModuleRow({ id: "m-2", name: "Dateless", status: "completed", sort_order: 0 });
const SECOND = makeModuleRow({
  id: "m-3",
  name: "Second Bar",
  status: "in-progress",
  start_date: "2026-09-05",
  target_date: "2026-09-12",
  sort_order: 0,
});

interface SetupArgs {
  modules?: ReturnType<typeof makeModuleRow>[];
  role?: number | null;
  sort?: ModuleSort | undefined;
  contextSort?: ModuleSort | null;
}

function setup({ modules = [DATED, DATELESS, SECOND], role = 15, sort, contextSort = null }: SetupArgs = {}) {
  createAppClient("https://test.invalid", async (url) => jsonResponse(url, 404, {}));
  mocks.updateModule.mockImplementation(async () => ({}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (role !== null) {
    queryClient.setQueryData(authKeys.projectRoles("acme"), { [PROJECT]: role });
  }
  const timeline = <ModulesTimeline workspaceSlug="acme" projectId={PROJECT} modules={modules} sort={sort} />;
  render(
    <QueryClientProvider client={queryClient}>
      {contextSort === null ? (
        timeline
      ) : (
        <ModuleListProvider
          value={{
            query: "",
            setQuery: () => undefined,
            sort: contextSort,
            setSort: () => undefined,
            filters: {
              favorites: false,
              status: [],
              lead: [],
              members: [],
              "start-date": [],
              "due-date": [],
            },
            setFilters: () => undefined,
            filtersActive: false,
          }}
        >
          {timeline}
        </ModuleListProvider>
      )}
    </QueryClientProvider>
  );
  return { queryClient };
}

function sidebarRows(): HTMLElement[] {
  return screen.getAllByTestId("gantt-sidebar-row");
}

function barOf(id: string): HTMLElement {
  const bar = document.querySelector<HTMLElement>(`#gantt-block-${id}`);
  if (bar === null) throw new Error(`no bar for ${id}`);
  return bar;
}

beforeEach(() => {
  routerHarness.navigate.mockClear();
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.useRealTimers();
  resetAppClient();
});

describe("ModulesTimeline rows and bars", () => {
  it("renders sidebar rows with a status glyph and detail links", () => {
    setup();
    const rows = sidebarRows();
    expect(rows).toHaveLength(3);
    const links = rows.map((row) => within(row).getByRole("link"));
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      `/acme/projects/${PROJECT}/modules/m-1`,
      `/acme/projects/${PROJECT}/modules/m-2`,
      `/acme/projects/${PROJECT}/modules/m-3`,
    ]);
    expect(links.map((link) => link.getAttribute("aria-label"))).toEqual(["Planned Bar", "Dateless", "Second Bar"]);
    const glyphs = screen.getAllByTestId("modules-timeline-status-glyph");
    expect(glyphs).toHaveLength(3);
    expect(glyphs[0]?.querySelector("circle")).toHaveAttribute("fill", "#3f76ff");
    expect(glyphs[1]?.querySelector("circle")).toHaveAttribute("fill", "#16a34a");
    expect(screen.queryByText("Duration")).not.toBeInTheDocument();
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("3 modules");
  });

  it("counts a single module with the singular noun", () => {
    setup({ modules: [DATED] });
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("1 module");
  });

  it("plots bars for dated modules and keeps dateless lanes bar-free", () => {
    setup();
    expect(barOf("m-1")).toBeInTheDocument();
    expect(barOf("m-3")).toBeInTheDocument();
    expect(document.querySelector("#gantt-block-m-2")).toBeNull();
    expect(within(barOf("m-1")).getByTestId("gantt-bar-body").getAttribute("style")).toContain("background-color");
    expect(screen.getAllByText("No dates")).toHaveLength(1);
  });

  it("renders nothing without modules", () => {
    setup({ modules: [] });
    expect(screen.queryByTestId("modules-timeline")).not.toBeInTheDocument();
  });

  it("opens the module detail on a plain row click", () => {
    setup();
    fireEvent.click(within(sidebarRows()[0]!).getByRole("link"));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: `/acme/projects/${PROJECT}/modules/m-1` });
  });
});

describe("ModulesTimeline tooltips", () => {
  it("carries the name and date summary on every bar", () => {
    setup();
    expect(barOf("m-1").getAttribute("title")).toBe("Planned Bar: Sep 01, 2026 to Sep 10, 2026");
    expect(barOf("m-3").getAttribute("title")).toBe("Second Bar: Sep 05, 2026 to Sep 12, 2026");
  });
});

describe("ModulesTimeline bar gestures", () => {
  it("moves a bar by whole days and persists both dates", async () => {
    setup();
    const bar = barOf("m-1");
    fireEvent.mouseDown(bar, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 180 });
    fireEvent.mouseUp(document);
    await waitFor(() =>
      expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, "m-1", {
        start_date: "2026-09-03",
        target_date: "2026-09-12",
      })
    );
  });

  it("resizes from the right handle and persists only the end date", async () => {
    setup();
    const handles = screen.getAllByTestId("gantt-resize-right");
    fireEvent.mouseDown(handles[0]!, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 180 });
    fireEvent.mouseUp(document);
    await waitFor(() =>
      expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, "m-1", {
        start_date: "2026-09-01",
        target_date: "2026-09-12",
      })
    );
  });

  it("renders no handles and persists nothing for a guest drag", () => {
    setup({ role: 5 });
    expect(screen.queryByTestId("gantt-resize-left")).not.toBeInTheDocument();
    expect(screen.queryByTestId("gantt-resize-right")).not.toBeInTheDocument();
    const bar = barOf("m-1");
    fireEvent.mouseDown(bar, { button: 0, clientX: 100 });
    fireEvent.mouseMove(document, { clientX: 220 });
    fireEvent.mouseUp(document);
    expect(mocks.updateModule).not.toHaveBeenCalled();
    expect(notifyInfo).not.toHaveBeenCalled();
  });

  it("treats a missing role as read-only", () => {
    setup({ role: null });
    expect(screen.queryByTestId("gantt-resize-left")).not.toBeInTheDocument();
  });

  it("offers no add-block on undated lanes", () => {
    setup();
    const lane = screen.getByText("No dates").closest("div")!;
    fireEvent.mouseMove(lane, { clientX: 400, clientY: 10 });
    expect(screen.queryByTestId("gantt-add-block")).not.toBeInTheDocument();
  });
});

describe("ModulesTimeline reorder", () => {
  const MANUAL: ModuleSort = { key: "manual", direction: "ascending" };

  function stubRowTops(rows: HTMLElement[]): void {
    rows.forEach((row, index) => {
      row.getBoundingClientRect = () => ({ top: index * 44, height: 44, bottom: (index + 1) * 44 }) as DOMRect;
    });
  }

  it("reorders rows under manual placement and persists normalized ranks", async () => {
    setup({ sort: MANUAL });
    const rows = sidebarRows();
    stubRowTops(rows);
    fireEvent.mouseDown(rows[2]!, { button: 0, clientY: 110 });
    fireEvent.mouseMove(rows[0]!, { clientY: 4 });
    fireEvent.mouseUp(rows[0]!);
    await waitFor(() => expect(mocks.updateModule).toHaveBeenCalledTimes(3));
    expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, "m-3", {
      sort_order: 1000,
    });
    expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, "m-1", {
      sort_order: 2000,
    });
    expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, "m-2", {
      sort_order: 3000,
    });
  });

  it("warns and reorders nothing outside manual placement", () => {
    setup({ sort: DEFAULT_MODULE_SORT });
    const rows = sidebarRows();
    stubRowTops(rows);
    fireEvent.mouseDown(rows[2]!, { button: 0, clientY: 110 });
    fireEvent.mouseMove(rows[0]!, { clientY: 4 });
    fireEvent.mouseUp(rows[0]!);
    expect(notifyInfo).toHaveBeenCalledWith(
      MODULE_TIMELINE_REORDER_WARNING_TITLE,
      MODULE_TIMELINE_REORDER_WARNING_BODY
    );
    expect(mocks.updateModule).not.toHaveBeenCalled();
    // The attempt's own click never opens the row.
    fireEvent.click(within(rows[0]!).getByRole("link"));
    expect(routerHarness.navigate).not.toHaveBeenCalled();
  });

  it("stays silent for a guest attempt under manual placement", () => {
    setup({ role: 5, sort: MANUAL });
    const rows = sidebarRows();
    stubRowTops(rows);
    fireEvent.mouseDown(rows[2]!, { button: 0, clientY: 110 });
    fireEvent.mouseMove(rows[0]!, { clientY: 4 });
    fireEvent.mouseUp(rows[0]!);
    expect(notifyInfo).not.toHaveBeenCalled();
    expect(mocks.updateModule).not.toHaveBeenCalled();
  });

  it("writes nothing for a drop back onto the dragged row", () => {
    setup({ sort: MANUAL });
    const rows = sidebarRows();
    stubRowTops(rows);
    fireEvent.mouseDown(rows[0]!, { button: 0, clientY: 22 });
    fireEvent.mouseMove(rows[0]!, { clientY: 40 });
    fireEvent.mouseUp(rows[0]!);
    expect(mocks.updateModule).not.toHaveBeenCalled();
    expect(notifyInfo).not.toHaveBeenCalled();
  });

  it("reads the sort from the list context", async () => {
    setup({ sort: undefined, contextSort: MANUAL });
    const rows = sidebarRows();
    stubRowTops(rows);
    fireEvent.mouseDown(rows[2]!, { button: 0, clientX: 10, clientY: 110 });
    fireEvent.mouseMove(rows[0]!, { clientY: 4 });
    fireEvent.mouseUp(rows[0]!);
    await waitFor(() => expect(mocks.updateModule).toHaveBeenCalled());
  });

  it("falls back to name order outside a list and warns on an attempt", () => {
    setup({ sort: undefined, contextSort: null });
    const rows = sidebarRows();
    stubRowTops(rows);
    fireEvent.mouseDown(rows[0]!, { button: 0, clientY: 22 });
    fireEvent.mouseMove(rows[1]!, { clientY: 60 });
    fireEvent.mouseUp(rows[1]!);
    expect(notifyInfo).toHaveBeenCalledWith(
      MODULE_TIMELINE_REORDER_WARNING_TITLE,
      MODULE_TIMELINE_REORDER_WARNING_BODY
    );
    expect(mocks.updateModule).not.toHaveBeenCalled();
  });
});
