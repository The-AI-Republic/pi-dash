// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { WorkspaceViewsSearch } from "./WorkspaceViewsSearch.js";

describe("WorkspaceViewsSearch", () => {
  it("reports every typed character", () => {
    const onQueryChange = vi.fn();
    render(<WorkspaceViewsSearch query="" onQueryChange={onQueryChange} />);
    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "tri" } });
    expect(onQueryChange).toHaveBeenCalledWith("tri");
  });

  it("clears the query on Escape", () => {
    const onQueryChange = vi.fn();
    render(<WorkspaceViewsSearch query="tri" onQueryChange={onQueryChange} />);
    fireEvent.keyDown(screen.getByPlaceholderText("Search"), { key: "Escape" });
    expect(onQueryChange).toHaveBeenCalledWith("");
  });

  it("ignores other keys", () => {
    const onQueryChange = vi.fn();
    render(<WorkspaceViewsSearch query="tri" onQueryChange={onQueryChange} />);
    fireEvent.keyDown(screen.getByPlaceholderText("Search"), { key: "a" });
    expect(onQueryChange).not.toHaveBeenCalled();
  });

  it("offers no clear button while the field is empty", () => {
    render(<WorkspaceViewsSearch query="" onQueryChange={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Clear search" })).not.toBeInTheDocument();
  });

  it("clears through the clear button once there is text", () => {
    const onQueryChange = vi.fn();
    render(<WorkspaceViewsSearch query="tri" onQueryChange={onQueryChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(onQueryChange).toHaveBeenCalledWith("");
  });
});
