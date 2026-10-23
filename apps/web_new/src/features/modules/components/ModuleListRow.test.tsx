// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One list row (MOD-001): the name is a real anchor to the module detail
// page, the row carries the module identity later slices read, and the
// leading and trailing slots mount without nesting inside the link.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className, ...rest }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className} {...rest}>
      {children}
    </a>
  ),
}));

import { MODULES_MODULE_ID, MODULES_PROJECT_ID, makeModuleRow } from "../testing.js";
import { ModuleListRow } from "./ModuleListRow.js";

describe("ModuleListRow", () => {
  it("links the name to the module detail page", () => {
    render(<ModuleListRow workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow()} />);
    const link = screen.getByRole("link", { name: "Delivery" });
    expect(link).toHaveAttribute("href", `/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}`);
    expect(screen.getByTestId("modules-row-name")).toHaveClass("truncate");
  });

  it("carries the module identity on the row", () => {
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ status: "in-progress" })}
      />
    );
    const row = screen.getByTestId("modules-row");
    expect(row).toHaveAttribute("data-module-id", MODULES_MODULE_ID);
    expect(row).toHaveAttribute("data-module-name", "Delivery");
    expect(row).toHaveAttribute("data-module-status", "in-progress");
  });

  it("mounts the leading and trailing slots beside the link", () => {
    render(
      <ModuleListRow
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        leading={<span data-testid="leading" />}
        trailing={<button type="button" data-testid="trailing" />}
      />
    );
    expect(screen.getByTestId("leading")).toBeInTheDocument();
    expect(screen.getByTestId("trailing")).toBeInTheDocument();
    expect(screen.getByTestId("modules-row-link")).not.toContainElement(screen.getByTestId("trailing"));
  });
});
