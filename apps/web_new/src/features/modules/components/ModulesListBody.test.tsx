// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One module set, three renditions (MOD-001/011): rows for the list, cards
// for the board, name rows for the timeline, each in the default name order
// and each absent from the other layouts.

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

import { MODULES_PROJECT_ID, makeModuleRow } from "../testing.js";
import { ModulesListBody } from "./ModulesListBody.js";

const ROWS = [
  makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174002", name: "Module 10" }),
  makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174003", name: "Alpha" }),
  makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174004", name: "Module 2" }),
];

function renderBody(layout: "list" | "board" | "timeline", modules = ROWS): void {
  render(<ModulesListBody workspaceSlug="acme" projectId={MODULES_PROJECT_ID} layout={layout} modules={modules} />);
}

describe("ModulesListBody", () => {
  it("renders one linked row per module in natural name order", () => {
    renderBody("list");
    const root = screen.getByTestId("modules-list");
    expect(root).toHaveAttribute("data-layout", "list");
    expect(screen.getAllByTestId("modules-row-name").map((node) => node.textContent)).toEqual([
      "Alpha",
      "Module 2",
      "Module 10",
    ]);
    expect(screen.queryAllByTestId("modules-board-card")).toHaveLength(0);
    expect(screen.queryAllByTestId("modules-timeline-row")).toHaveLength(0);
  });

  it("renders cards for the board layout", () => {
    renderBody("board");
    expect(screen.getByTestId("modules-list")).toHaveAttribute("data-layout", "board");
    expect(screen.getAllByTestId("modules-board-card-name").map((node) => node.textContent)).toEqual([
      "Alpha",
      "Module 2",
      "Module 10",
    ]);
    expect(screen.queryAllByTestId("modules-row")).toHaveLength(0);
  });

  it("renders timeline name rows for the timeline layout", () => {
    renderBody("timeline");
    expect(screen.getByTestId("modules-list")).toHaveAttribute("data-layout", "timeline");
    expect(screen.getAllByTestId("modules-timeline-row-name").map((node) => node.textContent)).toEqual([
      "Alpha",
      "Module 2",
      "Module 10",
    ]);
    expect(screen.queryAllByTestId("modules-row")).toHaveLength(0);
  });

  it("renders an empty list element for no modules", () => {
    renderBody("list", []);
    expect(screen.getByRole("list", { name: "Modules" })).toBeEmptyDOMElement();
    expect(screen.queryAllByTestId("modules-row")).toHaveLength(0);
  });
});
