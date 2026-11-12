// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Card snooze menu (NTF-020): exactly the five presets in order, the
// custom entry, and the remove entry only on an already-snoozed card.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { SNOOZE_PRESETS } from "../lib/snooze.js";
import { CUSTOM_SNOOZE_ENTRY, REMOVE_SNOOZE_ENTRY, SnoozeMenu } from "./SnoozeMenu.js";

function renderMenu(overrides: Partial<React.ComponentProps<typeof SnoozeMenu>> = {}) {
  const handlers = {
    onPreset: vi.fn(),
    onCustom: vi.fn(),
    onRemove: vi.fn(),
  };
  render(<SnoozeMenu snoozed={false} busy={false} {...handlers} {...overrides} />);
  return handlers;
}

describe("SnoozeMenu", () => {
  it("keeps the panel closed until the trigger is activated", () => {
    renderMenu();
    expect(screen.queryByTestId("snooze-panel")).not.toBeInTheDocument();
  });

  it("offers the five presets in menu order and the custom entry", async () => {
    const user = userEvent.setup();
    renderMenu();
    await user.click(screen.getByTestId("snooze-trigger"));
    const panel = await screen.findByTestId("snooze-panel");
    const options = Array.from(panel.querySelectorAll("button")).map((button) => button.textContent);
    expect(options).toEqual([...SNOOZE_PRESETS.map((preset) => preset.label), CUSTOM_SNOOZE_ENTRY]);
    expect(screen.getByTestId("snooze-option-day-1")).toHaveTextContent("1 day");
    expect(screen.getByTestId("snooze-option-week-2")).toHaveTextContent("2 weeks");
  });

  it("hides the remove entry on an unsnoozed card", async () => {
    const user = userEvent.setup();
    renderMenu();
    await user.click(screen.getByTestId("snooze-trigger"));
    await screen.findByTestId("snooze-panel");
    expect(screen.queryByTestId("snooze-option-unsnooze")).not.toBeInTheDocument();
    expect(screen.queryByText(REMOVE_SNOOZE_ENTRY)).not.toBeInTheDocument();
  });

  it("offers the remove entry on a snoozed card", async () => {
    const user = userEvent.setup();
    const handlers = renderMenu({ snoozed: true });
    await user.click(screen.getByTestId("snooze-trigger"));
    await user.click(await screen.findByTestId("snooze-option-unsnooze"));
    expect(handlers.onRemove).toHaveBeenCalledTimes(1);
  });

  it("hands a preset pick to the write in order", async () => {
    const user = userEvent.setup();
    const handlers = renderMenu();
    await user.click(screen.getByTestId("snooze-trigger"));
    await user.click(await screen.findByTestId("snooze-option-day-3"));
    expect(handlers.onPreset).toHaveBeenCalledWith(SNOOZE_PRESETS[1]);
  });

  it("closes after a custom pick so the dialog can open", async () => {
    const user = userEvent.setup();
    const handlers = renderMenu();
    await user.click(screen.getByTestId("snooze-trigger"));
    await user.click(await screen.findByTestId("snooze-option-custom"));
    expect(handlers.onCustom).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByTestId("snooze-panel")).not.toBeInTheDocument());
  });

  it("keeps rows inert while a write is in flight", () => {
    const handlers = renderMenu({ busy: true });
    expect(screen.getByTestId("snooze-trigger")).toBeDisabled();
    expect(handlers.onPreset).not.toHaveBeenCalled();
    expect(handlers.onCustom).not.toHaveBeenCalled();
  });
});
