// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { WORKSPACE_DEFAULT_VIEWS } from "../lib/workspaceViews.js";
import { makeView } from "../testing.js";
import { WorkspaceViewHeader } from "./WorkspaceViewHeader.js";

const CUSTOM = "223e4567-e89b-12d3-a456-426614174001";
const DEFAULT = WORKSPACE_DEFAULT_VIEWS[0];

if (!DEFAULT) throw new Error("missing all-issues default");

function renderHeader(overrides: Partial<React.ComponentProps<typeof WorkspaceViewHeader>> = {}) {
  const onOpenView = vi.fn();
  const onAddView = vi.fn();
  const onEdit = vi.fn();
  const onDelete = vi.fn();
  render(
    <WorkspaceViewHeader
      workspaceSlug="acme"
      defaultView={null}
      view={makeView({ id: CUSTOM, name: "Triage" })}
      detailHref={`/acme/workspace-views/${CUSTOM}`}
      customs={[makeView({ id: CUSTOM, name: "Triage" })]}
      customsLoading={false}
      customsFailed={false}
      standing={{ isOwner: true, canDelete: true }}
      layoutControl={<span data-testid="layout-control" />}
      filterControl={<span data-testid="filter-control" />}
      displayControl={<span data-testid="display-control" />}
      canAddView
      onOpenView={onOpenView}
      onAddView={onAddView}
      onEdit={onEdit}
      onDelete={onDelete}
      {...overrides}
    />
  );
  return { onOpenView, onAddView, onEdit, onDelete };
}

describe("WorkspaceViewHeader", () => {
  it("reads the Views crumb and the current custom", () => {
    renderHeader();
    const trail = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(within(trail).getByRole("link", { name: "Views" })).toHaveAttribute("href", "/acme/workspace-views");
    expect(within(trail).getByRole("button", { name: "Triage" })).toHaveTextContent("Triage");
  });

  it("renders the switcher unpositioned while the custom loads", () => {
    renderHeader({ view: null });
    const trail = screen.getByRole("navigation", { name: "Breadcrumb" });
    const [trigger] = within(trail).getAllByRole("button");
    expect(trigger).toHaveTextContent("");
  });

  it("reads the current default", () => {
    renderHeader({ defaultView: DEFAULT, view: null });
    const trail = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(within(trail).getByRole("button", { name: "All work items" })).toBeInTheDocument();
  });

  it("orders layout, filter, display, add and menu on the right", () => {
    renderHeader();
    const header = screen.getByTestId("workspace-view-header");
    const order = ["layout-control", "filter-control", "display-control", "workspace-view-add"].map((id) =>
      screen.getByTestId(id)
    );
    const positions = order.map((node) => header.innerHTML.indexOf(node.outerHTML));
    expect([...positions].sort((a, b) => a - b)).toEqual(positions);
    expect(screen.getByTestId("workspace-view-menu-trigger")).toBeInTheDocument();
  });

  it("adds a view from the header action", () => {
    const { onAddView } = renderHeader();
    fireEvent.click(screen.getByTestId("workspace-view-add"));
    expect(onAddView).toHaveBeenCalledTimes(1);
  });

  it("hides the add action from guests", () => {
    renderHeader({ canAddView: false });
    expect(screen.queryByTestId("workspace-view-add")).toBeNull();
  });

  it("navigates on a switcher pick", () => {
    const { onOpenView } = renderHeader();
    fireEvent.click(screen.getByRole("button", { name: "Triage" }));
    fireEvent.click(screen.getByRole("option", { name: "Assigned" }));
    expect(onOpenView).toHaveBeenCalledWith("assigned");
  });
});
