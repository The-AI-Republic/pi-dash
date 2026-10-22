// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

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

  it("links the Intake crumb back to the canonical address", () => {
    render(<IntakeShell {...BASE} />);
    expect(screen.getByTestId("intake-breadcrumb").querySelector('a[href="/acme/projects/p-1/intake"]')).not.toBeNull();
  });

  it("keeps the placeholder with no selection and with an empty tab", () => {
    render(<IntakeShell {...BASE} selectedId="i-9" />);
    expect(screen.getByTestId("intake-placeholder")).toBeInTheDocument();
  });
});
