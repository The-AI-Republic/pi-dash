// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One timeline row (MOD-011 identity): the sidebar name anchors to the
// detail page and the chart track mounts beside it for slice I.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
}));

import { MODULES_MODULE_ID, MODULES_PROJECT_ID, makeModuleRow } from "../testing.js";
import { ModuleTimelineRow } from "./ModuleTimelineRow.js";

describe("ModuleTimelineRow", () => {
  it("links the timeline name to the detail page", () => {
    render(<ModuleTimelineRow workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} />);
    expect(screen.getByRole("link", { name: "Delivery" })).toHaveAttribute(
      "href",
      `/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}`
    );
    expect(screen.getByTestId("modules-timeline-row-name")).toHaveClass("truncate");
  });

  it("carries the module identity and mounts the track slot", () => {
    render(
      <ModuleTimelineRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ status: "completed" })}
        track={<span data-testid="timeline-track" />}
      />
    );
    const row = screen.getByTestId("modules-timeline-row");
    expect(row).toHaveAttribute("data-module-id", MODULES_MODULE_ID);
    expect(row).toHaveAttribute("data-module-status", "completed");
    expect(screen.getByTestId("timeline-track")).toBeInTheDocument();
  });
});
