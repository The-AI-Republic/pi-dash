// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// The five layout chunks are heavy; the boundary's job is to mount the one
// the stored record names with the view's stored expression and settings.
// Each stand-in echoes the props this boundary must forward.
const mounted = vi.hoisted(() => ({ name: "" }));

vi.mock("../list/ListLayout.js", () => ({
  ListLayout: (props: { richFilters?: unknown; compact?: boolean }) => {
    mounted.name = "list";
    return (
      <div
        data-testid="layout-list"
        data-filters={JSON.stringify(props.richFilters ?? {})}
        data-compact={String(props.compact)}
      />
    );
  },
}));
vi.mock("../kanban/KanbanBoard.js", () => ({
  KanbanBoard: (props: { richFilters?: unknown }) => {
    mounted.name = "kanban";
    return <div data-testid="layout-kanban" data-filters={JSON.stringify(props.richFilters ?? {})} />;
  },
}));
vi.mock("../calendar/CalendarEntry.js", () => ({
  CalendarEntry: (props: { richExpression?: unknown }) => {
    mounted.name = "calendar";
    return <div data-testid="layout-calendar" data-filters={JSON.stringify(props.richExpression ?? {})} />;
  },
}));
vi.mock("../spreadsheet/SpreadsheetLayout.js", () => ({
  SpreadsheetLayout: (props: { richExpression?: unknown; hideEstimate?: boolean }) => {
    mounted.name = "spreadsheet";
    return (
      <div
        data-testid="layout-spreadsheet"
        data-filters={JSON.stringify(props.richExpression ?? {})}
        data-hide-estimate={String(props.hideEstimate)}
      />
    );
  },
}));
vi.mock("../gantt/GanttTimeline.js", () => ({
  GanttTimeline: (props: { issues: unknown[]; totalCount: number }) => {
    mounted.name = "gantt";
    return <div data-testid="layout-gantt" data-rows={props.issues.length} data-total={props.totalCount} />;
  },
}));

import { parseDisplayPrefs } from "../prefs/displayPrefs.js";
import { ViewLayoutBody, preloadViewLayout } from "./ViewLayoutBody.js";

const EXPRESSION = { and: [{ priority__in: "urgent" }] };

function renderBody(
  layout: React.ComponentProps<typeof ViewLayoutBody>["layout"],
  overrides: Partial<React.ComponentProps<typeof ViewLayoutBody>> = {}
) {
  return render(
    <ViewLayoutBody
      layout={layout}
      workspaceSlug="acme"
      projectId="project-1"
      projectIdentifier="WEB"
      entity={{ scope: "project", projectId: "project-1" }}
      prefs={parseDisplayPrefs({ order_by: "-created_at" }, {})}
      canEdit
      richExpression={EXPRESSION}
      hideCycle
      hideModule
      hideEstimate
      weekStart={1}
      issues={[]}
      totalCount={0}
      compact={false}
      {...overrides}
    />
  );
}

describe("ViewLayoutBody", () => {
  it("mounts the list layout with the stored expression", async () => {
    renderBody("list");
    const node = await screen.findByTestId("layout-list");
    expect(mounted.name).toBe("list");
    expect(JSON.parse(node.getAttribute("data-filters") ?? "{}")).toEqual(EXPRESSION);
  });

  it("passes the compact density to the list", async () => {
    renderBody("list", { compact: true });
    const node = await screen.findByTestId("layout-list");
    expect(node).toHaveAttribute("data-compact", "true");
  });

  it("mounts the board with the stored expression", async () => {
    renderBody("kanban");
    const node = await screen.findByTestId("layout-kanban");
    expect(mounted.name).toBe("kanban");
    expect(JSON.parse(node.getAttribute("data-filters") ?? "{}")).toEqual(EXPRESSION);
  });

  it("mounts the calendar with the stored expression", async () => {
    renderBody("calendar");
    const node = await screen.findByTestId("layout-calendar");
    expect(JSON.parse(node.getAttribute("data-filters") ?? "{}")).toEqual(EXPRESSION);
  });

  it("mounts the sheet with the stored expression and project flags", async () => {
    renderBody("spreadsheet", { hideEstimate: false });
    const node = await screen.findByTestId("layout-spreadsheet");
    expect(JSON.parse(node.getAttribute("data-filters") ?? "{}")).toEqual(EXPRESSION);
    expect(node).toHaveAttribute("data-hide-estimate", "false");
  });

  it("hands the timeline its prefetched rows and total", async () => {
    const rows = [
      { id: "i1", name: "One", sequence_id: 1 },
      { id: "i2", name: "Two", sequence_id: 2 },
    ] as never[];
    renderBody("gantt_chart", { issues: rows, totalCount: 9 });
    const node = await screen.findByTestId("layout-gantt");
    expect(node).toHaveAttribute("data-rows", "2");
    expect(node).toHaveAttribute("data-total", "9");
  });

  it("starts the chunk for each stored layout", async () => {
    await expect(preloadViewLayout("list")).resolves.toBeTruthy();
    await expect(preloadViewLayout("kanban")).resolves.toBeTruthy();
    await expect(preloadViewLayout("calendar")).resolves.toBeTruthy();
    await expect(preloadViewLayout("spreadsheet")).resolves.toBeTruthy();
    await expect(preloadViewLayout("gantt_chart")).resolves.toBeTruthy();
  });
});
