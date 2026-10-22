// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ForeignRowSelectionHint, GroupSelectionCheckbox, RowSelectionCheckbox } from "./SelectionCheckbox.js";

describe("selection checkboxes", () => {
  it("toggles a row control and stops the click from reaching the row", () => {
    const onChange = vi.fn();
    const parent = vi.fn();
    render(
      <div onClick={parent}>
        <RowSelectionCheckbox checked={false} label="Select WEB-1" onChange={onChange} />
      </div>
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "Select WEB-1" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(parent).not.toHaveBeenCalled();
  });

  it("shows a focusable disabled hint for a foreign-project row", () => {
    render(<ForeignRowSelectionHint label="Cannot select WEB-7" />);
    const hint = screen.getByRole("img", { name: "Cannot select WEB-7" });
    expect(hint).toHaveAttribute("tabindex", "0");
    expect(hint.className).toContain("cursor-not-allowed");
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("reflects complete and partial group states", () => {
    const onToggle = vi.fn();
    const { rerender } = render(<GroupSelectionCheckbox state="complete" label="Select all" onToggle={onToggle} />);
    const box = screen.getByRole("checkbox", { name: "Select all" }) as HTMLInputElement;
    expect(box.checked).toBe(true);
    expect(box.indeterminate).toBe(false);
    fireEvent.click(box);
    expect(onToggle).toHaveBeenCalledTimes(1);

    rerender(<GroupSelectionCheckbox state="partial" label="Select all" onToggle={onToggle} />);
    expect(box.checked).toBe(false);
    expect(box.indeterminate).toBe(true);

    rerender(<GroupSelectionCheckbox state="empty" label="Select all" onToggle={onToggle} />);
    expect(box.checked).toBe(false);
    expect(box.indeterminate).toBe(false);
  });
});
