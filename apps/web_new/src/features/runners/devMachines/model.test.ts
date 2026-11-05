// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { DevMachine, Runner } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  DEV_MACHINE_STATUS_LABELS,
  MACHINE_ACTION_COPY,
  devMachineName,
  devMachineStatus,
  devMachineSubline,
  engineState,
  machineActions,
  machineIsLive,
  machineSettings,
  machineTime,
  mergedIntoText,
  replacementCandidate,
  runnersByMachine,
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

describe("engineState", () => {
  const fmt = () => "Nov 7";
  it("shows nothing when the server reports no engine field or the machine is revoked", () => {
    expect(engineState(machine())).toBeNull();
    expect(engineState(machine({ engine_paused_by_owner: true, revoked_at: "2026-01-01T00:00:00Z" }))).toBeNull();
  });

  it("ranks the owner's pause, then the automatic pause, then the sign-in state", () => {
    const paused = { engine_paused_by_owner: true, engine_auto_pause_reason: "wallet_empty" };
    expect(engineState(machine(paused))?.label).toBe("Engine paused by you");
    expect(engineState(machine({ ...paused, engine_paused_by_owner: false }))?.label).toBe(
      "Engine paused: wallet is empty"
    );
    expect(engineState(machine({ engine_auto_pause_reason: "brand_new" }))?.label).toBe("Engine paused: brand_new");
    expect(engineState(machine({ engine_paused_by_owner: false }))).toEqual({
      label: "Engine available",
      tone: "success",
    });
  });

  it("reads the sign-in state with its expiry", () => {
    const at = { engine_credential_expires_at: "2026-11-07T00:00:00Z" };
    expect(engineState(machine({ ...at, engine_sign_in_state: "expiring" }), fmt)?.label).toBe(
      "Engine expiring on Nov 7"
    );
    expect(engineState(machine({ engine_sign_in_state: "expiring" }))?.label).toBe("Engine sign-in expiring");
    expect(engineState(machine({ engine_sign_in_state: "signed_out" }))?.label).toBe("Engine signed out");
    expect(engineState(machine({ engine_sign_in_state: "expired" }))?.label).toBe("Engine sign-in expired");
    expect(engineState(machine({ engine_sign_in_state: "never" }))?.label).toBe("Engine not signed in");
    expect(engineState(machine({ engine_sign_in_state: "active" }))?.label).toBe("Engine available");
  });
});

describe("machineSettings", () => {
  it("offers each engine control only when its field is reported", () => {
    expect(machineSettings(machine())).toEqual({ rename: true, pause: false, teamRuns: false, signOutEngine: false });
    expect(
      machineSettings(
        machine({ engine_paused_by_owner: false, engine_team_runs: true, engine_sign_in_state: "active" })
      )
    ).toEqual({ rename: true, pause: true, teamRuns: true, signOutEngine: true });
    expect(machineSettings(machine({ engine_sign_in_state: "signed_out" })).signOutEngine).toBe(false);
    expect(machineSettings(machine({ revoked_at: "2026-01-01T00:00:00Z", engine_paused_by_owner: false }))).toEqual({
      rename: false,
      pause: false,
      teamRuns: false,
      signOutEngine: false,
    });
  });
});

describe("replacementCandidate", () => {
  const live = machine({ id: "new", host_label: "box", control_online: true });
  it("pairs an offline machine with a live one of the same hostname", () => {
    const old = machine({ id: "old", host_label: "box" });
    expect(replacementCandidate(old, [old, live])?.id).toBe("new");
    expect(machineIsLive(live)).toBe(true);
  });

  it("skips live, revoked, merged and hostname-less machines and other hosts", () => {
    const others = [live, machine({ id: "far", host_label: "other", control_online: true })];
    expect(replacementCandidate(live, others)).toBeNull();
    expect(replacementCandidate(machine({ id: "a", host_label: "box", revoked_at: "x" }), others)).toBeNull();
    expect(replacementCandidate(machine({ id: "b", host_label: "box", merged_into: "new" }), others)).toBeNull();
    expect(replacementCandidate(machine({ id: "c", host_label: "" }), others)).toBeNull();
    expect(replacementCandidate(machine({ id: "d", host_label: "lonely" }), others)).toBeNull();
  });
});

describe("mergedIntoText", () => {
  it("names the target from the list, else its short id", () => {
    const rows = [machine({ id: "new-machine-id", label: "new box" })];
    expect(mergedIntoText(machine({ merged_into: "new-machine-id" }), rows)).toBe("Merged into new box");
    expect(mergedIntoText(machine({ merged_into: "abcdef0123" }), rows)).toBe("Merged into abcdef01");
    expect(mergedIntoText(machine(), rows)).toBeNull();
  });
});

describe("runnersByMachine", () => {
  it("groups runners by machine, engines apart from own agents", () => {
    const rows = [
      { id: "e", dev_machine: "m1", provisioning: "desktop_bundled" },
      { id: "a", dev_machine: "m1", provisioning: "manual" },
      { id: "z", dev_machine: null, provisioning: "manual" },
    ] as unknown as Runner[];
    const grouped = runnersByMachine(rows);
    expect(grouped.get("m1")?.engines.map((row) => row.id)).toEqual(["e"]);
    expect(grouped.get("m1")?.ownAgents.map((row) => row.id)).toEqual(["a"]);
    expect(grouped.size).toBe(1);
  });
});
