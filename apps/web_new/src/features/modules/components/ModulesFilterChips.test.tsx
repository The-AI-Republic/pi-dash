// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The applied-filter chip strip (MOD-007): nothing renders while no filter
// is applied; each active dimension raises one chip in spec order with its
// stable key; a value's own control removes just that value; the chip's
// control clears the dimension; clear-all resets everything. The archived
// variant hides the favorites and status chips.

import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  emptyModuleFilters,
  toggleModuleDateToken,
  toggleModuleFilterValue,
  type ModuleListFilters,
} from "../lib/moduleListControls.js";
import { ModulesFilterChips } from "./ModulesFilterChips.js";

const ME = "423e4567-e89b-12d3-a456-426614174003";
const OTHER = "623e4567-e89b-12d3-a456-426614174005";

const MEMBERS = [
  {
    id: "membership-me",
    role: 15,
    member: {
      id: ME,
      first_name: "Rich",
      last_name: "Owner",
      avatar: "",
      avatar_url: null,
      display_name: "Rich Owner",
      is_bot: false,
    },
  },
  {
    id: "membership-other",
    role: 15,
    member: {
      id: OTHER,
      first_name: "Pat",
      last_name: "Member",
      avatar: "",
      avatar_url: "https://cdn.test/pat.png",
      display_name: "Pat Member",
      is_bot: false,
    },
  },
] as never;

function apply(filters: Partial<ModuleListFilters> = {}): ModuleListFilters {
  return { ...emptyModuleFilters(), ...filters };
}

function renderChips(
  filters: ModuleListFilters,
  props: { showFavorites?: boolean; showStatus?: boolean; onChange?: (next: ModuleListFilters) => void } = {}
) {
  const onChange = props.onChange ?? vi.fn();
  render(
    <ModulesFilterChips
      filters={filters}
      members={MEMBERS}
      meId={ME}
      onFiltersChange={onChange}
      {...(props.showFavorites === undefined ? null : { showFavorites: props.showFavorites })}
      {...(props.showStatus === undefined ? null : { showStatus: props.showStatus })}
    />
  );
  return onChange;
}

function chipFor(key: string): HTMLElement {
  const chip = screen.getAllByTestId("modules-filter-chip").find((node) => node.getAttribute("data-chip-key") === key);
  if (chip === undefined) throw new Error(`no chip ${key}`);
  return chip;
}

describe("ModulesFilterChips", () => {
  it("renders nothing while no dimension is applied", () => {
    const { container } = render(
      <ModulesFilterChips filters={emptyModuleFilters()} members={MEMBERS} meId={ME} onFiltersChange={vi.fn()} />
    );
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByTestId("modules-filter-chips")).not.toBeInTheDocument();
  });

  it("renders one chip per active dimension in spec order with its key", () => {
    let filters = apply({ favorites: true, status: ["completed"] });
    filters = toggleModuleFilterValue(filters, "lead", ME);
    filters = toggleModuleDateToken(filters, "due-date", { kind: "preset", value: "week-1" });
    renderChips(filters);
    expect(screen.getAllByTestId("modules-filter-chip").map((node) => node.getAttribute("data-chip-key"))).toEqual([
      "status",
      "lead",
      "target date",
      "Modules",
    ]);
    expect(within(chipFor("Modules")).getByTestId("modules-filter-chip-value")).toHaveTextContent("Favorite");
    expect(within(chipFor("status")).getByText("Completed")).toBeInTheDocument();
    expect(within(chipFor("lead")).getByText("You")).toBeInTheDocument();
    expect(within(chipFor("target date")).getByText("1 week from now")).toBeInTheDocument();
  });

  it("shows a status dot, a member avatar and a custom range label", () => {
    const filters = apply({
      status: ["planned"],
      members: [OTHER],
      "start-date": [{ kind: "range", value: { start: "2026-10-20", end: "2026-11-20" } }],
    });
    renderChips(filters);
    expect(within(chipFor("status")).getByTestId("modules-filter-chip-dot").getAttribute("style") ?? "").toContain(
      "rgb"
    );
    expect(within(chipFor("members")).getByText("Pat Member")).toBeInTheDocument();
    expect(within(chipFor("start date")).getByText("Oct 20 - Nov 20, 2026")).toBeInTheDocument();
  });

  it("removes one value without touching its siblings", () => {
    const onChange = renderChips(apply({ status: ["planned", "completed"] }));
    fireEvent.click(within(chipFor("status")).getByRole("button", { name: "Remove Planned from Status" }));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ status: ["completed"] }));
  });

  it("removes a member value and an undated fallback member", () => {
    const onChange = renderChips(apply({ members: [OTHER, "923e4567-e89b-12d3-a456-426614174099"] }));
    expect(within(chipFor("members")).getByText("Unknown member")).toBeInTheDocument();
    fireEvent.click(within(chipFor("members")).getByRole("button", { name: `Remove Pat Member from Members` }));
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ members: ["923e4567-e89b-12d3-a456-426614174099"] })
    );
  });

  it("clears the whole dimension through its own control", () => {
    const onChange = renderChips(apply({ status: ["planned", "completed"] }));
    fireEvent.click(within(chipFor("status")).getByRole("button", { name: "Clear Status filter" }));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ status: [] }));
  });

  it("clears every filter through clear-all", () => {
    const onChange = renderChips(apply({ favorites: true, status: ["planned"] }));
    fireEvent.click(screen.getByTestId("modules-filter-clear-all"));
    expect(onChange).toHaveBeenCalledWith(emptyModuleFilters());
  });

  it("hides the favorites and status chips on the archived variant", () => {
    renderChips(apply({ favorites: true, status: ["completed"], lead: [ME] }), {
      showFavorites: false,
      showStatus: false,
    });
    expect(screen.getAllByTestId("modules-filter-chip").map((node) => node.getAttribute("data-chip-key"))).toEqual([
      "lead",
    ]);
  });
});
