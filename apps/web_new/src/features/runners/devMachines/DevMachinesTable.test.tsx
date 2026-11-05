// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { DevMachine, Runner } from "@pidash/api-client";
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  DEV_MACHINES_EMPTY,
  DEV_MACHINES_FAILED,
  DEV_MACHINES_LOADING,
  DevMachinesTable,
  MACHINE_DETAILS_HIDE,
  MACHINE_DETAILS_SHOW,
} from "./DevMachinesTable.js";

function runner(
  id: string,
  machineId: string,
  provisioning: Runner["provisioning"],
  project: string,
  name = `runner-${id}`
): Runner {
  return {
    id,
    name,
    status: "online",
    host_label: "",
    provisioning,
    os: "",
    arch: "",
    runner_version: "",
    dev_metadata: {},
    protocol_version: 4,
    capabilities: [],
    last_heartbeat_at: null,
    owner: "owner",
    dev_machine: machineId,
    dev_machine_detail: null,
    visibility: 0,
    pod: "pod",
    pod_detail: { id: "pod", name: `${project}-pod`, is_default: true, project: "p", project_identifier: project },
    live_state: null,
    enrolled_at: "2026-01-01T00:00:00Z",
    revoked_at: null,
    revoked_reason: "",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

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
    expect(screen.getAllByTestId("dev-machine-status-badge")).toHaveLength(4);
    // Three non-revoked machines carry three actions each, the revoked one delete.
    expect(screen.getAllByRole("button", { name: "Rotate" })).toHaveLength(3);
    expect(screen.getAllByRole("button", { name: "Revoke" })).toHaveLength(3);
    expect(screen.getAllByRole("button", { name: "Remove this computer" })).toHaveLength(4);
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

  it("shows the engine state only when the server reports engine fields", () => {
    setup({
      machines: [
        machine("plain"),
        machine("paused", { engine_paused_by_owner: true, engine_auto_pause_reason: "" }),
        machine("wallet", { engine_paused_by_owner: false, engine_auto_pause_reason: "wallet_empty" }),
      ],
    });
    const states = screen.getAllByTestId("dev-machine-engine-state").map((node) => node.textContent);
    expect(states).toEqual(["Engine paused by you", "Engine paused: wallet is empty"]);
  });

  it("expands a row into its engine and own-agent runners", () => {
    const runners = [
      runner("e1", "one", "desktop_bundled", "PROJ"),
      runner("a1", "one", "manual", "WEB", "claude-box"),
      runner("x1", "other", "manual", "WEB", "elsewhere"),
    ];
    setup({ runners });
    expect(screen.queryByTestId("dev-machine-details")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: MACHINE_DETAILS_SHOW }));
    const details = screen.getByTestId("dev-machine-details");
    expect(within(details).getByRole("region", { name: "Built-in engine" })).toHaveTextContent("PROJ");
    expect(within(details).getByRole("region", { name: "Own-agent runners" })).toHaveTextContent("claude-box · WEB");
    expect(details).not.toHaveTextContent("elsewhere");
    // An older server reports no engine controls, so only rename is offered.
    expect(within(details).getByRole("button", { name: "Rename" })).toBeInTheDocument();
    expect(within(details).queryByRole("button", { name: "Pause the built-in engine" })).not.toBeInTheDocument();
    expect(within(details).queryByRole("switch")).not.toBeInTheDocument();
    expect(within(details).queryByRole("button", { name: "Sign out the engine" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: MACHINE_DETAILS_HIDE }));
    expect(screen.queryByTestId("dev-machine-details")).not.toBeInTheDocument();
  });

  it("requests engine settings from an expanded row", () => {
    const onSetting = vi.fn();
    const engine: Partial<DevMachine> = {
      engine_paused_by_owner: true,
      engine_auto_pause_reason: "",
      engine_team_runs: false,
      engine_sign_in_state: "expiring",
    };
    setup({ machines: [machine("one", engine)], onSetting, runners: [] });
    fireEvent.click(screen.getByRole("button", { name: MACHINE_DETAILS_SHOW }));
    fireEvent.click(screen.getByRole("button", { name: "Resume the built-in engine" }));
    expect(onSetting).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "pause", paused: false }));
    fireEvent.click(screen.getByRole("switch"));
    expect(onSetting).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "team-runs", enabled: true }));
    fireEvent.click(screen.getByRole("button", { name: "Sign out the engine" }));
    expect(onSetting).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "sign-out-engine" }));
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    expect(onSetting).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "rename" }));
  });

  it("offers Replaced by for an offline twin of a live machine and names a merge", () => {
    const onSetting = vi.fn();
    setup({
      onSetting,
      runners: [],
      machines: [
        machine("old", { host_label: "box", runner_count: 1 }),
        machine("new", { host_label: "box", control_online: true }),
        machine("gone", { host_label: "box", revoked_at: "2026-01-01T00:00:00Z", merged_into: "new" }),
      ],
    });
    expect(screen.getByText("Merged into machine new")).toBeInTheDocument();
    const [oldToggle, newToggle] = screen.getAllByRole("button", { name: MACHINE_DETAILS_SHOW });
    fireEvent.click(newToggle as HTMLElement);
    expect(screen.queryByRole("button", { name: /Replaced by/ })).not.toBeInTheDocument();
    fireEvent.click(oldToggle as HTMLElement);
    fireEvent.click(screen.getByRole("button", { name: "Replaced by machine new" }));
    expect(onSetting).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: "replace",
        machine: expect.objectContaining({ id: "old" }),
        target: expect.objectContaining({ id: "new" }),
      })
    );
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
