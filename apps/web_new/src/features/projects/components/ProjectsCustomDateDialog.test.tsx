// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ProjectsCustomDateDialog } from "./ProjectsCustomDateDialog.js";

function toInputDay(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function setup(handlers: { onOpenChange?: (open: boolean) => void; onApply?: (values: string[]) => void } = {}) {
  const onOpenChange = handlers.onOpenChange ?? vi.fn();
  const onApply = handlers.onApply ?? vi.fn();
  const view = render(<ProjectsCustomDateDialog open={true} onOpenChange={onOpenChange} onApply={onApply} />);
  return { onOpenChange, onApply, view };
}

describe("ProjectsCustomDateDialog", () => {
  it("defaults to a range from today to next month", () => {
    setup();

    const today = new Date();
    const nextMonth = new Date(today.getFullYear(), today.getMonth() + 1, today.getDate());
    expect(screen.getByRole("radio", { name: "Range" })).toBeChecked();
    expect(screen.getByLabelText("Start date")).toHaveValue(toInputDay(today));
    expect(screen.getByLabelText("End date")).toHaveValue(toInputDay(nextMonth));
  });

  it("applies the range as an after/before pair and closes", () => {
    const { onApply, onOpenChange } = setup();

    fireEvent.change(screen.getByLabelText("Start date"), { target: { value: "2026-10-01" } });
    fireEvent.change(screen.getByLabelText("End date"), { target: { value: "2026-10-31" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    expect(onApply).toHaveBeenCalledWith(["2026-10-01;after", "2026-10-31;before"]);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("applies single-sided picks", () => {
    const { onApply } = setup();

    fireEvent.click(screen.getByRole("radio", { name: "Before" }));
    expect(screen.queryByLabelText("End date")).toBeNull();
    fireEvent.change(screen.getByLabelText("Date"), { target: { value: "2026-10-01" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(onApply).toHaveBeenCalledWith(["2026-10-01;before"]);

    fireEvent.click(screen.getByRole("radio", { name: "After" }));
    fireEvent.change(screen.getByLabelText("Date"), { target: { value: "2026-11-01" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(onApply).toHaveBeenCalledWith(["2026-11-01;after"]);
  });

  it("blocks backwards and empty ranges", () => {
    setup();

    fireEvent.change(screen.getByLabelText("Start date"), { target: { value: "2026-10-31" } });
    fireEvent.change(screen.getByLabelText("End date"), { target: { value: "2026-10-01" } });
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent("The end date must be on or after the start date.");

    fireEvent.change(screen.getByLabelText("End date"), { target: { value: "" } });
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
  });

  it("blocks an empty date on single-sided picks", () => {
    setup();

    fireEvent.click(screen.getByRole("radio", { name: "After" }));
    fireEvent.change(screen.getByLabelText("Date"), { target: { value: "" } });
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
  });

  it("cancels without applying", () => {
    const { onApply, onOpenChange } = setup();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(onApply).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("resets to defaults on every open", () => {
    const onOpenChange = vi.fn();
    const onApply = vi.fn();
    const view = render(<ProjectsCustomDateDialog open={true} onOpenChange={onOpenChange} onApply={onApply} />);

    fireEvent.click(screen.getByRole("radio", { name: "Before" }));
    fireEvent.change(screen.getByLabelText("Date"), { target: { value: "2020-01-01" } });
    view.rerender(<ProjectsCustomDateDialog open={false} onOpenChange={onOpenChange} onApply={onApply} />);
    view.rerender(<ProjectsCustomDateDialog open={true} onOpenChange={onOpenChange} onApply={onApply} />);

    expect(screen.getByRole("radio", { name: "Range" })).toBeChecked();
    expect(screen.getByLabelText("Start date")).toHaveValue(toInputDay(new Date()));
  });
});
