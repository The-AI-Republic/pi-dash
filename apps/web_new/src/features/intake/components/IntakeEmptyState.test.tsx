// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { IntakeEmptyState } from "./IntakeEmptyState.js";

describe("IntakeEmptyState (INT-008)", () => {
  it("names filters as the reason when the user filtered to nothing", () => {
    render(<IntakeEmptyState kind="filtered" />);
    expect(screen.getByTestId("intake-empty-filtered")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "No matching results." })).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("guides an empty Open tab with a create action", () => {
    const onCreate = vi.fn();
    render(<IntakeEmptyState kind="open-guidance" onCreate={onCreate} />);
    expect(screen.getByTestId("intake-empty-open")).toBeInTheDocument();
    expect(screen.queryByTestId("intake-empty-filtered")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create request" }));
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("keeps the Open create action visible but inert when the dialog is not wired yet", () => {
    render(<IntakeEmptyState kind="open-guidance" />);
    expect(screen.getByRole("button", { name: "Create request" })).toBeDisabled();
  });

  it("informs an empty Closed tab and offers no create action", () => {
    render(<IntakeEmptyState kind="closed-informational" />);
    expect(screen.getByTestId("intake-empty-closed")).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("intake-empty-open")).not.toBeInTheDocument();
  });
});
