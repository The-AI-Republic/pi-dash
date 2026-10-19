// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline layout entry (parity/specs/issues.md §6, ISS-044–049). The
// layout orders the fetched rows by the display preference, tints bars
// from the project's states, and docks the shared peek for a clicked row
// while the chart keeps its own scroll state.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sessionKeys } from "../../../core/session/queries.js";
import { authKeys } from "../../auth/queries.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { issueKeys } from "../api/keys.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import { GanttLayout, barColorsFor, orderedRows } from "./GanttLayout.js";
import { buildRows } from "./rows.js";
import { ganttStatesKey } from "./useGanttData.js";

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => vi.fn(),
}));

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const STATE = "323e4567-e89b-12d3-a456-426614174002";

function issue(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "First",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 1,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

const ISSUES = [
  issue(),
  issue({ id: "223e4567-e89b-12d3-a456-426614174010", name: "Second", sequence_id: 2, sort_order: 2, state_id: STATE }),
  issue({ id: "223e4567-e89b-12d3-a456-426614174011", name: "Third", sequence_id: 3, sort_order: 3 }),
];

function prefs(overrides: Partial<DisplayPrefs> = {}): DisplayPrefs {
  return {
    layout: "gantt_chart",
    groupBy: null,
    subGroupBy: null,
    orderBy: "sort_order",
    showEmptyGroups: true,
    showSubIssues: true,
    calendar: { mode: "month", showWeekends: true },
    properties: {},
    ...overrides,
  };
}

function renderLayout(
  options: {
    orderBy?: string;
    narrow?: boolean;
    totalCount?: number;
    issues?: IssueListItem[];
    projectRole?: number | null;
    workspaceRole?: number;
    completedCycle?: boolean;
  } = {}
) {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  vi.stubGlobal("matchMedia", () => ({
    matches: options.narrow ?? false,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  createAppClient("https://api.test", (() => {
    throw new Error("unexpected fetch in a seeded render");
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(ganttStatesKey(SLUG, PROJECT), [
    {
      id: STATE,
      name: "Doing",
      color: "#123456",
      group: "started",
      sequence: 2,
      project_id: PROJECT,
      workspace_id: SLUG,
      description: "",
      default: false,
    },
  ]);
  queryClient.setQueryData(issueKeys.detail(SLUG, ISSUES[0]!.id), { ...issue(), id: ISSUES[0]!.id });
  queryClient.setQueryData(sessionKeys.workspaces, [
    { id: "ws-1", name: "Acme", slug: SLUG, logo_url: null, total_members: 3, role: options.workspaceRole ?? 15 },
  ]);
  queryClient.setQueryData(
    authKeys.projectRoles(SLUG),
    options.projectRole === null ? {} : { [PROJECT]: options.projectRole ?? 15 }
  );
  render(
    <QueryClientProvider client={queryClient}>
      <React.Suspense fallback={<p>suspended</p>}>
        <GanttLayout
          workspaceSlug={SLUG}
          projectId={PROJECT}
          projectIdentifier="WEB"
          issues={options.issues ?? ISSUES}
          totalCount={options.totalCount ?? (options.issues ?? ISSUES).length}
          prefs={prefs({ orderBy: options.orderBy ?? "sort_order" })}
          weekStart={0}
          {...(options.completedCycle === undefined ? {} : { completedCycle: options.completedCycle })}
        />
      </React.Suspense>
    </QueryClientProvider>
  );
  return { queryClient };
}

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
});

describe("orderedRows", () => {
  it("keeps the server order for every preference", () => {
    // The server answers `order_by`; a local reverse would double it.
    for (const orderBy of ["sort_order", "-sort_order", "-created_at", "start_date"]) {
      const rows = orderedRows(ISSUES, prefs({ orderBy }), "WEB");
      expect(rows.map((row) => row.name)).toEqual(["First", "Second", "Third"]);
    }
  });
});

describe("barColorsFor", () => {
  it("takes the color of each row's state and skips uncolored rows", () => {
    const rows = buildRows(ISSUES, "WEB");
    expect(barColorsFor(rows, [{ id: STATE, color: "#123456" } as never])).toEqual({ [ISSUES[1]!.id]: "#123456" });
  });

  it("returns no colors without states", () => {
    expect(barColorsFor(buildRows(ISSUES, "WEB"), [])).toEqual({});
  });
});

describe("GanttLayout", () => {
  it("renders the chart with one sidebar row per issue", () => {
    renderLayout();
    expect(screen.getByTestId("gantt-layout")).toBeInTheDocument();
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(3);
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("3 work items");
  });

  it("keeps the server order under a reverse manual sort", () => {
    // `order_by=-sort_order` already arrives descending; the layout must
    // not flip it back (ISS-044/045).
    renderLayout({ orderBy: "-sort_order" });
    const names = screen.getAllByTestId("gantt-sidebar-row").map((row) => row.textContent);
    expect(names[0]).toContain("First");
    expect(names[2]).toContain("Third");
  });

  it("shows the server total, not the loaded page, in the header (ISS-044)", () => {
    // The list query is one page; the header count is the issue total.
    renderLayout({ totalCount: 240 });
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("240 work items");
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(3);
  });

  it("never reports fewer rows than it renders", () => {
    renderLayout({ totalCount: 1 });
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("3 work items");
  });

  it("renders rows in the order the server sent them", () => {
    // Manual order is the server's `order_by=sort_order` order; the layout
    // must not re-sort or reverse it (ISS-044/045).
    const ranks = [
      issue({ id: "423e4567-e89b-12d3-a456-426614174020", name: "Rank 30", sequence_id: 30, sort_order: 30 }),
      issue({ id: "423e4567-e89b-12d3-a456-426614174021", name: "Rank 10", sequence_id: 10, sort_order: 10 }),
    ];
    renderLayout({ issues: ranks });
    const names = screen.getAllByTestId("gantt-sidebar-row").map((row) => row.textContent);
    expect(names[0]).toContain("Rank 30");
    expect(names[1]).toContain("Rank 10");
  });

  it("starts every sidebar link at the workspace browse route", () => {
    renderLayout();
    for (const link of screen.getAllByRole("link")) {
      expect(link).toHaveAttribute("href", expect.stringMatching(/^\/acme\/browse\/WEB-\d+$/));
    }
  });

  it("keeps an unconfirmed create visible but inert in the timeline", () => {
    // A quick create prepends a temp row to the list cache; it has no
    // server identity, so it must not link or open a 404ing peek (ISS-049).
    renderLayout({
      issues: [issue({ id: "temp-abc", name: "Draft title", sequence_id: 0 }), ...ISSUES],
    });
    expect(screen.getAllByTestId("gantt-sidebar-row")).toHaveLength(4);
    expect(screen.queryByRole("link", { name: "Draft title" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("gantt-row-unconfirmed"));
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
  });

  it("opens the shared peek for a clicked row and closes it", () => {
    renderLayout();
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "First" }));
    expect(screen.getByTestId("issue-preview")).toBeInTheDocument();
    expect(document.querySelector(`[data-lane="${ISSUES[0]!.id}"]`)).toHaveAttribute("data-peeked", "true");
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
    expect(document.querySelector(`[data-lane="${ISSUES[0]!.id}"]`)).not.toHaveAttribute("data-peeked");
  });
});

describe("GanttLayout edit standing (ISS-050–055)", () => {
  it("omits every edit affordance without standing", () => {
    renderLayout({ projectRole: null });
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
    expect(screen.queryAllByTestId("gantt-resize-left")).toHaveLength(0);
    expect(screen.queryAllByTestId("gantt-undated-lane")).toHaveLength(0);
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });

  it("gives a project member the bars, add lane, quick-add and reorder", () => {
    renderLayout({
      projectRole: 15,
      issues: [issue({ start_date: "2026-10-01", target_date: "2026-10-05" }), ...ISSUES.slice(1)],
    });
    expect(screen.getAllByTestId("gantt-resize-left")).toHaveLength(1);
    expect(screen.getAllByTestId("gantt-undated-lane")).toHaveLength(2);
    expect(screen.getByTestId("gantt-quick-add")).toBeInTheDocument();
    expect(screen.getByTestId("gantt-sidebar")).toHaveAttribute("data-reorder", "true");
  });

  it("keeps a member's timeline read-only outside manual order", () => {
    renderLayout({
      projectRole: 15,
      orderBy: "-created_at",
      issues: [issue({ start_date: "2026-10-01", target_date: "2026-10-05" }), ...ISSUES.slice(1)],
    });
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
    // Bars still move and lanes still plant outside manual order.
    expect(screen.getAllByTestId("gantt-resize-left")).toHaveLength(1);
  });

  it("treats a descending manual sort as non-manual for reorder (ISS-050)", () => {
    renderLayout({
      projectRole: 15,
      orderBy: "-sort_order",
      issues: [issue({ start_date: "2026-10-01", target_date: "2026-10-05" }), ...ISSUES.slice(1)],
    });
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });

  it("gives a workspace admin with no project membership no edit affordance (ISS-058)", () => {
    renderLayout({ workspaceRole: 20, projectRole: null });
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
    expect(screen.queryAllByTestId("gantt-resize-left")).toHaveLength(0);
    expect(screen.queryAllByTestId("gantt-undated-lane")).toHaveLength(0);
    expect(screen.getByTestId("gantt-sidebar")).not.toHaveAttribute("data-reorder");
  });

  it("hides the quick-add in a completed cycle", () => {
    renderLayout({ projectRole: 15, completedCycle: true });
    expect(screen.queryByTestId("gantt-quick-add")).not.toBeInTheDocument();
  });
});
