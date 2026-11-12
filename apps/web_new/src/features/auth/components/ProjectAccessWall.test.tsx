// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The shared project access wall (AUTH-024, MOD-038): one block for the
// three refusals, a join action only where the server offers one, and
// nothing that reads as a missing record.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ProjectAccessWall } from "./ProjectAccessWall.js";

describe("ProjectAccessWall", () => {
  it("offers the join action with the project's own wording", () => {
    const onJoin = vi.fn();
    render(<ProjectAccessWall kind="join" onJoin={onJoin} />);
    const wall = screen.getByTestId("project-access-wall");
    expect(wall).toHaveAttribute("data-wall-kind", "join");
    expect(screen.getByRole("heading", { name: "You haven't joined this project yet" })).toBeInTheDocument();
    expect(screen.getByText("Join it to see its work.")).toBeInTheDocument();
    const button = screen.getByRole("button", { name: "Join project" });
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(onJoin).toHaveBeenCalledTimes(1);
  });

  it("autofocuses the join action so it is the step's primary control", () => {
    render(<ProjectAccessWall kind="join" onJoin={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Join project" })).toHaveFocus();
  });

  it("disables the join action and says it is joining while in flight", () => {
    const onJoin = vi.fn();
    render(<ProjectAccessWall kind="join" onJoin={onJoin} joining />);
    const button = screen.getByRole("button", { name: "Joining…" });
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(onJoin).not.toHaveBeenCalled();
  });

  it("renders the wall without a control when the caller has no join action", () => {
    render(<ProjectAccessWall kind="join" />);
    expect(screen.getByRole("heading", { name: "You haven't joined this project yet" })).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("explains a private project to a reader who cannot join", () => {
    render(<ProjectAccessWall kind="no-access" onJoin={vi.fn()} />);
    expect(screen.getByTestId("project-access-wall")).toHaveAttribute("data-wall-kind", "no-access");
    expect(screen.getByRole("heading", { name: "You can't open this project" })).toBeInTheDocument();
    expect(
      screen.getByText("This project is private. Ask a workspace admin for access to continue.")
    ).toBeInTheDocument();
    // A refusal that offers no join must not grow a join button.
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("reports an address that names no project", () => {
    render(<ProjectAccessWall kind="not-found" />);
    expect(screen.getByTestId("project-access-wall")).toHaveAttribute("data-wall-kind", "not-found");
    expect(screen.getByRole("heading", { name: "Project not found" })).toBeInTheDocument();
    expect(screen.getByText("It may have been deleted, or the address is wrong.")).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
