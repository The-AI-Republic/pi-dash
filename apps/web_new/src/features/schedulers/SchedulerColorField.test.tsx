// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DEFAULT_SCHEDULER_COLOR, SCHEDULER_COLOR_SWATCHES } from "./definition.js";
import { SchedulerColorField } from "./SchedulerColorField.js";

function setup(value: string = DEFAULT_SCHEDULER_COLOR, disabled = false) {
  const onChange = vi.fn();
  render(<SchedulerColorField value={value} disabled={disabled} onChange={onChange} />);
  return { onChange };
}

function swatches(): HTMLElement[] {
  return SCHEDULER_COLOR_SWATCHES.map((swatch) => screen.getByRole("button", { name: `Scheduler color ${swatch}` }));
}

describe("SchedulerColorField", () => {
  it("renders sixteen preset swatches plus the overflow button", () => {
    setup();
    expect(swatches()).toHaveLength(16);
    expect(screen.getByRole("button", { name: "More colors" })).toHaveTextContent("…");
    expect(screen.getByRole("group", { name: "Color" })).toBeInTheDocument();
    expect(screen.getByText("Marks this scheduler's blocks on the project calendar.")).toBeInTheDocument();
  });

  it("selects a preset on click", async () => {
    const user = userEvent.setup();
    const { onChange } = setup();
    await user.click(screen.getByRole("button", { name: "Scheduler color #ef4444" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith("#ef4444");
  });

  it("marks only the stored preset as pressed", () => {
    setup("#8b5cf6");
    for (const button of swatches()) {
      const pressed = button.getAttribute("aria-label") === "Scheduler color #8b5cf6";
      expect(button).toHaveAttribute("aria-pressed", String(pressed));
    }
    expect(screen.getByRole("button", { name: "More colors" })).toHaveAttribute("aria-pressed", "false");
  });

  it("matches the stored value case-insensitively", () => {
    setup("#3B82F6");
    expect(screen.getByRole("button", { name: "Scheduler color #3b82f6" })).toHaveAttribute("aria-pressed", "true");
  });

  it("opens the custom picker from the overflow button and takes its color", async () => {
    const user = userEvent.setup();
    const click = vi.spyOn(HTMLInputElement.prototype, "click").mockImplementation(() => {});
    try {
      const { onChange } = setup();
      await user.click(screen.getByRole("button", { name: "More colors" }));
      expect(click).toHaveBeenCalledTimes(1);
      expect(click.mock.instances[0]).toBe(screen.getByLabelText("Custom color"));
      fireEvent.change(screen.getByLabelText("Custom color"), { target: { value: "#112233" } });
      expect(onChange).toHaveBeenCalledWith("#112233");
    } finally {
      click.mockRestore();
    }
  });

  it("shows a custom color on the overflow button with no preset pressed", () => {
    setup("#112233");
    for (const button of swatches()) {
      expect(button).toHaveAttribute("aria-pressed", "false");
    }
    const more = screen.getByRole("button", { name: "More colors" });
    expect(more).toHaveAttribute("aria-pressed", "true");
    const dot = more.querySelector("span[aria-hidden='true']");
    expect(dot).not.toBeNull();
    expect(dot).toHaveStyle({ backgroundColor: "#112233" });
    // The synced picker keeps the current value readable for assistive tech.
    expect(screen.getByLabelText("Custom color")).toHaveValue("#112233");
  });

  it.each(["", "not-a-hex-color"])("falls back to the default picker value for %s", (value) => {
    setup(value);
    for (const button of swatches()) {
      expect(button).toHaveAttribute("aria-pressed", "false");
    }
    expect(screen.getByRole("button", { name: "More colors" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("Custom color")).toHaveValue(DEFAULT_SCHEDULER_COLOR);
  });

  it("shows the More tooltip on the overflow button", () => {
    setup();
    fireEvent.focus(screen.getByRole("button", { name: "More colors" }));
    expect(screen.getByRole("tooltip")).toHaveTextContent("More");
  });

  it("selects with the keyboard and keeps every option in the tab order", async () => {
    const user = userEvent.setup();
    const { onChange } = setup("#3b82f6");
    for (const button of swatches()) {
      await user.tab();
      expect(document.activeElement).toBe(button);
    }
    await user.tab();
    const more = screen.getByRole("button", { name: "More colors" });
    expect(document.activeElement).toBe(more);
    // Enter activates the focused swatch; Space activates the overflow.
    await user.tab({ shift: true });
    await user.keyboard("{Enter}");
    expect(onChange).toHaveBeenCalledWith(SCHEDULER_COLOR_SWATCHES[15]);
    await user.tab();
    await user.keyboard(" ");
    // Space on the overflow only opens the native picker, which jsdom
    // cannot show, so no further selection is reported.
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("disables every control while submitting", async () => {
    const user = userEvent.setup();
    const { onChange } = setup("#3b82f6", true);
    for (const button of swatches()) {
      expect(button).toBeDisabled();
    }
    expect(screen.getByRole("button", { name: "More colors" })).toBeDisabled();
    expect(screen.getByLabelText("Custom color")).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Scheduler color #ef4444" }));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("shows the field error in place of the help text", () => {
    const onChange = vi.fn();
    render(
      <SchedulerColorField value={DEFAULT_SCHEDULER_COLOR} error="Choose a six-digit hex color." onChange={onChange} />
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a six-digit hex color.");
    expect(screen.queryByText("Marks this scheduler's blocks on the project calendar.")).not.toBeInTheDocument();
  });
});
