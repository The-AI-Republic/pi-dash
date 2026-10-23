// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One board card (MOD-001/020 identity): the name links to the detail page
// and keeps its full text in a title attribute for truncation, the card
// carries the module identity, and the header, footer and actions slots
// mount outside the name link.

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
import { ModuleBoardCard } from "./ModuleBoardCard.js";

describe("ModuleBoardCard", () => {
  it("links the name and keeps the full name on hover", () => {
    const name = "Card With An Exceptionally Long Name That Surely Truncates Inside A Narrow Column";
    render(<ModuleBoardCard workspaceSlug="acme" projectId={MODULES_PROJECT_ID} module={makeModuleRow({ name })} />);
    expect(screen.getByRole("link", { name })).toHaveAttribute(
      "href",
      `/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}`
    );
    const label = screen.getByTestId("modules-board-card-name");
    expect(label).toHaveClass("truncate");
    expect(label).toHaveAttribute("title", name);
  });

  it("carries the module identity on the card", () => {
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ name: "Card", status: "paused" })}
      />
    );
    const card = screen.getByTestId("modules-board-card");
    expect(card).toHaveAttribute("data-module-id", MODULES_MODULE_ID);
    expect(card).toHaveAttribute("data-module-name", "Card");
    expect(card).toHaveAttribute("data-module-status", "paused");
  });

  it("mounts the header, footer and action slots outside the name link", () => {
    render(
      <ModuleBoardCard
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow()}
        header={<span data-testid="card-header" />}
        footer={<span data-testid="card-footer" />}
        actions={<button type="button" data-testid="card-actions" />}
      />
    );
    expect(screen.getByTestId("card-header")).toBeInTheDocument();
    expect(screen.getByTestId("card-footer")).toBeInTheDocument();
    const actions = screen.getByTestId("card-actions");
    expect(actions).toBeInTheDocument();
    expect(screen.getByTestId("modules-board-card-link")).not.toContainElement(actions);
  });
});
