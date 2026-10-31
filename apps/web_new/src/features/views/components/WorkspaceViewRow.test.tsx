// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom row identity (parity/specs/views.md §9, VIEW-032).
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
import { WORKSPACE_VIEW_NAME_LIMIT } from "../lib/workspaceViews.js";
import { WorkspaceViewRow } from "./WorkspaceViewRow.js";

describe("WorkspaceViewRow", () => {
  it("links the whole identity cluster to the view's detail page", () => {
    render(<WorkspaceViewRow workspaceSlug="acme" view={makeView({ id: "view-9", name: "Triage" })} />);
    const row = screen.getByTestId("workspace-views-custom-row");
    expect(row).toHaveAttribute("data-view-id", "view-9");
    expect(screen.getByRole("link", { name: "Triage" })).toHaveAttribute("href", "/acme/workspace-views/view-9");
  });

  it("shows the description under the name when the record has one", () => {
    const { container } = render(
      <WorkspaceViewRow workspaceSlug="acme" view={makeView({ name: "Triage", description: "Row desc" })} />
    );
    expect(screen.getByRole("link", { name: /Triage/ })).toHaveTextContent("Row desc");
    expect(container.textContent).toContain("Row desc");
  });

  it("omits the description line entirely when it is blank", () => {
    const { container } = render(
      <WorkspaceViewRow workspaceSlug="acme" view={makeView({ name: "Triage", description: "   " })} />
    );
    const link = screen.getByRole("link", { name: "Triage" });
    expect(link.querySelectorAll("span")).toHaveLength(2);
    expect(container.textContent).not.toContain("\n");
  });

  it("caps a long name so the row stays readable", () => {
    const long = "n".repeat(100);
    render(<WorkspaceViewRow workspaceSlug="acme" view={makeView({ name: long })} />);
    const link = screen.getByRole("link", { name: new RegExp("^n+…") });
    expect(link).toHaveTextContent(`${"n".repeat(WORKSPACE_VIEW_NAME_LIMIT - 1)}…`);
  });

  it("keeps the menu outside the anchor so it is reachable on its own", async () => {
    const onEdit = vi.fn();
    const onDelete = vi.fn();
    render(
      <WorkspaceViewRow workspaceSlug="acme" view={makeView({ name: "Triage" })} onEdit={onEdit} onDelete={onDelete} />
    );
    const trigger = screen.getByRole("button", { name: "Actions for Triage" });
    expect(trigger.closest("a")).toBeNull();
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit View" }));
    expect(onEdit).toHaveBeenCalledTimes(1);
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete View" }));
    expect(onDelete).toHaveBeenCalledTimes(1);
  });

  it("renders an inert menu while no handlers are wired", async () => {
    render(<WorkspaceViewRow workspaceSlug="acme" view={makeView({ name: "Triage" })} />);
    fireEvent.click(screen.getByRole("button", { name: "Actions for Triage" }));
    await waitFor(() => expect(screen.getAllByRole("menuitem")).toHaveLength(2));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit View" }));
    fireEvent.click(screen.getByRole("button", { name: "Actions for Triage" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete View" }));
  });
});
