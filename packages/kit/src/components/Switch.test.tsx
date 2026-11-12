// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Switch } from "./Switch";

describe("Switch", () => {
  it("renders a switch reflecting the controlled state", () => {
    const { rerender } = render(<Switch checked={false} aria-label="Notify me" />);
    const toggle = screen.getByRole("switch", { name: "Notify me" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    rerender(<Switch checked aria-label="Notify me" />);
    expect(screen.getByRole("switch", { name: "Notify me" })).toHaveAttribute("aria-checked", "true");
  });

  it("reports the next state on click when controlled", async () => {
    const user = userEvent.setup();
    const seen: boolean[] = [];
    render(<Switch checked={false} onCheckedChange={(next) => seen.push(next)} aria-label="Notify me" />);
    await user.click(screen.getByRole("switch", { name: "Notify me" }));
    expect(seen).toEqual([true]);
  });

  it("toggles uncontrolled from its default", async () => {
    const user = userEvent.setup();
    const seen: boolean[] = [];
    render(<Switch defaultChecked onCheckedChange={(next) => seen.push(next)} aria-label="Notify me" />);
    const toggle = screen.getByRole("switch", { name: "Notify me" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(seen).toEqual([false]);
  });

  it("toggles with the keyboard", async () => {
    const user = userEvent.setup();
    const seen: boolean[] = [];
    render(<Switch defaultChecked={false} onCheckedChange={(next) => seen.push(next)} aria-label="Notify me" />);
    const toggle = screen.getByRole("switch", { name: "Notify me" });
    toggle.focus();
    await user.keyboard(" ");
    expect(seen).toEqual([true]);
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  it("ignores interaction while disabled", async () => {
    const user = userEvent.setup();
    const onCheckedChange = vi.fn();
    render(<Switch defaultChecked={false} disabled onCheckedChange={onCheckedChange} aria-label="Notify me" />);
    const toggle = screen.getByRole("switch", { name: "Notify me" });
    expect(toggle).toHaveAttribute("aria-disabled", "true");
    await user.click(toggle);
    expect(onCheckedChange).not.toHaveBeenCalled();
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  it("renders without a change handler", async () => {
    const user = userEvent.setup();
    render(<Switch defaultChecked={false} aria-label="Notify me" />);
    const toggle = screen.getByRole("switch", { name: "Notify me" });
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  it("forwards labelling attributes", () => {
    render(
      <div>
        <span id="notify-label">Notify me</span>
        <Switch defaultChecked aria-labelledby="notify-label" data-testid="notify-switch" />
      </div>
    );
    const toggle = screen.getByRole("switch", { name: "Notify me" });
    expect(toggle).toHaveAttribute("aria-labelledby", "notify-label");
    expect(toggle).toHaveAttribute("data-testid", "notify-switch");
  });

  it("carries the visible focus ring and disabled styling hooks", () => {
    const { rerender } = render(<Switch defaultChecked aria-label="Notify me" className="extra-class" />);
    const toggle = screen.getByRole("switch", { name: "Notify me" });
    expect(toggle.className).toContain("focus-visible:outline-2");
    expect(toggle.className).toContain("extra-class");
    expect(toggle).toHaveAttribute("data-checked");
    rerender(<Switch defaultChecked aria-label="Notify me" disabled className="extra-class" />);
    expect(screen.getByRole("switch", { name: "Notify me" })).toHaveAttribute("data-disabled");
  });

  it("exposes the label as its accessible name with the checked state", () => {
    render(<Switch checked label="Enable job" onCheckedChange={() => {}} />);
    const control = screen.getByRole("switch", { name: "Enable job" });
    expect(control.getAttribute("aria-checked")).toBe("true");
  });

  it("reports unchecked when off", () => {
    render(<Switch checked={false} label="Enable job" onCheckedChange={() => {}} />);
    expect(screen.getByRole("switch", { name: "Enable job" }).getAttribute("aria-checked")).toBe("false");
  });

  it("notifies with the flipped state on click", async () => {
    const user = userEvent.setup();
    const onCheckedChange = vi.fn();
    render(<Switch checked={false} label="Enable job" onCheckedChange={onCheckedChange} />);
    await user.click(screen.getByRole("switch", { name: "Enable job" }));
    expect(onCheckedChange).toHaveBeenCalledWith(true);
  });

  it("flips with the keyboard", async () => {
    const user = userEvent.setup();
    const onCheckedChange = vi.fn();
    render(<Switch checked label="Enable job" onCheckedChange={onCheckedChange} />);
    const control = screen.getByRole("switch", { name: "Enable job" });
    control.focus();
    await user.keyboard(" ");
    expect(onCheckedChange).toHaveBeenCalledWith(false);
  });

  it("stays silent and marked when disabled", async () => {
    const user = userEvent.setup();
    const onCheckedChange = vi.fn();
    render(<Switch checked={false} label="Enable job" onCheckedChange={onCheckedChange} disabled />);
    const control = screen.getByRole("switch", { name: "Enable job" });
    expect(control.hasAttribute("data-disabled")).toBe(true);
    await user.click(control);
    expect(onCheckedChange).not.toHaveBeenCalled();
  });

  it("carries the visible focus ring classes", () => {
    render(<Switch checked={false} label="Enable job" onCheckedChange={() => {}} />);
    expect(screen.getByRole("switch", { name: "Enable job" }).className).toContain("focus-visible:outline-2");
  });

  it("passes extra classes through", () => {
    render(<Switch checked={false} label="Enable job" onCheckedChange={() => {}} className="extra" />);
    expect(screen.getByRole("switch", { name: "Enable job" }).className).toContain("extra");
  });

  it("exposes the on state to assistive technology", () => {
    render(<Switch checked label="Email updates" onCheckedChange={() => {}} />);
    const control = screen.getByRole("switch", { name: "Email updates" });
    expect(control).toHaveAttribute("aria-checked", "true");
    expect(control).toHaveAttribute("data-checked");
    expect(control).not.toBeDisabled();
  });

  it("exposes the off state without the checked marker", () => {
    render(<Switch checked={false} label="Email updates" onCheckedChange={() => {}} />);
    const control = screen.getByRole("switch", { name: "Email updates" });
    expect(control).toHaveAttribute("aria-checked", "false");
    expect(control).not.toHaveAttribute("data-checked");
  });

  it("reports the flipped state on user input", () => {
    const onCheckedChange = vi.fn();
    render(<Switch checked={false} label="Email updates" onCheckedChange={onCheckedChange} />);
    fireEvent.click(screen.getByRole("switch", { name: "Email updates" }));
    expect(onCheckedChange).toHaveBeenCalledWith(true);
  });

  it("stays silent when disabled", () => {
    const onCheckedChange = vi.fn();
    render(<Switch checked={false} label="Email updates" disabled onCheckedChange={onCheckedChange} />);
    const control = screen.getByRole("switch", { name: "Email updates" });
    expect(control).toHaveAttribute("aria-disabled", "true");
    expect(control).toHaveAttribute("data-disabled");
    fireEvent.click(control);
    expect(onCheckedChange).not.toHaveBeenCalled();
  });

  it("renders without a handler or label", () => {
    render(<Switch checked />);
    const control = screen.getByRole("switch");
    expect(control).toBeInTheDocument();
    expect(control).not.toHaveAttribute("aria-label");
    fireEvent.click(control);
  });

  it("accepts an extra class", () => {
    render(<Switch checked label="Email updates" className="extra-class" />);
    expect(screen.getByRole("switch", { name: "Email updates" })).toHaveClass("extra-class");
  });
});
