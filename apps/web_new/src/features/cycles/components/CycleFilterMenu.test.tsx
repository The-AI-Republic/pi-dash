// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles filters menu: the trigger counts applied values, the panel
// offers lifecycle states plus the start and due windows on the shared
// section chrome, presets toggle in place, Custom bounds one day from
// either side or pairs two ends, and the menu stays open throughout.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { profileQueryOptions } from "../../../core/session/queries.js";
import { resetCycleFilterStore, useCycleFilterStore } from "../api/filterStore.js";
import type { CycleFilterDimension } from "../lib/cycleFilters.js";
import { CYCLES_PROJECT_ID } from "../testing.js";
import { CycleFilterMenu, focusFirstCalendarDay } from "./CycleFilterMenu.js";

function renderMenu(offered?: readonly CycleFilterDimension[] | undefined): void {
  createAppClient("https://api.test", (() => {
    throw new Error("unexpected network fetch in a seeded render");
  }) as never);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(profileQueryOptions({} as never).queryKey, { start_of_the_week: 1 } as never);
  render(
    <QueryClientProvider client={client}>
      <CycleFilterMenu projectId={CYCLES_PROJECT_ID} offered={offered} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  cleanup();
  resetAppClient();
  resetCycleFilterStore();
});

function openMenu(): void {
  fireEvent.click(screen.getByTestId("cycles-filters-trigger"));
}

function section(testId: string): HTMLElement {
  return screen.getByTestId(testId);
}

function stored(): { status: readonly string[]; startDate: readonly string[]; endDate: readonly string[] } {
  return useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID);
}

function inMonthDays(calendarTestId: string): HTMLButtonElement[] {
  const calendar = screen.getByTestId(calendarTestId);
  return [...calendar.querySelectorAll<HTMLButtonElement>('button[data-day][data-in-month="true"]')];
}

describe("focusFirstCalendarDay", () => {
  it("ignores a missing host or a host without day cells", () => {
    expect(() => focusFirstCalendarDay(null)).not.toThrow();
    const host = document.createElement("div");
    expect(() => focusFirstCalendarDay(host)).not.toThrow();
    expect(document.activeElement).not.toBe(host);
  });

  it("moves focus to the first day cell", () => {
    const host = document.createElement("div");
    host.innerHTML = '<button type="button" data-day="2026-11-01">1</button>';
    document.body.append(host);
    try {
      focusFirstCalendarDay(host);
      expect(host.querySelector("button")).toHaveFocus();
    } finally {
      host.remove();
    }
  });
});

describe("CycleFilterMenu", () => {
  it("keeps the panel closed until the trigger opens it", () => {
    renderMenu();
    expect(screen.queryByTestId("cycles-filters-panel")).not.toBeInTheDocument();
    expect(screen.getByTestId("cycles-filters-trigger")).toHaveAccessibleName("Filters");
    expect(screen.queryByTestId("cycles-filters-count")).not.toBeInTheDocument();
    openMenu();
    expect(screen.getByTestId("cycles-filters-panel")).toBeInTheDocument();
  });

  it("offers status, start and due sections in order", () => {
    renderMenu();
    openMenu();
    const panel = screen.getByTestId("cycles-filters-panel");
    const titles = [...panel.querySelectorAll("[data-section-title]")].map((node) =>
      node.getAttribute("data-section-title")
    );
    expect(titles).toEqual(["Status of the cycle", "Start date", "Due date"]);
    expect(within(section("cycle-filter-status")).getByRole("checkbox", { name: "In progress" })).toBeInTheDocument();
    expect(within(section("cycle-filter-status")).getByRole("checkbox", { name: "Yet to start" })).toBeInTheDocument();
    expect(within(section("cycle-filter-status")).getByRole("checkbox", { name: "Completed" })).toBeInTheDocument();
    expect(within(section("cycle-filter-status")).getByRole("checkbox", { name: "Draft" })).toBeInTheDocument();
    expect(
      within(section("cycle-filter-start-date")).getByRole("checkbox", { name: "1 week from now" })
    ).toBeInTheDocument();
    expect(
      within(section("cycle-filter-due-date")).getByRole("checkbox", { name: "2 months from now" })
    ).toBeInTheDocument();
  });

  it("toggles lifecycle states and counts them on the trigger", () => {
    renderMenu();
    openMenu();
    const options = within(section("cycle-filter-status"));
    fireEvent.click(options.getByRole("checkbox", { name: "Yet to start" }));
    fireEvent.click(options.getByRole("checkbox", { name: "Draft" }));
    expect(stored().status).toEqual(["upcoming", "draft"]);
    // The menu stays open while options toggle.
    expect(screen.getByTestId("cycles-filters-panel")).toBeInTheDocument();
    expect(screen.getByTestId("cycles-filters-trigger")).toHaveAccessibleName("Filters, 2 applied");
    expect(screen.getByTestId("cycles-filters-count")).toHaveTextContent("2");
    fireEvent.click(options.getByRole("checkbox", { name: "Yet to start" }));
    expect(stored().status).toEqual(["draft"]);
    expect(screen.getByTestId("cycles-filters-trigger")).toHaveAccessibleName("Filters, 1 applied");
  });

  it("toggles presets per window and keeps the windows apart", () => {
    renderMenu();
    openMenu();
    fireEvent.click(within(section("cycle-filter-start-date")).getByRole("checkbox", { name: "1 week from now" }));
    fireEvent.click(within(section("cycle-filter-due-date")).getByRole("checkbox", { name: "1 month from now" }));
    expect(stored().startDate).toEqual(["1_weeks;after;fromnow"]);
    expect(stored().endDate).toEqual(["1_months;after;fromnow"]);
    expect(screen.getByTestId("cycles-filters-count")).toHaveTextContent("2");
    fireEvent.click(within(section("cycle-filter-start-date")).getByRole("checkbox", { name: "1 week from now" }));
    expect(stored().startDate).toEqual([]);
    expect(stored().endDate).toEqual(["1_months;after;fromnow"]);
  });

  it("focuses the calendar when Custom ticks with nothing stored", () => {
    renderMenu();
    openMenu();
    const custom = within(section("cycle-filter-start-date")).getByRole("checkbox", { name: "Custom" });
    expect(custom).toHaveAttribute("aria-checked", "false");
    fireEvent.click(custom);
    expect(stored().startDate).toEqual([]);
    const first = screen.getByTestId("cycle-filter-start-date-custom-calendar").querySelector("button[data-day]");
    expect(first).not.toBeNull();
    expect(first).toHaveFocus();
  });

  it("commits one picked day after the chosen bound", () => {
    renderMenu();
    openMenu();
    const scope = within(section("cycle-filter-start-date"));
    fireEvent.click(scope.getByRole("radio", { name: "After" }));
    const days = inMonthDays("cycle-filter-start-date-custom-calendar");
    const iso = days[0]?.getAttribute("data-day") ?? "";
    expect(iso).not.toBe("");
    fireEvent.click(days[0] as HTMLElement);
    expect(stored().startDate).toEqual([`${iso};after`]);
    expect(scope.getByRole("checkbox", { name: "Custom" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("cycles-filters-panel")).toBeInTheDocument();
  });

  it("commits one picked day before the chosen bound", () => {
    renderMenu();
    openMenu();
    const scope = within(section("cycle-filter-due-date"));
    fireEvent.click(scope.getByRole("radio", { name: "Before" }));
    const days = inMonthDays("cycle-filter-due-date-custom-calendar");
    const iso = days[0]?.getAttribute("data-day") ?? "";
    fireEvent.click(days[0] as HTMLElement);
    expect(stored().endDate).toEqual([`${iso};before`]);
  });

  it("pairs two picked ends into one custom range", () => {
    renderMenu();
    openMenu();
    const scope = within(section("cycle-filter-start-date"));
    expect(scope.getByRole("radio", { name: "Range" })).toHaveAttribute("aria-checked", "true");
    const days = inMonthDays("cycle-filter-start-date-custom-calendar");
    const first = days[0]?.getAttribute("data-day") ?? "";
    const last = days[days.length - 1]?.getAttribute("data-day") ?? "";
    expect(first).not.toBe(last);
    fireEvent.click(days[0] as HTMLElement);
    expect(stored().startDate).toEqual([]);
    expect(screen.getByRole("status")).toHaveTextContent("Pick an end date");
    fireEvent.click(days[days.length - 1] as HTMLElement);
    expect(stored().startDate).toEqual([`${first};after`, `${last};before`]);
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("asks for the start again after clearing the pending pair", () => {
    renderMenu();
    openMenu();
    const days = inMonthDays("cycle-filter-start-date-custom-calendar");
    fireEvent.click(days[0] as HTMLElement);
    expect(screen.getByRole("status")).toHaveTextContent("Pick an end date");
    // Clicking the pending day again clears both ends.
    fireEvent.click(days[0] as HTMLElement);
    expect(stored().startDate).toEqual([]);
    expect(screen.getByRole("status")).toHaveTextContent("Pick a start date");
  });

  it("unticking Custom clears the custom half and keeps presets", () => {
    useCycleFilterStore
      .getState()
      .setDateValues(CYCLES_PROJECT_ID, "startDate", ["1_weeks;after;fromnow", "2026-11-01;after"]);
    renderMenu();
    openMenu();
    const scope = within(section("cycle-filter-start-date"));
    expect(scope.getByRole("checkbox", { name: "Custom" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(scope.getByRole("checkbox", { name: "Custom" }));
    expect(stored().startDate).toEqual(["1_weeks;after;fromnow"]);
    expect(scope.getByRole("checkbox", { name: "Custom" })).toHaveAttribute("aria-checked", "false");
  });

  it("reopens a stored bound in its own shape", () => {
    useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, "startDate", ["2026-11-01;after"]);
    useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, "endDate", ["2026-12-01;before"]);
    renderMenu();
    openMenu();
    expect(within(section("cycle-filter-start-date")).getByRole("radio", { name: "After" })).toHaveAttribute(
      "aria-checked",
      "true"
    );
    expect(within(section("cycle-filter-due-date")).getByRole("radio", { name: "Before" })).toHaveAttribute(
      "aria-checked",
      "true"
    );
  });

  it("reopens a stored pair and a bare day on the range shape", () => {
    useCycleFilterStore
      .getState()
      .setDateValues(CYCLES_PROJECT_ID, "startDate", ["2026-11-01;after", "2026-11-30;before"]);
    useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, "endDate", ["2026-12-15"]);
    renderMenu();
    openMenu();
    expect(within(section("cycle-filter-start-date")).getByRole("radio", { name: "Range" })).toHaveAttribute(
      "aria-checked",
      "true"
    );
    expect(within(section("cycle-filter-due-date")).getByRole("radio", { name: "Range" })).toHaveAttribute(
      "aria-checked",
      "true"
    );
  });

  it("converts the stored day when the shape changes", () => {
    useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, "startDate", ["2026-11-01;after"]);
    renderMenu();
    openMenu();
    const scope = within(section("cycle-filter-start-date"));
    fireEvent.click(scope.getByRole("radio", { name: "Before" }));
    expect(stored().startDate).toEqual(["2026-11-01;before"]);
    fireEvent.click(scope.getByRole("radio", { name: "Range" }));
    // The stored bound waits untouched until the second end lands.
    expect(stored().startDate).toEqual(["2026-11-01;before"]);
    expect(screen.getByRole("status")).toHaveTextContent("Pick an end date");
    const days = inMonthDays("cycle-filter-start-date-custom-calendar");
    const end = days.find((day) => day.getAttribute("data-day") === "2026-11-20");
    expect(end).toBeDefined();
    fireEvent.click(end as HTMLElement);
    expect(stored().startDate).toEqual(["2026-11-01;after", "2026-11-20;before"]);
  });

  it("converts the stored day when switching back to After", () => {
    useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, "startDate", ["2026-11-01;before"]);
    renderMenu();
    openMenu();
    fireEvent.click(within(section("cycle-filter-start-date")).getByRole("radio", { name: "After" }));
    expect(stored().startDate).toEqual(["2026-11-01;after"]);
  });

  it("changes shape without storing when no day applies", () => {
    renderMenu();
    openMenu();
    const scope = within(section("cycle-filter-start-date"));
    fireEvent.click(scope.getByRole("radio", { name: "After" }));
    expect(stored().startDate).toEqual([]);
    expect(scope.getByRole("radio", { name: "After" })).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("omits the lifecycle section when only the windows are offered", () => {
    renderMenu(["startDate", "endDate"]);
    openMenu();
    expect(screen.queryByTestId("cycle-filter-status")).not.toBeInTheDocument();
    expect(screen.getByTestId("cycle-filter-start-date")).toBeInTheDocument();
    expect(screen.getByTestId("cycle-filter-due-date")).toBeInTheDocument();
  });

  it("omits the windows the host does not offer", () => {
    renderMenu(["status"]);
    openMenu();
    expect(screen.getByTestId("cycle-filter-status")).toBeInTheDocument();
    expect(screen.queryByTestId("cycle-filter-start-date")).not.toBeInTheDocument();
    expect(screen.queryByTestId("cycle-filter-due-date")).not.toBeInTheDocument();
  });
});
