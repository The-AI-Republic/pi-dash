// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { IntakeShell } from "./IntakeShell.js";

const BASE = {
  workspaceSlug: "acme",
  workspaceName: "Acme",
  projectId: "p-1",
  projectName: "Apollo",
  tab: "open",
  openTotal: 2,
  loading: false,
  failed: false,
  rows: [],
  emptyKind: "open-guidance",
  selectedId: undefined,
  onSelectTab: () => {},
  onSelectRow: () => {},
  onRetry: () => {},
} as const;

describe("IntakeShell (INT-001)", () => {
  it("splits the body into a list pane and a content pane", () => {
    render(<IntakeShell {...BASE} />);
    expect(screen.getByTestId("intake-list-pane")).toBeInTheDocument();
    expect(screen.getByTestId("intake-content-pane")).toBeInTheDocument();
    expect(screen.getByTestId("intake-placeholder")).toBeInTheDocument();
  });

  it("carries the workspace, project and Intake trail", () => {
    render(<IntakeShell {...BASE} />);
    const trail = screen.getByTestId("intake-breadcrumb");
    expect(trail).toHaveTextContent("Acme");
    expect(trail).toHaveTextContent("Apollo");
    expect(trail).toHaveTextContent("Intake");
  });

  it("links the visible Intake crumb back to the canonical address", () => {
    render(<IntakeShell {...BASE} />);
    const trail = screen.getByTestId("intake-breadcrumb");
    const crumb = screen.getByRole("link", { name: "Intake" });
    expect(trail.contains(crumb)).toBe(true);
    expect(crumb).toBeVisible();
    expect(crumb).toHaveAttribute("href", "/acme/projects/p-1/intake");
    expect(crumb).toHaveAttribute("aria-current", "page");
    expect(trail.querySelectorAll("a").length).toBe(1);
  });

  it("renders the supplied detail instead of the pick placeholder", () => {
    render(<IntakeShell {...BASE} selectedId="i-9" detail={<p>request detail</p>} />);
    expect(screen.getByTestId("intake-content-pane")).toBeInTheDocument();
    expect(screen.getByText("request detail")).toBeInTheDocument();
    expect(screen.queryByTestId("intake-placeholder")).toBeNull();
  });
});

describe("IntakeShell header create control (INT-013)", () => {
  it("offers creation in the header while the screen allows it", () => {
    const onCreate = vi.fn();
    render(<IntakeShell {...BASE} canCreate onCreate={onCreate} />);
    const control = screen.getByTestId("intake-header-create");
    expect(control).toHaveTextContent("Create request");
    fireEvent.click(control);
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("hides the control when creation is not offered", () => {
    render(<IntakeShell {...BASE} canCreate={false} onCreate={() => {}} />);
    expect(screen.queryByTestId("intake-header-create")).toBeNull();
  });

  it("hides the control without a create handler", () => {
    render(<IntakeShell {...BASE} canCreate />);
    expect(screen.queryByTestId("intake-header-create")).toBeNull();
  });
});
