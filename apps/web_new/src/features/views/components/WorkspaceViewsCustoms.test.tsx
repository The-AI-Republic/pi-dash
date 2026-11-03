// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Customs half of the workspace list (parity/specs/views.md §9, VIEW-039).
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
}));

import { makeView } from "../testing.js";
import { WorkspaceViewsCustoms } from "./WorkspaceViewsCustoms.js";

const BASE = {
  views: [],
  searchHidesRows: false,
  workspaceSlug: "acme",
  onRetry: () => undefined,
} as const;

describe("WorkspaceViewsCustoms", () => {
  it("renders the skeleton while loading", () => {
    render(<WorkspaceViewsCustoms {...BASE} state="loading" />);
    expect(screen.getByTestId("project-views-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-views-customs")).not.toBeInTheDocument();
  });

  it("renders the inline error with a working retry", () => {
    const onRetry = vi.fn();
    render(<WorkspaceViewsCustoms {...BASE} state="error" onRetry={onRetry} />);
    expect(screen.getByTestId("project-views-error")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("renders nothing at all when the workspace has no customs", () => {
    const { container } = render(<WorkspaceViewsCustoms {...BASE} state="rows" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders one row per visible custom", () => {
    render(
      <WorkspaceViewsCustoms
        {...BASE}
        state="rows"
        views={[makeView({ id: "a", name: "Alpha" }), makeView({ id: "b", name: "Beta" })]}
        searchHidesRows
      />
    );
    const rowIds = screen.getAllByTestId("workspace-views-custom-row").map((row) => row.getAttribute("data-view-id"));
    expect(rowIds).toEqual(["a", "b"]);
    expect(screen.queryByTestId("workspace-views-customs-no-match")).not.toBeInTheDocument();
  });

  it("says so quietly when the search hid every custom", () => {
    render(
      <WorkspaceViewsCustoms {...BASE} state="rows" views={[]} searchHidesRows onEdit={vi.fn()} onDelete={vi.fn()} />
    );
    expect(screen.getByTestId("workspace-views-customs-no-match")).toHaveTextContent("No views match your search.");
  });

  it("passes the row's record to the edit and delete handlers", async () => {
    const onEdit = vi.fn();
    const onDelete = vi.fn();
    const view = makeView({ id: "a", name: "Alpha" });
    render(<WorkspaceViewsCustoms {...BASE} state="rows" views={[view]} onEdit={onEdit} onDelete={onDelete} />);
    fireEvent.click(screen.getByRole("button", { name: "Actions for Alpha" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit View" }));
    expect(onEdit).toHaveBeenCalledWith(view);
    fireEvent.click(screen.getByRole("button", { name: "Actions for Alpha" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete View" }));
    expect(onDelete).toHaveBeenCalledWith(view);
  });
});
