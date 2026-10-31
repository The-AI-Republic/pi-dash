// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LayoutKind } from "../layouts/kinds.js";
import {
  CalendarReloadBar,
  CalendarSkeleton,
  GanttSkeleton,
  KanbanSkeleton,
  LayoutSkeleton,
  SpreadsheetSkeleton,
} from "./skeletons.js";

function rowCount(testid: string): number {
  return screen.getByTestId(testid).querySelectorAll('[data-skeleton-row="true"]').length;
}

describe("layout skeletons", () => {
  it("renders header plus body rows for the spreadsheet", () => {
    render(<SpreadsheetSkeleton />);
    expect(rowCount("issues-skeleton-spreadsheet")).toBe(8);
  });

  it("renders pulsing day blocks under a reload bar for the calendar", () => {
    render(<CalendarSkeleton />);
    expect(rowCount("issues-skeleton-calendar")).toBe(35);
    expect(screen.getByTestId("issues-calendar-reload-bar")).toBeInTheDocument();
    const blocks = screen.getByTestId("issues-skeleton-calendar").querySelectorAll(".animate-pulse");
    expect(blocks.length).toBeGreaterThan(35);
  });

  it("renders the reload bar standalone for calendar refetches", () => {
    render(<CalendarReloadBar />);
    expect(screen.getByTestId("issues-calendar-reload-bar")).toBeInTheDocument();
  });

  it("renders grouped columns for the board", () => {
    render(<KanbanSkeleton />);
    expect(rowCount("issues-skeleton-kanban")).toBe(3);
  });

  it("renders sidebar rows beside chart bars for the timeline", () => {
    render(<GanttSkeleton />);
    expect(rowCount("issues-skeleton-gantt")).toBe(6);
  });

  it.each([
    ["list", "issues-skeleton-list"],
    ["kanban", "issues-skeleton-kanban"],
    ["calendar", "issues-skeleton-calendar"],
    ["spreadsheet", "issues-skeleton-spreadsheet"],
    ["gantt_chart", "issues-skeleton-gantt"],
  ] as Array<[LayoutKind, string]>)("routes %s to its skeleton", (kind, testid) => {
    render(<LayoutSkeleton kind={kind} />);
    expect(screen.getByTestId(testid)).toBeInTheDocument();
  });
});
