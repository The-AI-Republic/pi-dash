// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom row overflow menu (parity/specs/views.md §9, VIEW-032).
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { WorkspaceViewRowMenu } from "./WorkspaceViewRowMenu.js";

describe("WorkspaceViewRowMenu", () => {
  it("names its trigger after the row it acts on", () => {
    render(<WorkspaceViewRowMenu viewName="Triage" onEdit={vi.fn()} onDelete={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Actions for Triage" })).toBeInTheDocument();
  });

  it("offers Edit View and Delete View in that order", async () => {
    render(<WorkspaceViewRowMenu viewName="Triage" onEdit={vi.fn()} onDelete={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Actions for Triage" }));
    await waitFor(() => expect(screen.getAllByRole("menuitem")).toHaveLength(2));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Edit View", "Delete View"]);
  });

  it("runs the edit handler", async () => {
    const onEdit = vi.fn();
    render(<WorkspaceViewRowMenu viewName="Triage" onEdit={onEdit} onDelete={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Actions for Triage" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit View" }));
    expect(onEdit).toHaveBeenCalledTimes(1);
  });

  it("runs the delete handler", async () => {
    const onDelete = vi.fn();
    render(<WorkspaceViewRowMenu viewName="Triage" onEdit={vi.fn()} onDelete={onDelete} />);
    fireEvent.click(screen.getByRole("button", { name: "Actions for Triage" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete View" }));
    expect(onDelete).toHaveBeenCalledTimes(1);
  });
});
