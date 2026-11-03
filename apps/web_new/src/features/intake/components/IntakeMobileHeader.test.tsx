// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Narrow-viewport request header (INT-030): the list toggle with its
// expanded state, the neighbour chevrons, the status chip, and the
// always-present overflow menu.

import { INTAKE_STATUS } from "@pidash/api-client";
import type { MenuEntry } from "@pidash/kit";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { NEXT_REQUEST_LABEL, PREV_REQUEST_LABEL, REQUEST_ACTIONS_LABEL } from "./IntakeDetailHeader.js";
import { MOBILE_LIST_TOGGLE_LABEL, IntakeMobileHeader } from "./IntakeMobileHeader.js";

function renderHeader(props: Partial<React.ComponentProps<typeof IntakeMobileHeader>> = {}) {
  const onToggleList = vi.fn();
  const onPrev = vi.fn();
  const onNext = vi.fn();
  const onSnooze = vi.fn();
  const menuItems: MenuEntry[] = [{ label: "Snooze", onSelect: onSnooze }];
  render(
    <IntakeMobileHeader
      listOpen
      onToggleList={onToggleList}
      onPrev={onPrev}
      onNext={onNext}
      menuItems={menuItems}
      {...props}
    />
  );
  return { onToggleList, onPrev, onNext, onSnooze };
}

describe("IntakeMobileHeader (INT-030)", () => {
  it("toggles the list and reports the expanded state", () => {
    const { onToggleList } = renderHeader({ listOpen: true });
    const toggle = screen.getByRole("button", { name: MOBILE_LIST_TOGGLE_LABEL });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(toggle);
    expect(onToggleList).toHaveBeenCalledTimes(1);
  });

  it("reports a closed list as collapsed", () => {
    renderHeader({ listOpen: false });
    expect(screen.getByRole("button", { name: MOBILE_LIST_TOGGLE_LABEL })).toHaveAttribute("aria-expanded", "false");
  });

  it("steps to the neighbours", () => {
    const { onPrev, onNext } = renderHeader();
    fireEvent.click(screen.getByRole("button", { name: PREV_REQUEST_LABEL }));
    fireEvent.click(screen.getByRole("button", { name: NEXT_REQUEST_LABEL }));
    expect(onPrev).toHaveBeenCalledTimes(1);
    expect(onNext).toHaveBeenCalledTimes(1);
  });

  it("shows the status chip once the status is known", () => {
    const { rerender } = render(
      <IntakeMobileHeader
        listOpen
        onToggleList={() => undefined}
        onPrev={() => undefined}
        onNext={() => undefined}
        menuItems={[]}
      />
    );
    expect(screen.queryByTestId("intake-status-chip")).not.toBeInTheDocument();
    rerender(
      <IntakeMobileHeader
        listOpen
        onToggleList={() => undefined}
        onPrev={() => undefined}
        onNext={() => undefined}
        menuItems={[]}
        status={INTAKE_STATUS.snoozed}
      />
    );
    expect(screen.getByTestId("intake-status-chip")).toBeInTheDocument();
  });

  it("fires the header's own menu entries", async () => {
    const user = userEvent.setup();
    const { onSnooze } = renderHeader();
    await user.click(screen.getByRole("button", { name: REQUEST_ACTIONS_LABEL }));
    await user.click(await screen.findByRole("menuitem", { name: "Snooze" }));
    expect(onSnooze).toHaveBeenCalledTimes(1);
  });

  it("keeps the menu trigger with no entries", () => {
    renderHeader({ menuItems: [] });
    expect(screen.getByTestId("intake-mobile-menu")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: REQUEST_ACTIONS_LABEL })).toBeInTheDocument();
  });
});
