// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CUSTOMIZE_DELAY_LABEL, DelayPicker } from "./DelayPicker.js";

function setup(props: Partial<React.ComponentProps<typeof DelayPicker>> = {}) {
  const onSelect = vi.fn();
  const onCustomize = vi.fn();
  render(<DelayPicker value={1} label="Delay" onSelect={onSelect} onCustomize={onCustomize} {...props} />);
  return { onSelect, onCustomize };
}

describe("DelayPicker", () => {
  it("labels the closed trigger with the current delay", () => {
    setup({ value: 3 });
    const trigger = screen.getByRole("button", { name: /^Delay,/ });
    expect(trigger).toHaveTextContent("3 months");
    // The caret is a decorative icon, not text: anchor probes read the
    // trigger's text as exactly the delay phrase.
    expect(trigger.textContent).toBe("3 months");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("opens a listbox with every preset and the custom entry", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: /^Delay,/ }));
    const list = screen.getByRole("listbox", { name: "Delay" });
    expect(
      within(list)
        .getAllByRole("option")
        .map((option) => option.textContent)
    ).toEqual(["1 month", "3 months", "6 months", "9 months", "12 months"]);
    expect(within(list).getByRole("option", { name: "1 month" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: CUSTOMIZE_DELAY_LABEL })).toBeInTheDocument();
  });

  it("reports a preset and closes", async () => {
    const user = userEvent.setup();
    const { onSelect, onCustomize } = setup();
    await user.click(screen.getByRole("button", { name: /^Delay,/ }));
    await user.click(screen.getByRole("option", { name: "9 months" }));
    expect(onSelect).toHaveBeenCalledWith(9);
    expect(onCustomize).not.toHaveBeenCalled();
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("opens the custom dialog without choosing a preset", async () => {
    const user = userEvent.setup();
    const { onSelect, onCustomize } = setup();
    await user.click(screen.getByRole("button", { name: /^Delay,/ }));
    await user.click(screen.getByRole("button", { name: CUSTOMIZE_DELAY_LABEL }));
    expect(onCustomize).toHaveBeenCalledTimes(1);
    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("closes on Escape and on an outside click without reporting", async () => {
    const user = userEvent.setup();
    const { onSelect, onCustomize } = setup();
    await user.click(screen.getByRole("button", { name: /^Delay,/ }));
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    await user.click(screen.getByRole("button", { name: /^Delay,/ }));
    await user.click(document.body);
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(onSelect).not.toHaveBeenCalled();
    expect(onCustomize).not.toHaveBeenCalled();
  });

  it("toggles closed from the trigger", async () => {
    const user = userEvent.setup();
    setup();
    const trigger = screen.getByRole("button", { name: /^Delay,/ });
    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("stays closed while disabled", async () => {
    const user = userEvent.setup();
    setup({ disabled: true });
    const trigger = screen.getByRole("button", { name: /^Delay,/ });
    expect(trigger).toBeDisabled();
    await user.click(trigger);
    expect(screen.queryByRole("listbox")).toBeNull();
  });
});
