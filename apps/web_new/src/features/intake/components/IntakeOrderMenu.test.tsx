// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { IntakeOrderMenu } from "./IntakeOrderMenu.js";

function openMenu(): void {
  fireEvent.click(screen.getByTestId("intake-order-trigger"));
}

describe("IntakeOrderMenu (INT-011)", () => {
  it("triggers from the current field and direction", () => {
    render(<IntakeOrderMenu order={{ field: "created", direction: "desc" }} onChange={() => {}} />);
    expect(screen.getByTestId("intake-order-trigger")).toHaveAttribute("aria-label", "Sort: Created date, Descending");
  });

  it("marks the chosen field and direction", () => {
    render(<IntakeOrderMenu order={{ field: "updated", direction: "asc" }} onChange={() => {}} />);
    openMenu();
    expect(screen.getByTestId("intake-order-field-created")).toHaveAttribute("data-order-checked", "false");
    expect(screen.getByTestId("intake-order-field-updated")).toHaveAttribute("data-order-checked", "true");
    expect(screen.getByTestId("intake-order-field-id")).toHaveAttribute("data-order-checked", "false");
    expect(screen.getByTestId("intake-order-direction-desc")).toHaveAttribute("data-order-checked", "false");
    expect(screen.getByTestId("intake-order-direction-asc")).toHaveAttribute("data-order-checked", "true");
    expect(screen.getAllByRole("menuitem").map((node) => node.textContent)).toEqual([
      "Created date",
      "Last updated date",
      "ID",
      "Descending",
      "Ascending",
    ]);
  });

  it("re-sorts by field keeping the direction", () => {
    const onChange = vi.fn();
    render(<IntakeOrderMenu order={{ field: "created", direction: "desc" }} onChange={onChange} />);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "ID" }));
    expect(onChange).toHaveBeenCalledWith({ field: "id", direction: "desc" });
  });

  it("re-sorts by direction keeping the field", () => {
    const onChange = vi.fn();
    render(<IntakeOrderMenu order={{ field: "id", direction: "desc" }} onChange={onChange} />);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Ascending" }));
    expect(onChange).toHaveBeenCalledWith({ field: "id", direction: "asc" });
  });
});
