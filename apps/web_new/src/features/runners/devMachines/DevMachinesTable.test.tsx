// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { DevMachine } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DEV_MACHINES_EMPTY, DEV_MACHINES_FAILED, DEV_MACHINES_LOADING, DevMachinesTable } from "./DevMachinesTable.js";

function machine(id: string, overrides: Partial<DevMachine> = {}): DevMachine {
  return {
    id,
    host_label: `host-${id}`,
    label: `machine ${id}`,
    visibility: 0,
    runner_count: 0,
    online_runner_count: 0,
    control_online: false,
    last_seen_at: null,
    last_heartbeat_at: null,
    revoked_at: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function setup(overrides: Partial<React.ComponentProps<typeof DevMachinesTable>> = {}) {
  const onAction = vi.fn();
  render(
    <DevMachinesTable machines={[machine("one")]} loading={false} failed={false} onAction={onAction} {...overrides} />
  );
  return { onAction };
}

describe("DevMachinesTable", () => {
  it("renders one row per machine with status, counts and actions", () => {
    setup({
      machines: [
        machine("active", { online_runner_count: 1, runner_count: 1 }),
        machine("offline", { runner_count: 1 }),
        machine("registered"),
        machine("revoked", { revoked_at: "2026-01-01T00:00:00Z" }),
      ],
    });
    expect(screen.getByText("machine active")).toBeInTheDocument();
    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(screen.getByText("1 active / 1 total")).toBeInTheDocument();
    expect(screen.getByText("Offline")).toBeInTheDocument();
    expect(screen.getByText("Registered")).toBeInTheDocument();
    expect(screen.getByText("Revoked")).toBeInTheDocument();
    // Three non-revoked machines carry three actions each, the revoked one delete.
    expect(screen.getAllByRole("button", { name: "Rotate" })).toHaveLength(3);
    expect(screen.getAllByRole("button", { name: "Revoke" })).toHaveLength(3);
    expect(screen.getAllByRole("button", { name: "Delete" })).toHaveLength(4);
  });

  it("hands the action and its machine to the caller", () => {
    const { onAction } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Rotate" }));
    expect(onAction).toHaveBeenCalledWith("rotate", expect.objectContaining({ id: "one" }));
  });

  it("shows Never for missing timestamps and formatted ones otherwise", () => {
    setup({
      machines: [machine("one", { last_seen_at: "2026-01-01T00:00:00Z", last_heartbeat_at: null })],
      formatTime: () => "Jan 1",
    });
    expect(screen.getByText("Jan 1")).toBeInTheDocument();
    expect(screen.getByText("Never")).toBeInTheDocument();
  });

  it("renders the loading, failed and empty states", () => {
    const props = { machines: undefined, loading: true, failed: false, onAction: vi.fn() };
    const { rerender } = render(<DevMachinesTable {...props} />);
    expect(screen.getByText(DEV_MACHINES_LOADING)).toBeInTheDocument();

    rerender(<DevMachinesTable {...props} loading={false} failed />);
    expect(screen.getByText(DEV_MACHINES_FAILED)).toBeInTheDocument();

    rerender(<DevMachinesTable {...props} loading={false} failed={false} machines={[]} />);
    expect(screen.getByText(DEV_MACHINES_EMPTY)).toBeInTheDocument();
  });
});
