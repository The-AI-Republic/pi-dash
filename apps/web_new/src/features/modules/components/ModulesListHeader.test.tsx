// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list header (MOD-001/011): the project crumb plus a trailing
// Modules crumb, the slot controls in the cluster, and the layout switcher
// on the trailing edge.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerHarness.navigate,
}));

const viewportHarness = vi.hoisted(() => ({ narrow: false }));

vi.mock("../../../shared/shell/index.js", () => ({
  useNarrowViewport: () => viewportHarness.narrow,
}));

import { ModulesListHeader } from "./ModulesListHeader.js";

afterEach(() => {
  viewportHarness.narrow = false;
});

describe("ModulesListHeader", () => {
  it("renders the project name and a trailing Modules crumb", () => {
    render(<ModulesListHeader projectName="Website relaunch" layout="list" onLayoutChange={vi.fn()} />);
    const trail = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(trail).toHaveTextContent("Website relaunch");
    const crumbs = trail.querySelectorAll("li");
    expect(crumbs).toHaveLength(2);
    expect(crumbs[1]).toHaveTextContent("Modules");
    // The list is the modules level, so the trailing crumb is not a link.
    expect(trail.querySelectorAll("a")).toHaveLength(0);
  });

  it("mounts every control slot in the cluster", () => {
    render(
      <ModulesListHeader
        projectName="Website relaunch"
        layout="list"
        onLayoutChange={vi.fn()}
        createControl={<button type="button">Add Module</button>}
        searchControl={<button type="button">Search</button>}
        orderControl={<button type="button">Name</button>}
        filterControl={<button type="button">Filters</button>}
      />
    );
    const cluster = screen.getByTestId("modules-list-controls");
    for (const name of ["Search", "Name", "Filters"]) {
      expect(cluster).toContainElement(screen.getByRole("button", { name }));
    }
    expect(cluster).toContainElement(screen.getByRole("group", { name: "Module layout" }));
    expect(screen.getByRole("button", { name: "Add Module" })).toBeInTheDocument();
  });

  it("switches layout through the switcher", () => {
    const onLayoutChange = vi.fn();
    render(<ModulesListHeader projectName="Website relaunch" layout="list" onLayoutChange={onLayoutChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Board layout" }));
    expect(onLayoutChange).toHaveBeenCalledWith("board");
  });

  it("keeps the compact switcher on small screens", () => {
    viewportHarness.narrow = true;
    render(<ModulesListHeader projectName="Website relaunch" layout="list" onLayoutChange={vi.fn()} />);
    expect(screen.queryByRole("group", { name: "Module layout" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Change module layout, current List" })).toBeInTheDocument();
  });
});
