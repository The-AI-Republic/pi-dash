// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { makeMember } from "../testing.js";
import { EMPTY_VIEW_LIST_FILTERS, type ViewListFilters } from "../lib/viewListControls.js";
import { jsonResponse } from "../testing.js";
import { ProjectViewsFilterMenu } from "./ProjectViewsFilterMenu.js";

const MEMBERS = [makeMember("user-1"), makeMember("user-2")];

function Harness({ initial = EMPTY_VIEW_LIST_FILTERS }: { initial?: ViewListFilters }): React.ReactElement {
  const [filters, setFilters] = React.useState(initial);
  return (
    <ProjectViewsFilterMenu
      filters={filters}
      members={MEMBERS}
      selfId="user-1"
      onFiltersChange={(next) => {
        setFilters(next);
        onFiltersChange(next);
      }}
    />
  );
}

let onFiltersChange = vi.fn();

afterEach(() => {
  onFiltersChange = vi.fn();
  delete edition.flags["view-access-filter"];
});

function renderMenu(initial?: ViewListFilters): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Harness {...(initial === undefined ? {} : { initial })} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  onFiltersChange = vi.fn();
  createAppClient("https://api.test", (async (url: string) =>
    jsonResponse(url, 200, { start_of_the_week: 0 })) as never);
});

afterEach(() => {
  resetAppClient();
});

function open(): void {
  fireEvent.click(screen.getByTestId("project-views-filter-trigger"));
}

function panel(): HTMLElement {
  return screen.getByTestId("project-views-filter-panel");
}

function section(name: string): HTMLElement {
  return within(panel()).getByRole("region", { name });
}

function row(name: string, scope: HTMLElement = panel()): HTMLElement {
  return within(scope).getByRole("checkbox", { name });
}

describe("ProjectViewsFilterMenu", () => {
  it("shows favorites, created date and created by but no access on the OSS build", () => {
    renderMenu();
    open();

    expect(within(panel()).getByText("Favorites")).toBeInTheDocument();
    expect(within(panel()).getByText("Created date")).toBeInTheDocument();
    expect(within(panel()).getByText("Created by")).toBeInTheDocument();
    expect(within(panel()).queryByText("Access")).not.toBeInTheDocument();
    expect(within(panel()).queryByRole("checkbox", { name: "Public" })).not.toBeInTheDocument();
  });

  it("offers the three relative presets and the custom row", () => {
    renderMenu();
    open();

    expect(
      within(section("Created date"))
        .getAllByRole("checkbox")
        .map((entry) => entry.textContent)
    ).toEqual(["1 week ago", "2 weeks ago", "1 month ago", "Custom"]);
  });

  it("toggles favorites and shows the active badge", () => {
    renderMenu();
    open();

    expect(screen.queryByTestId("project-views-filter-badge")).not.toBeInTheDocument();
    fireEvent.click(row("Favorites"));

    expect(onFiltersChange).toHaveBeenCalledWith({ ...EMPTY_VIEW_LIST_FILTERS, favorites: true });
    expect(screen.getByTestId("project-views-filter-badge")).toBeInTheDocument();
    expect(screen.queryByTestId("project-views-filter-panel")).not.toBeInTheDocument();
    open();
    expect(row("Favorites")).toBeChecked();
  });

  it("keeps the menu open across multi-select date picks", () => {
    renderMenu();
    open();

    fireEvent.click(row("1 week ago", section("Created date")));
    fireEvent.click(row("2 weeks ago", section("Created date")));

    expect(onFiltersChange).toHaveBeenLastCalledWith({
      ...EMPTY_VIEW_LIST_FILTERS,
      created: { presets: ["1_week_ago", "2_weeks_ago"], custom: null },
    });
    expect(panel()).toBeInTheDocument();
  });

  it("reveals the custom window from the Custom row and applies a picked range", () => {
    renderMenu();
    open();
    expect(screen.queryByTestId("project-views-custom-window")).not.toBeInTheDocument();

    fireEvent.click(row("Custom", section("Created date")));
    expect(screen.getByTestId("project-views-custom-window")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Custom creation window" }));
    const days = within(screen.getByTestId("project-views-custom-date-calendar-grid")).getAllByRole("button");
    const inMonth = days.filter((day) => day.getAttribute("data-in-month") === "true");
    fireEvent.click(inMonth[0] as HTMLElement);
    fireEvent.click(inMonth[inMonth.length - 1] as HTMLElement);

    const applied = onFiltersChange.mock.calls.at(-1)?.[0] as ViewListFilters;
    expect(applied.created.custom).not.toBeNull();
    expect(applied.created.custom?.start).not.toBeNull();
    expect(applied.created.custom?.end).not.toBeNull();
    expect(screen.queryByTestId("project-views-custom-window")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-views-filter-panel")).not.toBeInTheDocument();
  });

  it("waits for both ends before narrowing and stays open on a half pick", () => {
    renderMenu();
    open();
    fireEvent.click(row("Custom", section("Created date")));
    fireEvent.click(screen.getByRole("button", { name: "Custom creation window" }));
    const days = within(screen.getByTestId("project-views-custom-date-calendar-grid"))
      .getAllByRole("button")
      .filter((day) => day.getAttribute("data-in-month") === "true");

    fireEvent.click(days[0] as HTMLElement);

    expect(onFiltersChange).not.toHaveBeenCalled();
    expect(screen.getByTestId("project-views-custom-window")).toBeInTheDocument();
    expect(days[0]).toHaveAttribute("aria-pressed", "true");
  });

  it("abandons the custom window on a same-day double pick", () => {
    renderMenu();
    open();
    fireEvent.click(row("Custom", section("Created date")));
    fireEvent.click(screen.getByRole("button", { name: "Custom creation window" }));
    const day = within(screen.getByTestId("project-views-custom-date-calendar-grid"))
      .getAllByRole("button")
      .find(
        (entry) => entry.getAttribute("data-in-month") === "true" && !entry.hasAttribute("disabled")
      ) as HTMLElement;

    fireEvent.click(day);
    fireEvent.click(day);

    expect(onFiltersChange).toHaveBeenLastCalledWith(EMPTY_VIEW_LIST_FILTERS);
    expect(screen.queryByTestId("project-views-custom-window")).not.toBeInTheDocument();
  });

  it("clears the custom window when the Custom row is picked again", () => {
    renderMenu({
      ...EMPTY_VIEW_LIST_FILTERS,
      created: { presets: ["1_week_ago"], custom: { start: "2026-01-01", end: "2026-01-31" } },
    });
    open();

    fireEvent.click(row("Custom", section("Created date")));

    expect(onFiltersChange).toHaveBeenLastCalledWith({
      ...EMPTY_VIEW_LIST_FILTERS,
      created: { presets: ["1_week_ago"], custom: null },
    });
    expect(screen.queryByTestId("project-views-custom-window")).not.toBeInTheDocument();
  });

  it("narrows the options by its own search and says when nothing matches", () => {
    renderMenu();
    open();
    const search = within(panel()).getByPlaceholderText("Search");

    fireEvent.change(search, { target: { value: "zzz-no-such-option" } });

    expect(within(panel()).queryByText("Favorites")).not.toBeInTheDocument();
    expect(within(panel()).getAllByText("No matches found").length).toBeGreaterThan(0);

    fireEvent.click(within(panel()).getByRole("button", { name: "Clear filter search" }));
    expect(within(panel()).getByText("Favorites")).toBeInTheDocument();
  });

  it("narrows creator options by name and labels the caller You", () => {
    renderMenu();
    open();
    const creators = section("Created by");
    expect(row("You", creators)).toBeInTheDocument();
    expect(row("Ada Lovelace", creators)).toBeInTheDocument();

    fireEvent.click(row("You", creators));
    expect(onFiltersChange).toHaveBeenLastCalledWith({ ...EMPTY_VIEW_LIST_FILTERS, creators: ["user-1"] });

    fireEvent.change(within(panel()).getByPlaceholderText("Search"), { target: { value: "no such person" } });
    expect(section("Created by")).toHaveTextContent("No matches found");
  });

  it("offers the access dimension only where the edition opts in", () => {
    edition.flags["view-access-filter"] = true;
    renderMenu();
    open();

    const access = section("Access");
    expect(row("Private", access)).toBeInTheDocument();
    expect(row("Public", access)).toBeInTheDocument();

    fireEvent.click(row("Private", access));
    expect(onFiltersChange).toHaveBeenLastCalledWith({ ...EMPTY_VIEW_LIST_FILTERS, access: [0] });
  });
});
