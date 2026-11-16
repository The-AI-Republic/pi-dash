// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The one card every issues empty state renders (parity/specs/issues.md
// §8): context stamp, heading, description, and at most two entries whose
// disabled state follows the caller's standing.
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { EmptyContextCard } from "./EmptyContextCard.js";

describe("EmptyContextCard", () => {
  it("renders a stamped card without actions", () => {
    render(
      <EmptyContextCard
        context="profile-assigned"
        title="Nothing assigned"
        description="Work items assigned to you show here."
        artwork={<span data-testid="art" />}
      />
    );
    const card = screen.getByTestId("issues-empty-state");
    expect(card).toHaveAttribute("data-context", "profile-assigned");
    expect(screen.getByTestId("art")).toBeInTheDocument();
    expect(screen.getByText("Nothing assigned")).toBeInTheDocument();
    expect(screen.getByText("Work items assigned to you show here.")).toBeInTheDocument();
    expect(within(card).queryAllByRole("button")).toHaveLength(0);
  });

  it("renders the primary and secondary entries and calls them", () => {
    const primary = vi.fn();
    const secondary = vi.fn();
    render(
      <EmptyContextCard
        context="cycle"
        title="Empty"
        description="Body"
        artwork={null}
        actions={[
          { label: "Create work item", onSelect: primary },
          { label: "Add existing work item", onSelect: secondary },
        ]}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Create work item" }));
    fireEvent.click(screen.getByRole("button", { name: "Add existing work item" }));
    expect(primary).toHaveBeenCalledTimes(1);
    expect(secondary).toHaveBeenCalledTimes(1);
  });

  it("keeps disabled entries visible with a dead control", () => {
    const onSelect = vi.fn();
    render(
      <EmptyContextCard
        context="global"
        title="No project"
        description="Body"
        artwork={<span />}
        actions={[{ label: "Start your first project", onSelect, disabled: true }]}
      />
    );
    const action = screen.getByRole("button", { name: "Start your first project" });
    expect(action).toBeDisabled();
    fireEvent.click(action);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("accepts an action whose disabled flag is omitted", () => {
    render(
      <EmptyContextCard
        context="project"
        title="Start"
        description="Body"
        artwork={<span />}
        actions={[{ label: "Create your first work item", onSelect: () => {} }]}
      />
    );
    expect(screen.getByRole("button", { name: "Create your first work item" })).toBeEnabled();
  });
});
