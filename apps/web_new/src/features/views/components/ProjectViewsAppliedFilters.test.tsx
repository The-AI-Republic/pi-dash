// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { makeMember } from "../testing.js";
import { EMPTY_VIEW_LIST_FILTERS, type ViewListFilters } from "../lib/viewListControls.js";
import { ProjectViewsAppliedFilters } from "./ProjectViewsAppliedFilters.js";

const MEMBERS = [makeMember("user-1"), makeMember("user-2")];

function filters(overrides: Partial<ViewListFilters> = {}): ViewListFilters {
  return { ...EMPTY_VIEW_LIST_FILTERS, ...overrides };
}

function renderStrip(value: ViewListFilters, onFiltersChange: (next: ViewListFilters) => void = vi.fn()) {
  render(
    <ProjectViewsAppliedFilters filters={value} members={MEMBERS} selfId="user-1" onFiltersChange={onFiltersChange} />
  );
  return onFiltersChange;
}

describe("ProjectViewsAppliedFilters", () => {
  it("renders nothing while no filter is active", () => {
    renderStrip(EMPTY_VIEW_LIST_FILTERS);
    expect(screen.queryByTestId("project-views-applied-filters")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Clear all" })).not.toBeInTheDocument();
  });

  it("lists one group per dimension and the selected values", () => {
    renderStrip(
      filters({ favorites: true, created: { presets: ["1_month_ago"], custom: null }, creators: ["user-2"] })
    );
    expect(screen.getByText("Favorites")).toBeInTheDocument();
    expect(screen.getByText("Created at")).toBeInTheDocument();
    expect(screen.getByText("1 month ago")).toBeInTheDocument();
    expect(screen.getByText("Created by")).toBeInTheDocument();
    expect(screen.getByText("Ada Lovelace")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear all" })).toBeInTheDocument();
    expect(document.querySelectorAll("[data-chip-dimension]")).toHaveLength(3);
  });

  it("removes one value and keeps every sibling dimension", () => {
    const onFiltersChange = vi.fn();
    const value = filters({ favorites: true, created: { presets: ["1_week_ago", "1_month_ago"], custom: null } });
    renderStrip(value, onFiltersChange);

    fireEvent.click(screen.getByRole("button", { name: "Remove 1 week ago" }));

    expect(onFiltersChange).toHaveBeenCalledWith({ ...value, created: { presets: ["1_month_ago"], custom: null } });
  });

  it("removes a dimension without touching its siblings", () => {
    const onFiltersChange = vi.fn();
    const value = filters({ favorites: true, creators: ["user-2"] });
    renderStrip(value, onFiltersChange);

    fireEvent.click(screen.getByRole("button", { name: "Clear Favorites filters" }));

    expect(onFiltersChange).toHaveBeenCalledWith({ ...value, favorites: false });
  });

  it("clears every dimension from Clear all", () => {
    const onFiltersChange = vi.fn();
    renderStrip(filters({ favorites: true, access: [1], creators: ["user-2"] }), onFiltersChange);

    fireEvent.click(screen.getByRole("button", { name: "Clear all" }));

    expect(onFiltersChange).toHaveBeenCalledWith(EMPTY_VIEW_LIST_FILTERS);
  });

  it("shows a custom window chip that clears only the window", () => {
    const onFiltersChange = vi.fn();
    const value = filters({ created: { presets: [], custom: { start: "2026-01-01", end: "2026-01-31" } } });
    renderStrip(value, onFiltersChange);

    fireEvent.click(screen.getByRole("button", { name: "Remove Custom" }));

    expect(onFiltersChange).toHaveBeenCalledWith({ ...value, created: { presets: [], custom: null } });
  });

  it("hides once the last dimension is removed by its own control", () => {
    function Host(): React.ReactElement {
      const [value, setValue] = React.useState<ViewListFilters>(filters({ creators: ["user-2"] }));
      return <ProjectViewsAppliedFilters filters={value} members={MEMBERS} selfId={null} onFiltersChange={setValue} />;
    }
    render(<Host />);
    fireEvent.click(screen.getByRole("button", { name: "Clear Created by filters" }));
    expect(screen.queryByTestId("project-views-applied-filters")).not.toBeInTheDocument();
  });
});
