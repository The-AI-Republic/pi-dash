// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom-month dialog (parity/specs/project-settings-part-f.md,
// PSET-053/054; AGT-036): seeded and error-free on open, the number
// field explains empty and whole-number out-of-range values, refuses
// fractional counts silently, cancels reset, and a valid month forwards
// and closes.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { DELAY_RANGE_MESSAGE } from "../api/months.js";
import { CUSTOM_MONTH_SEED, CustomMonthDialog } from "./CustomMonthDialog.js";

function setup(open = true) {
  const onSubmit = vi.fn();
  const onOpenChange = vi.fn();
  const view = render(<CustomMonthDialog open={open} onOpenChange={onOpenChange} onSubmit={onSubmit} />);
  return { onSubmit, onOpenChange, ...view };
}

function dialog(): HTMLElement {
  return screen.getByRole("dialog", { name: "Customize time range" });
}

describe("CustomMonthDialog", () => {
  it("renders nothing while closed", () => {
    setup(false);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("opens seeded with one month and no range message", () => {
    setup();
    const field = within(dialog()).getByPlaceholderText("Enter Months");
    expect(field).toHaveValue(Number(CUSTOM_MONTH_SEED));
    expect(field).toHaveAttribute("min", "1");
    expect(field).toHaveAttribute("max", "12");
    expect(field).toHaveAttribute("step", "1");
    expect(within(dialog()).queryByText(DELAY_RANGE_MESSAGE)).toBeNull();
  });

  it("refuses an empty field with the range message until the user edits", async () => {
    const user = userEvent.setup();
    const { onSubmit, onOpenChange } = setup();
    const field = within(dialog()).getByPlaceholderText("Enter Months");
    await user.clear(field);
    await user.click(within(dialog()).getByRole("button", { name: "Submit" }));
    expect(within(dialog()).getByText(DELAY_RANGE_MESSAGE)).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    await user.type(field, "4");
    expect(within(dialog()).queryByText(DELAY_RANGE_MESSAGE)).toBeNull();
  });

  it("refuses zero and thirteen with the range message without forwarding or closing", async () => {
    const user = userEvent.setup();
    const { onSubmit, onOpenChange } = setup();
    const field = within(dialog()).getByPlaceholderText("Enter Months");
    for (const value of ["0", "13"]) {
      await user.clear(field);
      await user.type(field, value);
      await user.click(within(dialog()).getByRole("button", { name: "Submit" }));
      expect(within(dialog()).getByPlaceholderText("Enter Months")).toHaveValue(Number(value));
      expect(within(dialog()).getByText(DELAY_RANGE_MESSAGE)).toBeInTheDocument();
    }
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("keeps the numeric bounds while explaining an out-of-range keyboard submit", async () => {
    const user = userEvent.setup();
    const { onSubmit, onOpenChange } = setup();
    const field = within(dialog()).getByPlaceholderText("Enter Months");
    await user.clear(field);
    await user.type(field, "13");
    expect(field).toHaveAttribute("type", "number");
    expect(field).not.toHaveAttribute("required");
    expect(field).toHaveAttribute("min", "1");
    expect(field).toHaveAttribute("max", "12");
    expect(field).toHaveAttribute("step", "1");
    expect((field as HTMLInputElement).checkValidity()).toBe(false);
    await user.type(field, "{Enter}");
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(within(dialog()).getByText(DELAY_RANGE_MESSAGE)).toBeInTheDocument();
  });

  it("explains an out-of-range value submitted past native validation", async () => {
    const user = userEvent.setup();
    const { onSubmit, onOpenChange } = setup();
    const field = within(dialog()).getByPlaceholderText("Enter Months");
    await user.clear(field);
    await user.type(field, "13");
    const form = field.closest("form");
    if (form === null) throw new Error("month field is outside its form");
    // A direct submit skips the browser's own check; the handler still
    // keeps the dialog open and explains the accepted range.
    fireEvent.submit(form);
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(within(dialog()).getByText(DELAY_RANGE_MESSAGE)).toBeInTheDocument();
  });

  it("forwards a whole month and asks to close", async () => {
    const user = userEvent.setup();
    const { onSubmit, onOpenChange } = setup();
    const field = within(dialog()).getByPlaceholderText("Enter Months");
    await user.clear(field);
    await user.type(field, "5");
    await user.click(within(dialog()).getByRole("button", { name: "Submit" }));
    expect(onSubmit).toHaveBeenCalledWith(5);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("forwards a whole month entered in exponent form", async () => {
    const user = userEvent.setup();
    const { onSubmit, onOpenChange } = setup();
    const field = within(dialog()).getByPlaceholderText("Enter Months");
    await user.clear(field);
    await user.type(field, "1e1");
    // The number field accepts exponent notation and reports it as a valid
    // whole value; the handler must forward the months it denotes.
    await user.click(within(dialog()).getByRole("button", { name: "Submit" }));
    expect(onSubmit).toHaveBeenCalledWith(10);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("cancels without submitting and reseeds on the next open", async () => {
    const user = userEvent.setup();
    const { onSubmit, onOpenChange, rerender } = setup();
    const field = within(dialog()).getByPlaceholderText("Enter Months");
    await user.clear(field);
    await user.type(field, "7");
    await user.click(within(dialog()).getByRole("button", { name: "Submit" }));
    expect(onSubmit).toHaveBeenCalledWith(7);
    await user.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);

    rerender(<CustomMonthDialog open={false} onOpenChange={onOpenChange} onSubmit={onSubmit} />);
    rerender(<CustomMonthDialog open onOpenChange={onOpenChange} onSubmit={onSubmit} />);
    expect(within(dialog()).getByPlaceholderText("Enter Months")).toHaveValue(1);
    expect(within(dialog()).queryByText(DELAY_RANGE_MESSAGE)).toBeNull();
  });

  it("clears a stale range message when reopened", async () => {
    const user = userEvent.setup();
    const { onSubmit, onOpenChange, rerender } = setup();
    const field = within(dialog()).getByPlaceholderText("Enter Months");
    await user.clear(field);
    await user.click(within(dialog()).getByRole("button", { name: "Submit" }));
    expect(within(dialog()).getByText(DELAY_RANGE_MESSAGE)).toBeInTheDocument();

    rerender(<CustomMonthDialog open={false} onOpenChange={onOpenChange} onSubmit={onSubmit} />);
    rerender(<CustomMonthDialog open onOpenChange={onOpenChange} onSubmit={onSubmit} />);
    expect(within(dialog()).queryByText(DELAY_RANGE_MESSAGE)).toBeNull();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("refuses a fractional value silently without forwarding", async () => {
    const user = userEvent.setup();
    const { onSubmit, onOpenChange } = setup();
    const field = within(dialog()).getByPlaceholderText("Enter Months");
    await user.clear(field);
    await user.type(field, "2.5");
    // A fractional count is refused the way the number field's step
    // check would: silently, without a message, dialog left open.
    await user.click(within(dialog()).getByRole("button", { name: "Submit" }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(within(dialog()).getByPlaceholderText("Enter Months")).toHaveValue(2.5);
    expect(within(dialog()).queryByText(DELAY_RANGE_MESSAGE)).toBeNull();
  });
});
