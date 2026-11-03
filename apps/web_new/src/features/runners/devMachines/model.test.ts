// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { DevMachine } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  DEV_MACHINE_STATUS_LABELS,
  MACHINE_ACTION_COPY,
  devMachineName,
  devMachineStatus,
  devMachineSubline,
  machineActions,
  machineTime,
  runnerCountLabel,
  shortMachineId,
  type DevMachineStatus,
} from "./model.js";

function machine(overrides: Partial<DevMachine> = {}): DevMachine {
  return {
    id: "12345678-aaaa-bbbb-cccc-dddddddddddd",
    host_label: "host-a",
    label: "build box",
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

describe("devMachineStatus", () => {
  const cases: [Partial<DevMachine>, DevMachineStatus][] = [
    [{ revoked_at: "2026-01-01T00:00:00Z", online_runner_count: 5, runner_count: 5 }, "revoked"],
    [{ online_runner_count: 1 }, "active"],
    [{ runner_count: 1 }, "offline"],
    [{}, "registered"],
  ];
  it.each(cases)("derives %o", (overrides, expected) => {
    expect(devMachineStatus(machine(overrides))).toBe(expected);
    expect(DEV_MACHINE_STATUS_LABELS[expected]).toBeTruthy();
  });
});

describe("machine naming", () => {
  it("uses the label, then the host, then the short id", () => {
    expect(devMachineName(machine())).toBe("build box");
    expect(devMachineName(machine({ label: "" }))).toBe("host-a");
    expect(devMachineName(machine({ label: "", host_label: "" }))).toBe("12345678");
    expect(shortMachineId("abcdefghij")).toBe("abcdefgh");
  });

  it("renders the host as subline only when it differs from the name", () => {
    expect(devMachineSubline(machine())).toBe("host-a");
    expect(devMachineSubline(machine({ host_label: "build box" }))).toBe("id 12345678");
    expect(devMachineSubline(machine({ label: "" }))).toBe("id 12345678");
  });
});

describe("runnerCountLabel", () => {
  it("renders the counts sentence", () => {
    expect(runnerCountLabel(machine({ online_runner_count: 2, runner_count: 3 }))).toBe("2 active / 3 total");
  });
});

describe("machineTime", () => {
  it("returns Never for a missing stamp", () => {
    expect(machineTime(null)).toBe("Never");
  });

  it("formats a present stamp with the injected formatter", () => {
    expect(machineTime("2026-01-01T00:00:00Z", () => "stamp")).toBe("stamp");
  });

  it("falls back to the locale formatter", () => {
    expect(machineTime("2026-01-01T00:00:00Z")).toBe(new Date("2026-01-01T00:00:00Z").toLocaleString());
  });
});

describe("machineActions", () => {
  it("offers all three before revocation and only delete after", () => {
    expect(machineActions(machine())).toEqual(["rotate", "revoke", "delete"]);
    expect(machineActions(machine({ revoked_at: "2026-01-01T00:00:00Z" }))).toEqual(["delete"]);
  });

  it("carries confirmation copy for every action", () => {
    for (const action of ["rotate", "revoke", "delete"] as const) {
      expect(MACHINE_ACTION_COPY[action].title).toContain("?");
      expect(MACHINE_ACTION_COPY[action].body.length).toBeGreaterThan(0);
    }
  });
});
