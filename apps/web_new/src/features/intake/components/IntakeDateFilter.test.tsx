// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { IntakeDateValue } from "../lib/dates.js";
import { IntakeDateFilter } from "./IntakeDateFilter.js";

const TODAY = "2026-10-23";

const BASE = {
  section: "createdAt",
  value: null,
  searchText: "",
  onSearchTextChange: () => {},
  today: TODAY,
  weekStart: 0,
  onPresetToggle: () => {},
  onCustomApply: () => {},
} as const;

function props(overrides: Partial<React.ComponentProps<typeof IntakeDateFilter>> = {}) {
  return { ...BASE, ...overrides };
}

function options(): string[] {
  return screen.getAllByRole("radio").map((node) => node.getAttribute("aria-label") ?? "");
}

function openCustom(): HTMLElement {
  fireEvent.click(screen.getByRole("radio", { name: "Custom" }));
  return screen.getByRole("dialog");
}

function dayButton(dialog: HTMLElement, label: string): HTMLElement {
  return within(dialog).getByRole("button", { name: label });
}

describe("IntakeDateFilter (INT-010)", () => {
  it("offers the presets plus Custom as single-select options", () => {
    render(<IntakeDateFilter {...props()} />);
    expect(options()).toEqual(["Today", "Yesterday", "Last 7 days", "Last 30 days", "Custom"]);
    expect(screen.getByRole("radio", { name: "Today" })).toHaveAttribute("aria-checked", "false");
  });

  it("marks the applied preset or the custom range", () => {
    const { rerender } = render(<IntakeDateFilter {...props()} value={{ kind: "preset", preset: "today" }} />);
    expect(screen.getByRole("radio", { name: "Today" })).toHaveAttribute("aria-checked", "true");
    rerender(<IntakeDateFilter {...props()} value={{ kind: "custom-range", from: TODAY, to: TODAY }} />);
    expect(screen.getByRole("radio", { name: "Custom" })).toHaveAttribute("aria-checked", "true");
  });

  it("toggles presets through the host", () => {
    const onPresetToggle = vi.fn();
    render(<IntakeDateFilter {...props({ onPresetToggle })} />);
    fireEvent.click(screen.getByRole("radio", { name: "Last 7 days" }));
    expect(onPresetToggle).toHaveBeenCalledWith("last7");
  });

  it("titles the dialog after the section it was opened from", () => {
    const { rerender } = render(<IntakeDateFilter {...props()} />);
    expect(openCustom()).toHaveTextContent("Created date — Custom");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    rerender(<IntakeDateFilter {...props({ section: "updatedAt" })} />);
    expect(openCustom()).toHaveTextContent("Updated date — Custom");
  });

  it("applies the seeded range without picking", () => {
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ onCustomApply })} />);
    const dialog = openCustom();
    expect(within(dialog).getByRole("radio", { name: "Range" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(onCustomApply).toHaveBeenCalledWith({ kind: "custom-range", from: TODAY, to: "2026-11-23" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("applies a picked range pair", () => {
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ onCustomApply })} />);
    const dialog = openCustom();
    fireEvent.click(dayButton(dialog, "October 15, 2026"));
    fireEvent.click(dayButton(dialog, "October 20, 2026"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(onCustomApply).toHaveBeenCalledWith({
      kind: "custom-range",
      from: "2026-10-15",
      to: "2026-10-20",
    });
  });

  it("applies a one-sided bound and never disables Apply there", () => {
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ onCustomApply })} />);
    const dialog = openCustom();
    fireEvent.click(within(dialog).getByRole("radio", { name: "Before" }));
    const apply = within(dialog).getByRole("button", { name: "Apply" });
    expect(apply).not.toBeDisabled();
    fireEvent.click(dayButton(dialog, "October 20, 2026"));
    fireEvent.click(apply);
    expect(onCustomApply).toHaveBeenCalledWith({ kind: "custom-before", day: "2026-10-20" });
  });

  it("applies an after bound", () => {
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ onCustomApply })} />);
    const dialog = openCustom();
    fireEvent.click(within(dialog).getByRole("radio", { name: "After" }));
    fireEvent.click(dayButton(dialog, "October 20, 2026"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(onCustomApply).toHaveBeenCalledWith({ kind: "custom-after", day: "2026-10-20" });
  });

  it("closes a one-sided Apply without a day and applies nothing", () => {
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ onCustomApply })} />);
    const dialog = openCustom();
    fireEvent.click(within(dialog).getByRole("radio", { name: "Before" }));
    // Drop the carried seed day back to none: range, reset, back.
    fireEvent.click(within(dialog).getByRole("radio", { name: "Range" }));
    fireEvent.click(dayButton(dialog, "October 23, 2026"));
    fireEvent.click(within(dialog).getByRole("radio", { name: "Before" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(onCustomApply).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("disables Apply while the range pair is incomplete", () => {
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ onCustomApply })} />);
    const dialog = openCustom();
    fireEvent.click(within(dialog).getByRole("radio", { name: "Before" }));
    fireEvent.click(dayButton(dialog, "October 20, 2026"));
    fireEvent.click(within(dialog).getByRole("radio", { name: "Range" }));
    expect(within(dialog).getByRole("button", { name: "Apply" })).toBeDisabled();
    expect(onCustomApply).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("opens a fresh range over a stored preset", () => {
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ value: { kind: "preset", preset: "today" }, onCustomApply })} />);
    const dialog = openCustom();
    expect(within(dialog).getByRole("radio", { name: "Range" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(dayButton(dialog, "October 15, 2026"));
    fireEvent.click(dayButton(dialog, "October 20, 2026"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(onCustomApply).toHaveBeenCalledWith({
      kind: "custom-range",
      from: "2026-10-15",
      to: "2026-10-20",
    });
  });

  it("ignores a mode pick that changes nothing", () => {
    render(<IntakeDateFilter {...props()} />);
    const dialog = openCustom();
    fireEvent.click(within(dialog).getByRole("radio", { name: "Range" }));
    expect(within(dialog).getByRole("radio", { name: "Range" })).toHaveAttribute("aria-checked", "true");
    expect(within(dialog).getByRole("button", { name: "Apply" })).not.toBeDisabled();
  });

  it("cancels without applying", () => {
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ onCustomApply })} />);
    openCustom();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCustomApply).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("reopens a stored range in range mode", () => {
    const value: IntakeDateValue = { kind: "custom-range", from: "2026-10-15", to: "2026-10-20" };
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ value, onCustomApply })} />);
    const dialog = openCustom();
    expect(within(dialog).getByRole("radio", { name: "Range" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(onCustomApply).toHaveBeenCalledWith(value);
  });

  it("reopens a stored one-sided bound in its own mode", () => {
    const value: IntakeDateValue = { kind: "custom-before", day: "2026-10-20" };
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ value, onCustomApply })} />);
    const dialog = openCustom();
    expect(within(dialog).getByRole("radio", { name: "Before" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(onCustomApply).toHaveBeenCalledWith(value);
  });

  it("reopens a stored after bound in after mode", () => {
    const value: IntakeDateValue = { kind: "custom-after", day: "2026-10-20" };
    const onCustomApply = vi.fn();
    render(<IntakeDateFilter {...props({ value, onCustomApply })} />);
    const dialog = openCustom();
    expect(within(dialog).getByRole("radio", { name: "After" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(onCustomApply).toHaveBeenCalledWith(value);
  });
});
