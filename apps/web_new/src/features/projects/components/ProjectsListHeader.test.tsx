// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ProjectsListHeader } from "./ProjectsListHeader.js";

function controls() {
  return {
    searchControl: <input aria-label="Search projects" />,
    sortControl: <button type="button">Sort: Name</button>,
    filterControl: <button type="button">Filters</button>,
  };
}

describe("ProjectsListHeader", () => {
  it("shows the Projects crumb with a working creation button for editors", () => {
    const onCreate = vi.fn();
    render(<ProjectsListHeader archived={false} canCreate={true} onCreate={onCreate} />);

    expect(screen.getByText("Projects")).not.toBeNull();
    expect(screen.queryByText("Archived")).toBeNull();
    const create = screen.getByRole("button", { name: /Project/ });
    fireEvent.click(create);
    expect(onCreate).toHaveBeenCalledOnce();
  });

  it("mounts both the full and the short creation label for the responsive switch", () => {
    render(<ProjectsListHeader archived={false} canCreate={true} onCreate={() => {}} />);

    const create = screen.getByRole("button", { name: /Project/ });
    expect(create.querySelector(".hidden.sm\\:inline")).toHaveTextContent("Add Project");
    expect(create.querySelector(".sm\\:hidden")).toHaveTextContent("Project");
  });

  it("adds the Archived crumb and hides the creation button in the archive view", () => {
    render(<ProjectsListHeader archived={true} canCreate={true} onCreate={() => {}} {...controls()} />);

    expect(screen.getByText("Archived")).not.toBeNull();
    expect(screen.queryByRole("button", { name: /Project/ })).toBeNull();
  });

  it("hides the creation button from guests", () => {
    render(<ProjectsListHeader archived={false} canCreate={false} onCreate={() => {}} {...controls()} />);

    expect(screen.getByText("Projects")).not.toBeNull();
    expect(screen.queryByRole("button", { name: /Project/ })).toBeNull();
  });

  it("hides the creation button from guests in the archive view too", () => {
    render(<ProjectsListHeader archived={true} canCreate={false} onCreate={() => {}} />);

    expect(screen.getByText("Archived")).not.toBeNull();
    expect(screen.queryByRole("button", { name: /Project/ })).toBeNull();
  });

  it("renders every list crumb as text, never a link", () => {
    render(<ProjectsListHeader archived={true} canCreate={true} onCreate={() => {}} />);

    expect(screen.queryByRole("link")).toBeNull();
  });

  it("mounts the control slots in the desktop filter row", () => {
    render(<ProjectsListHeader archived={false} canCreate={true} onCreate={() => {}} {...controls()} />);

    const row = screen.getByTestId("projects-desktop-filters");
    expect(row.className).toContain("hidden");
    expect(row.className).toContain("md:flex");
    expect(row.querySelector('input[aria-label="Search projects"]')).not.toBeNull();
    expect(row.querySelector("button")).not.toBeNull();
  });

  it("mounts sort and filter in the touch two-pane bar below medium", () => {
    render(<ProjectsListHeader archived={false} canCreate={true} onCreate={() => {}} {...controls()} />);

    const bar = screen.getByTestId("projects-mobile-bar");
    expect(bar.className).toContain("grid-cols-2");
    expect(bar.className).toContain("md:hidden");
    const buttons = Array.from(bar.querySelectorAll("button")).map((button) => button.textContent);
    expect(buttons).toEqual(["Sort: Name", "Filters"]);
    expect(bar.querySelector("input")).toBeNull();
  });

  it("renders the responsive shells even before the controls land", () => {
    render(<ProjectsListHeader archived={false} canCreate={true} onCreate={() => {}} />);

    expect(screen.getByTestId("projects-desktop-filters")).not.toBeNull();
    expect(screen.getByTestId("projects-mobile-bar")).not.toBeNull();
  });
});
