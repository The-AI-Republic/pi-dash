// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Static default row (parity/specs/views.md §9, VIEW-030/031).
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
}));

import { WORKSPACE_DEFAULT_VIEWS } from "../lib/workspaceViews.js";
import { WorkspaceDefaultViewRow } from "./WorkspaceDefaultViewRow.js";

describe("WorkspaceDefaultViewRow", () => {
  it.each(WORKSPACE_DEFAULT_VIEWS.map((view) => [view.key, view.label]))(
    "links the %s default to its static detail route",
    (key, label) => {
      const view = WORKSPACE_DEFAULT_VIEWS.find((entry) => entry.key === key);
      if (view === undefined) throw new Error("missing default");
      render(<WorkspaceDefaultViewRow workspaceSlug="acme" view={view} />);
      const row = screen.getByTestId("workspace-views-default-row");
      expect(row).toHaveAttribute("data-view-key", key);
      expect(screen.getByRole("link", { name: label })).toHaveAttribute("href", `/acme/workspace-views/${key}`);
    }
  );

  it("renders one bare link — no star, badge or menu button", () => {
    render(<WorkspaceDefaultViewRow workspaceSlug="acme" view={WORKSPACE_DEFAULT_VIEWS[0]!} />);
    const row = screen.getByTestId("workspace-views-default-row");
    expect(row.querySelectorAll("a")).toHaveLength(1);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(row.textContent).toBe("All work items");
  });
});
