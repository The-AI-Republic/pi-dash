// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { WorkspacePendingView } from "./WorkspacePendingView.js";

describe("WorkspacePendingView", () => {
  it("names the admin address the request went to", () => {
    render(<WorkspacePendingView adminEmail="admin@example.com" onCreateInstead={() => undefined} />);
    expect(screen.getByRole("heading", { name: "Your request is pending" })).toBeInTheDocument();
    expect(screen.getByText("admin@example.com")).toBeInTheDocument();
  });

  it("falls back to a generic message without an address", () => {
    render(<WorkspacePendingView adminEmail="" onCreateInstead={() => undefined} />);
    expect(screen.getByText(/waiting for approval/)).toBeInTheDocument();
  });

  it("returns to the create view through its only action", () => {
    const onCreateInstead = vi.fn();
    render(<WorkspacePendingView adminEmail="admin@example.com" onCreateInstead={onCreateInstead} />);
    expect(screen.getAllByRole("button")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Create a workspace instead" }));
    expect(onCreateInstead).toHaveBeenCalledTimes(1);
  });
});
