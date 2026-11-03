// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Restriction and denial bodies (PSET-003/PSET-004): one view per denial
// kind, the join action and its pending label, and the refusal body kept
// inside the shell.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  ProjectSettingsNotAuthorized,
  ProjectSettingsRestriction,
  RESTRICTION_JOIN_BODY,
  RESTRICTION_JOIN_TITLE,
  RESTRICTION_NO_ACCESS_BODY,
  RESTRICTION_NO_ACCESS_TITLE,
  RESTRICTION_NOT_FOUND_BODY,
  RESTRICTION_NOT_FOUND_TITLE,
} from "./ProjectSettingsRestriction.js";

describe("ProjectSettingsRestriction", () => {
  it("asks non-members to join and runs the action", () => {
    const onJoin = vi.fn();
    render(<ProjectSettingsRestriction kind="join" onJoin={onJoin} />);
    expect(screen.getByRole("heading", { name: RESTRICTION_JOIN_TITLE })).toBeInTheDocument();
    expect(screen.getByText(RESTRICTION_JOIN_BODY)).toBeInTheDocument();
    expect(screen.getByTestId("project-settings-restriction")).toHaveAttribute("data-restriction", "join");
    expect(screen.getByTestId("project-settings-join")).toHaveTextContent("Join project");
    fireEvent.click(screen.getByRole("button", { name: "Join project" }));
    expect(onJoin).toHaveBeenCalledTimes(1);
  });

  it("disables the join action while joining", () => {
    render(<ProjectSettingsRestriction kind="join" joining onJoin={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Joining…" })).toBeDisabled();
  });

  it("allows a join view with no action wired", () => {
    render(<ProjectSettingsRestriction kind="join" />);
    expect(screen.getByRole("button", { name: "Join project" })).toBeEnabled();
  });

  it("explains a refusal without an action", () => {
    render(<ProjectSettingsRestriction kind="no-access" />);
    expect(screen.getByTestId("project-settings-restriction")).toHaveAttribute("data-restriction", "no-access");
    expect(screen.getByRole("heading", { name: RESTRICTION_NO_ACCESS_TITLE })).toBeInTheDocument();
    expect(screen.getByText(RESTRICTION_NO_ACCESS_BODY)).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-join")).not.toBeInTheDocument();
  });

  it("reports an unknown project without a join action", () => {
    render(<ProjectSettingsRestriction kind="not-found" />);
    expect(screen.getByTestId("project-settings-restriction")).toHaveAttribute("data-restriction", "not-found");
    expect(screen.getByRole("heading", { name: RESTRICTION_NOT_FOUND_TITLE })).toBeInTheDocument();
    expect(screen.getByText(RESTRICTION_NOT_FOUND_BODY)).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});

describe("ProjectSettingsNotAuthorized", () => {
  it("names the refused tab", () => {
    render(<ProjectSettingsNotAuthorized heading="Repository" />);
    expect(screen.getByTestId("project-settings-denied")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Not authorized" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Not authorized to view repository" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Repository" })).toBeInTheDocument();
  });
});
