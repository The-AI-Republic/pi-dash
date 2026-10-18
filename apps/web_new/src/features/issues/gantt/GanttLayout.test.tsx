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

function renderLayout(options: { orderBy?: string; narrow?: boolean } = {}) {
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
  render(
    <QueryClientProvider client={queryClient}>
      <React.Suspense fallback={<p>suspended</p>}>
        <GanttLayout
          workspaceSlug={SLUG}
          projectId={PROJECT}
          projectIdentifier="WEB"
          issues={ISSUES}
          prefs={prefs({ orderBy: options.orderBy ?? "sort_order" })}
          weekStart={0}
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
  it("keeps the server order for every preference but a reverse manual sort", () => {
    const rows = orderedRows(ISSUES, prefs({ orderBy: "sort_order" }), "WEB");
    expect(rows.map((row) => row.name)).toEqual(["First", "Second", "Third"]);
    const reversed = orderedRows(ISSUES, prefs({ orderBy: "-sort_order" }), "WEB");
    expect(reversed.map((row) => row.name)).toEqual(["Third", "Second", "First"]);
    const created = orderedRows(ISSUES, prefs({ orderBy: "-created_at" }), "WEB");
    expect(created.map((row) => row.name)).toEqual(["First", "Second", "Third"]);
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

  it("renders rows in reverse manual order", () => {
    renderLayout({ orderBy: "-sort_order" });
    const names = screen.getAllByTestId("gantt-sidebar-row").map((row) => row.textContent);
    expect(names[0]).toContain("Third");
    expect(names[2]).toContain("First");
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
