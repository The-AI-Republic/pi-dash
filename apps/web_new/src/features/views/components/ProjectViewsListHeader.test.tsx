// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ProjectViewsListHeader } from "./ProjectViewsListHeader.js";

describe("ProjectViewsListHeader", () => {
  it("trails the project with a Views crumb and offers Add view", () => {
    const onCreateView = vi.fn();
    render(<ProjectViewsListHeader workspaceName="Acme" projectName="Website relaunch" onCreateView={onCreateView} />);
    const trail = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(trail).toHaveTextContent("Acme");
    expect(trail).toHaveTextContent("Website relaunch");
    const crumbs = trail.querySelectorAll("li");
    expect(crumbs[crumbs.length - 1]).toHaveTextContent("Views");
    const add = screen.getByRole("button", { name: "Add view" });
    expect(add).toBeEnabled();
    fireEvent.click(add);
    expect(onCreateView).toHaveBeenCalledTimes(1);
  });

  it("stays enabled without a dialog opener until NEWFRONT-721 lands", () => {
    render(<ProjectViewsListHeader workspaceName="Acme" projectName="Website relaunch" />);
    expect(screen.getByRole("button", { name: "Add view" })).toBeEnabled();
  });

  it("mounts the NEWFRONT-720 control slots when provided", () => {
    render(
      <ProjectViewsListHeader
        workspaceName="Acme"
        projectName="Website relaunch"
        searchControl={<span data-testid="slot-search" />}
        sortControl={<span data-testid="slot-sort" />}
        filterControl={<span data-testid="slot-filter" />}
      />
    );
    expect(screen.getByTestId("project-views-controls")).toBeInTheDocument();
    expect(screen.getByTestId("slot-search")).toBeInTheDocument();
    expect(screen.getByTestId("slot-sort")).toBeInTheDocument();
    expect(screen.getByTestId("slot-filter")).toBeInTheDocument();
  });

  it("omits the control row while every slot is empty", () => {
    render(<ProjectViewsListHeader workspaceName="Acme" projectName="Website relaunch" />);
    expect(screen.queryByTestId("project-views-controls")).not.toBeInTheDocument();
  });
});
