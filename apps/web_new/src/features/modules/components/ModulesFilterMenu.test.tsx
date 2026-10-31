// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules filters menu (MOD-006): the five/six groups in order, the
// You-first five-then-view-all people groups, the menu-only option search,
// the six statuses with dots, relative date lower bounds, the custom range
// dialog adding two tokens, the Custom toggle removing only the range, and
// the archived variant hiding favorites and status.

import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../issues/dropdowns/weekStart.js", () => ({ useWeekStart: () => 0 }));

import { emptyModuleFilters, toggleModuleFilterValue, type ModuleListFilters } from "../lib/moduleListControls.js";
import { ModulesFilterMenu } from "./ModulesFilterMenu.js";

const ME = "423e4567-e89b-12d3-a456-426614174003";

function person(id: string, display: string, role = 15, extra: Record<string, unknown> = {}) {
  return {
    id: `membership-${id}`,
    role,
    member: {
      id,
      first_name: display,
      last_name: "",
      avatar: "",
      avatar_url: null,
      display_name: display,
      is_bot: false,
      ...extra,
    },
  };
}

const MEMBERS = [
  person(ME, "Rich Owner"),
  person("123e4567-e89b-12d3-a456-426614174101", "Amy Adams"),
  person("123e4567-e89b-12d3-a456-426614174102", "Ben Brown"),
  person("123e4567-e89b-12d3-a456-426614174103", "Cara Cook"),
  person("123e4567-e89b-12d3-a456-426614174104", "Dan Diaz"),
  person("123e4567-e89b-12d3-a456-426614174105", "Eve Ellis"),
  person("123e4567-e89b-12d3-a456-426614174106", "Fay Ford"),
  person("123e4567-e89b-12d3-a456-426614174107", "Guest Nine", 5),
];

beforeEach(() => {
  vi.setSystemTime(new Date(2026, 9, 20, 12, 0, 0));
});

interface HostProps {
  filters?: ModuleListFilters;
  showFavorites?: boolean;
  showStatus?: boolean;
  open?: boolean;
  onChange?: (filters: ModuleListFilters) => void;
}

function renderMenu({
  filters = emptyModuleFilters(),
  showFavorites,
  showStatus,
  open,
  onChange = vi.fn(),
}: HostProps = {}) {
  const view = render(
    <ModulesFilterMenu
      filters={filters}
      members={MEMBERS}
      meId={ME}
      onFiltersChange={onChange}
      showFavorites={showFavorites}
      showStatus={showStatus}
      open={open}
    />
  );
  return view;
}

function group(title: string): HTMLElement {
  return screen
    .getAllByTestId("modules-filter-group")
    .find((node) => node.getAttribute("data-filter-group") === title)!;
}

describe("ModulesFilterMenu", () => {
  it("opens from the trigger and lists the groups in spec order", () => {
    renderMenu();
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const panel = screen.getByTestId("modules-filter-panel");
    expect(within(panel).getByTestId("modules-filter-favorites")).toHaveTextContent("Favorites");
    expect(within(panel).getByTestId("modules-filter-status")).toBeInTheDocument();
    expect(screen.getAllByTestId("modules-filter-group").map((node) => node.getAttribute("data-filter-group"))).toEqual(
      ["Status", "Lead", "Members", "Start date", "Due date"]
    );
  });

  it("shows six statuses with their dots and narrows by the menu search only", () => {
    renderMenu();
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const status = screen.getByTestId("modules-filter-status");
    expect(
      within(status)
        .getAllByRole("checkbox")
        .map((node) => node.getAttribute("aria-label"))
    ).toEqual(["Backlog", "Planned", "In Progress", "Paused", "Completed", "Cancelled"]);
    expect(
      within(status)
        .getAllByTestId("modules-status-dot")
        .map((dot) => dot.getAttribute("data-status"))
    ).toEqual(["backlog", "planned", "in-progress", "paused", "completed", "cancelled"]);
    const search = screen.getByRole("textbox", { name: "Search filters" });
    fireEvent.change(search, { target: { value: "cancel" } });
    expect(
      within(status)
        .getAllByRole("checkbox")
        .map((node) => node.getAttribute("aria-label"))
    ).toEqual(["Cancelled"]);
    expect(within(status).getAllByRole("checkbox")[0]).toHaveTextContent("Cancelled");
    expect(within(group("Lead")).queryAllByRole("checkbox")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Clear filter search" }));
    expect(within(status).getAllByRole("checkbox")).toHaveLength(6);
  });

  it("toggles favorites and status values through the change callback", () => {
    const onChange = vi.fn();
    renderMenu({ onChange });
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    fireEvent.click(within(screen.getByTestId("modules-filter-favorites")).getByRole("checkbox"));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ favorites: true }));
    fireEvent.click(within(screen.getByTestId("modules-filter-status")).getByRole("checkbox", { name: "Planned" }));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ status: ["planned"] }));
  });

  it("offers You first, five at a time, then view-all", () => {
    renderMenu();
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const lead = group("Lead");
    const labels = () =>
      within(lead)
        .getAllByRole("checkbox")
        .map((node) => node.getAttribute("aria-label"));
    expect(labels()).toEqual(["You", "Amy Adams", "Ben Brown", "Cara Cook", "Dan Diaz"]);
    fireEvent.click(within(lead).getByRole("button", { name: "View all" }));
    expect(labels()).toHaveLength(7);
    expect(within(lead).getByRole("button", { name: "View less" })).toBeInTheDocument();
    fireEvent.click(within(lead).getByRole("button", { name: "View less" }));
    expect(labels()).toHaveLength(5);
  });

  it("surfaces an applied count and selected rows", () => {
    const filters = toggleModuleFilterValue(emptyModuleFilters(), "lead", ME);
    renderMenu({ filters });
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const lead = group("Lead");
    expect(within(lead).getByTestId("modules-filter-dimension-count")).toHaveTextContent("1");
    expect(within(lead).getByRole("checkbox", { name: "You" })).toHaveAttribute("aria-checked", "true");
  });

  it("shows the relative date choices and adds a preset token", () => {
    const onChange = vi.fn();
    renderMenu({ onChange });
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const start = screen.getAllByTestId("modules-filter-date-group")[0]!;
    expect(
      within(start)
        .getAllByRole("checkbox")
        .map((node) => node.getAttribute("aria-label"))
    ).toEqual(["1 week from now", "2 weeks from now", "1 month from now", "2 months from now", "Custom"]);
    fireEvent.click(within(start).getByRole("checkbox", { name: "1 week from now" }));
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ "start-date": [{ kind: "preset", value: "week-1" }] })
    );
  });

  it("applies a custom range as two tokens and toggles it off alone", () => {
    const onChange = vi.fn();
    const { rerender } = renderMenu({ onChange });
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const start = () => screen.getAllByTestId("modules-filter-date-group")[0]!;
    fireEvent.click(within(start()).getByRole("checkbox", { name: "Custom" }));
    expect(screen.getByTestId("modules-custom-date")).toBeInTheDocument();
    const calendar = screen.getByTestId("modules-custom-calendar");
    // The dialog opens on today through the same day next month (§2.6.1),
    // and both ends are drawn as selected.
    expect(within(calendar).getByRole("button", { name: "October 20, 2026" })).toHaveAttribute("aria-pressed", "true");
    // The far end is next month, so only the month on screen is drawn.
    fireEvent.click(within(calendar).getByRole("button", { name: "Next month" }));
    expect(
      within(screen.getByTestId("modules-custom-calendar")).getByRole("button", { name: "November 20, 2026" })
    ).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByTestId("modules-custom-apply"));
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        "start-date": [{ kind: "range", value: { start: "2026-10-20", end: "2026-11-20" } }],
      })
    );
    expect(screen.queryByTestId("modules-custom-date")).not.toBeInTheDocument();

    const applied: ModuleListFilters = {
      ...emptyModuleFilters(),
      "start-date": [
        { kind: "preset", value: "week-1" },
        { kind: "range", value: { start: "2026-10-20", end: "2026-11-20" } },
      ],
    };
    rerender(<ModulesFilterMenu filters={applied} members={MEMBERS} meId={ME} onFiltersChange={onChange} open />);
    fireEvent.click(within(start()).getByRole("checkbox", { name: "Custom" }));
    expect(onChange).toHaveBeenLastCalledWith(
      expect.objectContaining({ "start-date": [{ kind: "preset", value: "week-1" }] })
    );
  });

  it("cancels a custom range without writing anything", () => {
    const onChange = vi.fn();
    renderMenu({ onChange });
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const start = screen.getAllByTestId("modules-filter-date-group")[0]!;
    fireEvent.click(within(start).getByRole("checkbox", { name: "Custom" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByTestId("modules-custom-date")).not.toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("picks a range and keeps the dialog open until Apply", () => {
    const onChange = vi.fn();
    renderMenu({ onChange });
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const start = screen.getAllByTestId("modules-filter-date-group")[0]!;
    fireEvent.click(within(start).getByRole("checkbox", { name: "Custom" }));
    const calendar = screen.getByTestId("modules-custom-calendar");
    // The first pick keeps the default start and moves the end.
    fireEvent.click(within(calendar).getByRole("button", { name: "October 25, 2026" }));
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByTestId("modules-custom-date")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("modules-custom-apply"));
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        "start-date": [{ kind: "range", value: { start: "2026-10-20", end: "2026-10-25" } }],
      })
    );
    expect(screen.queryByTestId("modules-custom-date")).not.toBeInTheDocument();
  });

  it("moves only the end when a later day is picked and keeps relative tokens", () => {
    const onChange = vi.fn();
    renderMenu({ onChange, filters: { ...emptyModuleFilters(), "start-date": [{ kind: "preset", value: "week-1" }] } });
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const start = screen.getAllByTestId("modules-filter-date-group")[0]!;
    fireEvent.click(within(start).getByRole("checkbox", { name: "Custom" }));
    fireEvent.click(
      within(screen.getByTestId("modules-custom-calendar")).getByRole("button", { name: "October 23, 2026" })
    );
    fireEvent.click(screen.getByTestId("modules-custom-apply"));
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        "start-date": [
          { kind: "preset", value: "week-1" },
          { kind: "range", value: { start: "2026-10-20", end: "2026-10-23" } },
        ],
      })
    );
  });

  it("reseeds the default window each time the dialog opens and keeps Cancel inert", () => {
    const onChange = vi.fn();
    renderMenu({ onChange });
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const start = screen.getAllByTestId("modules-filter-date-group")[0]!;
    fireEvent.click(within(start).getByRole("checkbox", { name: "Custom" }));
    const calendar = screen.getByTestId("modules-custom-calendar");
    fireEvent.click(within(calendar).getByRole("button", { name: "October 10, 2026" }));
    expect(within(calendar).getByRole("button", { name: "October 20, 2026" })).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onChange).not.toHaveBeenCalled();
    // Reopening drops the cancelled pick and shows the default again.
    fireEvent.click(within(start).getByRole("checkbox", { name: "Custom" }));
    const reopened = screen.getByTestId("modules-custom-calendar");
    expect(within(reopened).getByRole("button", { name: "October 20, 2026" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByTestId("modules-custom-apply"));
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        "start-date": [{ kind: "range", value: { start: "2026-10-20", end: "2026-11-20" } }],
      })
    );
  });

  it("marks the trigger while a filter is active", () => {
    renderMenu({ filters: toggleModuleFilterValue(emptyModuleFilters(), "favorites") });
    expect(screen.getByTestId("modules-filter-active")).toBeInTheDocument();
  });

  it("hides favorites and status on the archived variant", () => {
    renderMenu({ showFavorites: false, showStatus: false });
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    expect(screen.queryByTestId("modules-filter-favorites")).not.toBeInTheDocument();
    expect(screen.queryByTestId("modules-filter-status")).not.toBeInTheDocument();
    expect(screen.getAllByTestId("modules-filter-group")).toHaveLength(4);
  });

  it("drops the option search when the panel closes", () => {
    renderMenu({ open: true, onChange: vi.fn() });
    fireEvent.change(screen.getByRole("textbox", { name: "Search filters" }), { target: { value: "cancel" } });
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    expect(screen.getByRole("textbox", { name: "Search filters" })).toHaveValue("");
  });

  it("shows no-match text in a people group whose options all filter out", () => {
    renderMenu({ open: true });
    fireEvent.change(screen.getByRole("textbox", { name: "Search filters" }), { target: { value: "zzz" } });
    expect(within(group("Lead")).getByText("No matches found")).toBeInTheDocument();
  });

  it("drops status and favorites when the menu search excludes them", () => {
    renderMenu({ open: true });
    fireEvent.change(screen.getByRole("textbox", { name: "Search filters" }), { target: { value: "zzz" } });
    expect(screen.queryByTestId("modules-filter-favorites")).not.toBeInTheDocument();
    expect(screen.queryByTestId("modules-filter-status")).not.toBeInTheDocument();
    expect(screen.getAllByTestId("modules-filter-group").map((node) => node.getAttribute("data-filter-group"))).toEqual(
      ["Lead", "Members", "Start date", "Due date"]
    );
  });

  it("shows a member's avatar beside the name when the directory has one", () => {
    const avatarUrl = "https://cdn.example.test/avatars/zed.png";
    render(
      <ModulesFilterMenu
        filters={emptyModuleFilters()}
        members={[
          MEMBERS[0]!,
          person("123e4567-e89b-12d3-a456-426614174101", "Amy Adams", 15, { avatar_url: avatarUrl }),
        ]}
        meId={ME}
        onFiltersChange={vi.fn()}
        open
      />
    );
    const row = group("Lead").querySelector('[data-filter-option="Amy Adams"]')!;
    expect(row.querySelector("img")?.getAttribute("src")).toBe(avatarUrl);
    expect(group("Lead").querySelector('[data-filter-option="You"] img')).toBeNull();
  });

  it("reports panel open changes and lets the host control them", () => {
    const onOpenChange = vi.fn();
    const { rerender } = render(
      <ModulesFilterMenu
        filters={emptyModuleFilters()}
        members={MEMBERS}
        meId={ME}
        onFiltersChange={vi.fn()}
        open={false}
        onOpenChange={onOpenChange}
      />
    );
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    expect(screen.queryByTestId("modules-filter-panel")).not.toBeInTheDocument();
    rerender(
      <ModulesFilterMenu
        filters={emptyModuleFilters()}
        members={MEMBERS}
        meId={ME}
        onFiltersChange={vi.fn()}
        open
        onOpenChange={onOpenChange}
      />
    );
    expect(screen.getByTestId("modules-filter-panel")).toBeInTheDocument();
  });
});
