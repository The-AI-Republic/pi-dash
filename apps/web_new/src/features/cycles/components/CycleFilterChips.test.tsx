// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Applied-filter chips: one group per dimension, one pill per value,
// dimension clears, and a clear-all that resets every filter at once.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { resetCycleFilterStore, useCycleFilterStore } from "../api/filterStore.js";
import { CYCLES_PROJECT_ID } from "../testing.js";
import { CycleFilterChips } from "./CycleFilterChips.js";

beforeEach(() => {
  resetCycleFilterStore();
});

afterEach(() => {
  resetCycleFilterStore();
});

function renderChips(projectId: string = CYCLES_PROJECT_ID): void {
  render(<CycleFilterChips projectId={projectId} />);
}

function applyStatus(...values: string[]): void {
  for (const value of values) useCycleFilterStore.getState().toggleStatus(CYCLES_PROJECT_ID, value);
}

function applyDates(dimension: "startDate" | "endDate", ...values: string[]): void {
  useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, dimension, values);
}

describe("CycleFilterChips", () => {
  it("renders nothing before any filter applies", () => {
    renderChips();
    expect(screen.queryByTestId("cycles-filter-chips")).not.toBeInTheDocument();
  });

  it("renders nothing for a project without filters", () => {
    applyStatus("upcoming");
    renderChips("other-project");
    expect(screen.queryByTestId("cycles-filter-chips")).not.toBeInTheDocument();
  });

  it("groups one pill per value under its dimension label", () => {
    applyStatus("upcoming", "draft");
    applyDates("startDate", "1_weeks;after;fromnow");
    applyDates("endDate", "2026-11-01;after", "2026-12-01;before");
    renderChips();
    const row = screen.getByTestId("cycles-filter-chips");
    expect(row).toBeInTheDocument();
    const status = screen.getByTestId("cycles-filter-chip-status");
    expect(status).toHaveTextContent("Status");
    expect(status).toHaveTextContent("Yet to start");
    expect(status).toHaveTextContent("Draft");
    const start = screen.getByTestId("cycles-filter-chip-startDate");
    expect(start).toHaveTextContent("Start date");
    expect(start).toHaveTextContent("1 week from now");
    const end = screen.getByTestId("cycles-filter-chip-endDate");
    expect(end).toHaveTextContent("Due date");
    expect(end).toHaveTextContent("After 2026-11-01");
    expect(end).toHaveTextContent("Before 2026-12-01");
    expect(screen.getByRole("button", { name: "Clear all" })).toBeInTheDocument();
  });

  it("reads an unknown lifecycle as a draft pill and unknown entries raw", () => {
    applyStatus("bogus");
    applyDates("startDate", "3_weeks;after;fromnow", "tomorrow;after");
    renderChips();
    expect(screen.getByTestId("cycles-filter-chip-status")).toHaveTextContent("Draft");
    const start = screen.getByTestId("cycles-filter-chip-startDate");
    expect(start).toHaveTextContent("3_weeks;after;fromnow");
    expect(start).toHaveTextContent("tomorrow;after");
  });

  it("removes exactly one value per pill and drops the emptied group", () => {
    applyStatus("upcoming", "draft");
    applyDates("startDate", "1_weeks;after;fromnow");
    renderChips();
    fireEvent.click(screen.getByRole("button", { name: "Remove Yet to start" }));
    expect(screen.getByTestId("cycles-filter-chip-status")).toHaveTextContent("Draft");
    expect(screen.getByTestId("cycles-filter-chip-status")).not.toHaveTextContent("Yet to start");
    expect(screen.getByTestId("cycles-filter-chip-startDate")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove Draft" }));
    expect(screen.queryByTestId("cycles-filter-chip-status")).not.toBeInTheDocument();
    expect(screen.getByTestId("cycles-filter-chip-startDate")).toBeInTheDocument();
  });

  it("clears one dimension and keeps the others", () => {
    applyStatus("upcoming");
    applyDates("startDate", "1_weeks;after;fromnow");
    renderChips();
    fireEvent.click(screen.getByRole("button", { name: "Clear Status filters" }));
    expect(screen.queryByTestId("cycles-filter-chip-status")).not.toBeInTheDocument();
    expect(screen.getByTestId("cycles-filter-chip-startDate")).toBeInTheDocument();
    expect(screen.getByTestId("cycles-filter-chips")).toBeInTheDocument();
  });

  it("clears every dimension at once and unmounts the row", () => {
    applyStatus("upcoming");
    applyDates("startDate", "1_weeks;after;fromnow");
    renderChips();
    fireEvent.click(screen.getByRole("button", { name: "Clear all" }));
    expect(screen.queryByTestId("cycles-filter-chips")).not.toBeInTheDocument();
    expect(useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID)).toEqual({
      status: [],
      startDate: [],
      endDate: [],
    });
  });

  it("removes one custom bound and keeps its pair", () => {
    applyDates("endDate", "2026-11-01;after", "2026-12-01;before");
    renderChips();
    fireEvent.click(screen.getByRole("button", { name: "Remove After 2026-11-01" }));
    const end = screen.getByTestId("cycles-filter-chip-endDate");
    expect(end).not.toHaveTextContent("After 2026-11-01");
    expect(end).toHaveTextContent("Before 2026-12-01");
  });
});
