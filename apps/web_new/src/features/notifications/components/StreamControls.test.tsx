// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DEFAULT_STREAM_MODES } from "../lib/stream.js";
import { StreamControls, type StreamControlsProps } from "./StreamControls.js";

async function openMenu(user: ReturnType<typeof userEvent.setup>, name: string): Promise<void> {
  const trigger = screen.getByRole("button", { name });
  trigger.focus();
  await user.keyboard("{ArrowDown}");
  expect(screen.getByRole("menu", { name })).toBeInTheDocument();
}

function setup(overrides: Partial<StreamControlsProps> = {}): {
  props: StreamControlsProps;
} {
  const props: StreamControlsProps = {
    origins: [],
    modes: DEFAULT_STREAM_MODES,
    refreshing: false,
    markingRead: false,
    onRefresh: vi.fn(),
    onMarkAllRead: vi.fn(),
    onToggleOrigin: vi.fn(),
    onToggleMode: vi.fn(),
    ...overrides,
  };
  render(<StreamControls {...props} />);
  return { props };
}

describe("StreamControls refresh (NTF-013)", () => {
  it("refreshes on click and shows no progress at rest", () => {
    const { props } = setup();
    const refresh = screen.getByRole("button", { name: "Refresh" });
    expect(refresh).not.toHaveAttribute("aria-busy");
    expect(screen.queryByTestId("inbox-refresh-progress")).not.toBeInTheDocument();
    fireEvent.click(refresh);
    expect(props.onRefresh).toHaveBeenCalledTimes(1);
  });

  it("shows progress and refuses a repeat press while one runs", () => {
    const { props } = setup({ refreshing: true });
    const refresh = screen.getByRole("button", { name: "Refresh" });
    expect(refresh).toHaveAttribute("aria-busy", "true");
    expect(refresh).toBeDisabled();
    expect(screen.getByTestId("inbox-refresh-progress")).toBeInTheDocument();
    fireEvent.click(refresh);
    expect(props.onRefresh).not.toHaveBeenCalled();
  });
});

describe("StreamControls mark-all-read (NTF-014)", () => {
  it("marks all read on click", () => {
    const { props } = setup();
    const button = screen.getByRole("button", { name: "Mark all as read" });
    expect(button).not.toHaveAttribute("aria-busy");
    fireEvent.click(button);
    expect(props.onMarkAllRead).toHaveBeenCalledTimes(1);
  });

  it("shows progress and refuses a repeat press while one runs", () => {
    const { props } = setup({ markingRead: true });
    const button = screen.getByRole("button", { name: "Mark all as read" });
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).toBeDisabled();
    expect(screen.getByTestId("inbox-mark-all-read-progress")).toBeInTheDocument();
    fireEvent.click(button);
    expect(props.onMarkAllRead).not.toHaveBeenCalled();
  });
});

describe("StreamControls origin filter (NTF-015)", () => {
  it("lists the three origins with checkmarks", async () => {
    const user = userEvent.setup();
    setup({ origins: ["created"] });
    await openMenu(user, "Filter by origin");
    const options = screen.getAllByRole("menuitemcheckbox");
    expect(options.map((option) => option.getAttribute("data-origin"))).toEqual(["assigned", "created", "subscribed"]);
    expect(options.map((option) => option.getAttribute("aria-checked"))).toEqual(["false", "true", "false"]);
    for (const option of options) expect(option.textContent).not.toBe("");
  });

  it("draws a check only on the active origins", async () => {
    const user = userEvent.setup();
    setup({ origins: ["assigned", "subscribed"] });
    await openMenu(user, "Filter by origin");
    const checks = screen.getAllByTestId("stream-control-check").map((slot) => slot.getAttribute("data-checked"));
    expect(checks).toEqual(["true", "false", "true"]);
  });

  it("toggles the clicked origin and keeps the menu open for more picks", async () => {
    const user = userEvent.setup();
    const { props } = setup();
    await openMenu(user, "Filter by origin");
    fireEvent.click(screen.getByTestId("inbox-filter-origin-assigned"));
    console.log("DEBUG state", screen.getByTestId("inbox-filter-origin-assigned").closest("[role=menu]") !== null);
    console.log("DEBUG calls", (props.onToggleOrigin as any).mock.calls);
    expect(props.onToggleOrigin).toHaveBeenCalledWith("assigned");
    expect(screen.getByTestId("inbox-filter-origin-created")).toBeInTheDocument();
  });
});

describe("StreamControls overflow modes (NTF-016)", () => {
  it("lists the three modes in order with their active state", async () => {
    const user = userEvent.setup();
    setup({ modes: { unreadOnly: true, archived: false, snoozed: true } });
    await openMenu(user, "More options");
    const modes = screen.getAllByTestId(/^inbox-mode-/);
    expect(modes.map((mode) => mode.getAttribute("data-mode"))).toEqual(["unread", "archived", "snoozed"]);
    expect(modes.map((mode) => within(mode).getByTestId("stream-control-check").getAttribute("data-checked"))).toEqual([
      "true",
      "false",
      "true",
    ]);
  });

  it("toggles the selected mode", async () => {
    const user = userEvent.setup();
    const { props } = setup();
    await openMenu(user, "More options");
    fireEvent.click(screen.getByTestId("inbox-mode-archived"));
    expect(props.onToggleMode).toHaveBeenCalledWith("archived");
  });
});

describe("StreamControls wiring", () => {
  it("calls no handler until the user acts", () => {
    const { props } = setup();
    expect(props.onRefresh).not.toHaveBeenCalled();
    expect(props.onMarkAllRead).not.toHaveBeenCalled();
    expect(props.onToggleOrigin).not.toHaveBeenCalled();
    expect(props.onToggleMode).not.toHaveBeenCalled();
  });
});
