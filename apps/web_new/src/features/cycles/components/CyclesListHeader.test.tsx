// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The cycles header shows the creation action to members and admins
// only, and docks the search/filter controls below once provided
// (CYC-014, CYC-021).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CyclesListHeader } from "./CyclesListHeader.js";

describe("CyclesListHeader", () => {
  it("offers creation to members and admins", () => {
    const onCreateCycle = vi.fn();
    render(<CyclesListHeader canCreate onCreateCycle={onCreateCycle} />);
    expect(screen.getByTestId("cycles-list-header")).toBeInTheDocument();
    const action = screen.getByRole("button", { name: "Add cycle" });
    fireEvent.click(action);
    expect(onCreateCycle).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("cycles-list-controls")).not.toBeInTheDocument();
  });

  it("renders no creation affordance for guests and viewers", () => {
    render(<CyclesListHeader canCreate={false} onCreateCycle={() => undefined} />);
    expect(screen.getByTestId("cycles-list-header")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add cycle" })).not.toBeInTheDocument();
  });

  it("docks the search and filter controls below the action row", () => {
    render(
      <CyclesListHeader
        canCreate
        searchControl={<div data-testid="search-slot">search</div>}
        filterControl={<div data-testid="filter-slot">filters</div>}
      />
    );
    expect(screen.getByTestId("cycles-list-controls")).toBeInTheDocument();
    expect(screen.getByTestId("search-slot")).toBeInTheDocument();
    expect(screen.getByTestId("filter-slot")).toBeInTheDocument();
  });
});
