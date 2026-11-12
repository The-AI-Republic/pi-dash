// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ProjectsEmptyState, type ProjectsEmptyVariant } from "./ProjectsEmptyState.js";

const VARIANTS: ProjectsEmptyVariant[] = ["first-run", "no-match", "empty-archive"];

describe("ProjectsEmptyState", () => {
  it("renders the first-run state with an enabled creation action for editors", () => {
    const onCreate = vi.fn();
    render(<ProjectsEmptyState variant="first-run" canCreate={true} onCreate={onCreate} />);

    expect(screen.getByRole("heading", { name: "No active projects" })).not.toBeNull();
    const action = screen.getByRole("button", { name: "Start your first project" });
    expect(action).toBeEnabled();
    fireEvent.click(action);
    expect(onCreate).toHaveBeenCalledOnce();
  });

  it("disables the first-run action without hiding it for guests", () => {
    render(<ProjectsEmptyState variant="first-run" canCreate={false} onCreate={() => {}} />);

    expect(screen.getByRole("button", { name: "Start your first project" })).toBeDisabled();
  });

  it("renders the no-match variant with no creation action", () => {
    render(<ProjectsEmptyState variant="no-match" canCreate={true} onCreate={() => {}} />);

    expect(screen.getByRole("heading", { name: "No matching results." })).not.toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("renders the empty-archive variant with no creation action", () => {
    render(<ProjectsEmptyState variant="empty-archive" canCreate={true} onCreate={() => {}} />);

    expect(screen.getByRole("heading", { name: "No projects archived" })).not.toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("marks each variant and gives each its own artwork", () => {
    const signatures = VARIANTS.map((variant) => {
      const { container, unmount } = render(
        <ProjectsEmptyState variant={variant} canCreate={true} onCreate={() => {}} />
      );
      const state = screen.getByTestId("projects-empty-state");
      expect(state).toHaveAttribute("data-variant", variant);
      const art = container.querySelector("svg[data-artwork]");
      expect(art).toHaveAttribute("data-artwork", variant);
      const signature = art?.outerHTML ?? variant;
      unmount();
      return signature;
    });
    expect(new Set(signatures).size).toBe(VARIANTS.length);
  });
});
