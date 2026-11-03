// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Label } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";
import { LabelChips } from "./LabelChips.js";

const LABELS: Label[] = [
  {
    id: "a",
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name: "Alpha",
    color: "#FF6900",
    parent: null,
    sort_order: 10000,
  },
  {
    id: "b",
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name: "Beta",
    color: "#00D084",
    parent: null,
    sort_order: 20000,
  },
];

describe("LabelChips", () => {
  it("renders nothing for empty or stale selections", () => {
    const { rerender } = render(<LabelChips labels={LABELS} selectedIds={[]} />);
    expect(screen.queryByRole("list", { name: "Selected labels" })).not.toBeInTheDocument();
    rerender(<LabelChips labels={LABELS} selectedIds={["ghost"]} />);
    expect(screen.queryByRole("list", { name: "Selected labels" })).not.toBeInTheDocument();
  });

  it("renders the selected chips with their colors", () => {
    render(<LabelChips labels={LABELS} selectedIds={["b", "a"]} />);
    const list = screen.getByRole("list", { name: "Selected labels" });
    expect(list.textContent).toContain("Beta");
    expect(list.textContent).toContain("Alpha");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("removes through the chip buttons", async () => {
    const user = userEvent.setup();
    const onRemove = vi.fn();
    render(<LabelChips labels={LABELS} selectedIds={["a", "ghost", "b"]} onRemove={onRemove} />);
    await user.click(screen.getByRole("button", { name: "Remove Alpha" }));
    expect(onRemove).toHaveBeenCalledWith("a");
    await user.click(screen.getByRole("button", { name: "Remove Beta" }));
    expect(onRemove).toHaveBeenCalledWith("b");
    expect(onRemove).toHaveBeenCalledTimes(2);
  });
});
