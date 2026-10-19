// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline lazy boundary (parity/specs/issues.md §6, ISS-055): the route
// chunk carries only the suspense boundary; the layout receives every
// prop, including a completed cycle's status, so its quick-add guard is
// reachable from a cycle timeline mount.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { GanttTimeline } from "./GanttTimeline.js";

const forwarded = vi.hoisted(() => ({ props: null as Record<string, unknown> | null }));

vi.mock("./GanttLayout.js", () => ({
  GanttLayout: (props: Record<string, unknown>) => {
    forwarded.props = props;
    return <div data-testid="layout-stub" />;
  },
}));

vi.mock("../dropdowns/weekStart.js", () => ({ useWeekStart: () => 0 }));

const PREFS = {
  layout: "gantt_chart",
  groupBy: null,
  subGroupBy: null,
  orderBy: "sort_order",
  showEmptyGroups: true,
  showSubIssues: true,
  calendar: { mode: "month", showWeekends: true },
  properties: {},
} as const;

function renderTimeline(completedCycle?: boolean): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <React.Suspense fallback={<p>loading</p>}>
        <GanttTimeline
          workspaceSlug="acme"
          projectId="project-1"
          projectIdentifier="WEB"
          issues={[]}
          totalCount={0}
          prefs={PREFS as never}
          {...(completedCycle === undefined ? {} : { completedCycle })}
        />
      </React.Suspense>
    </QueryClientProvider>
  );
}

afterEach(() => {
  forwarded.props = null;
});

describe("GanttTimeline", () => {
  it("forwards the completed-cycle status into the layout (ISS-055)", async () => {
    renderTimeline(true);
    expect(await screen.findByTestId("layout-stub")).toBeInTheDocument();
    expect(forwarded.props?.["completedCycle"]).toBe(true);
  });

  it("defaults a project timeline to an unfinished cycle", async () => {
    renderTimeline();
    expect(await screen.findByTestId("layout-stub")).toBeInTheDocument();
    expect(forwarded.props?.["completedCycle"]).toBe(false);
  });
});
