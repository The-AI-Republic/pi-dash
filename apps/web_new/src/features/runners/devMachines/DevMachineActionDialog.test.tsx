// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DevMachineActionDialog } from "./DevMachineActionDialog.js";
import { MACHINE_ACTION_COPY } from "./model.js";

function setup(overrides: Partial<React.ComponentProps<typeof DevMachineActionDialog>> = {}) {
  const handlers = { onCancel: vi.fn(), onConfirm: vi.fn() };
  const view = render(
    <DevMachineActionDialog open action="delete" machineName="build box" pending={false} {...handlers} {...overrides} />
  );
  return { ...handlers, ...view };
}

describe("DevMachineActionDialog", () => {
  it("renders the action's title, body, machine and confirm label", () => {
    setup({ action: "rotate", pending: false });
    expect(screen.getByRole("heading", { name: MACHINE_ACTION_COPY.rotate.title })).toBeInTheDocument();
    expect(screen.getByText(MACHINE_ACTION_COPY.rotate.body)).toBeInTheDocument();
    expect(screen.getByText("build box")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Rotate" })).toBeInTheDocument();
  });

  it("confirms and cancels when idle", () => {
    const { onCancel, onConfirm } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onCancel).toHaveBeenCalled();
    expect(onConfirm).toHaveBeenCalled();
  });

  it("is inert while a write is in flight", () => {
    const { onCancel, onConfirm } = setup({ pending: true });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onCancel).not.toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
  });

  it("cancels on a dismiss when idle and ignores one in flight", () => {
    const idle = setup();
    fireEvent.keyDown(document.body, { key: "Escape" });
    fireEvent.click(document.querySelector('[aria-label="Close dialog"]') as HTMLElement);
    expect(idle.onCancel).toHaveBeenCalled();

    idle.unmount();
    const busy = setup({ pending: true });
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
    expect(busy.onCancel).not.toHaveBeenCalled();
  });

  it("renders nothing when closed", () => {
    setup({ open: false });
    expect(screen.queryByRole("button", { name: "Delete" })).not.toBeInTheDocument();
  });
});
