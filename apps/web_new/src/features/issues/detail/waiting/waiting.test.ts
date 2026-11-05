// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { AgentRunSummary, DispatchWait, Issue } from "@pidash/api-client";
import {
  MACHINE_WAIT_BADGE,
  UNKNOWN_MACHINE_NAME,
  machineWaitBannerText,
  machineWaitLabel,
  machineWaitName,
  machineWaitPause,
  selectMachineWait,
  selectWaitingBadge,
  selectWaitingRun,
  waitingBadgeLabel,
  waitingBannerText,
  waitingReasonText,
} from "./waiting.js";

const WAIT = {
  reason: "runner_offline",
  pin_kind: "dedicated",
  runner: "223e4567-e89b-12d3-a456-426614174001",
  runner_name: "gpu-box",
  queued_at: "2026-10-11T20:14:03Z",
  runner_last_seen_at: "2026-10-22T02:00:00Z",
  runs_ahead: 0,
  releasable: true,
} as DispatchWait;

function runSummary(overrides: Partial<AgentRunSummary> = {}): AgentRunSummary {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    status: "queued",
    executor_kind: "local_runner",
    queue_position: null,
    runner: null,
    runner_name: null,
    pinned_runner: "223e4567-e89b-12d3-a456-426614174001",
    pinned_runner_name: "gpu-box",
    pin_kind: "dedicated",
    dispatch_wait: WAIT,
    created_at: "2026-10-11T20:14:03Z",
    assigned_at: null,
    started_at: null,
    ended_at: null,
    done_payload: null,
    error: "",
    error_code: "",
    error_diagnostic: null,
    llm_model: "",
    input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    live_state: null,
    ...overrides,
  };
}

function issueWith(activeRun: AgentRunSummary | null): Pick<Issue, "agent_status"> {
  return {
    agent_status:
      activeRun === null ? null : { ticker: null, active_run: activeRun, latest_run: activeRun, run_count: 1 },
  };
}

describe("selectWaitingRun", () => {
  it("selects a queued pinned run with its wait", () => {
    const waiting = selectWaitingRun(issueWith(runSummary()));
    expect(waiting?.run.id).toBe("123e4567-e89b-12d3-a456-426614174000");
    expect(waiting?.wait.reason).toBe("runner_offline");
    expect(waiting?.runnerName).toBe("gpu-box");
  });

  it("falls back to the wait's runner name without a pinned name", () => {
    const waiting = selectWaitingRun(issueWith(runSummary({ pinned_runner_name: null })));
    expect(waiting?.runnerName).toBe("gpu-box");
  });

  it("reads nothing waiting without a queued pinned run", () => {
    expect(selectWaitingRun({ agent_status: null })).toBe(null);
    expect(selectWaitingRun(issueWith(null))).toBe(null);
    expect(selectWaitingRun(issueWith(runSummary({ status: "running" })))).toBe(null);
    expect(selectWaitingRun(issueWith(runSummary({ dispatch_wait: null })))).toBe(null);
    expect(selectWaitingRun(issueWith(runSummary({ dispatch_wait: undefined })))).toBe(null);
  });
});

describe("waitingReasonText", () => {
  const now = Date.parse("2026-10-22T04:00:00Z");

  it("names offline heartbeats, busy queues and the other reasons", () => {
    expect(waitingReasonText(WAIT, now)).toBe("offline, last seen 2 h ago");
    expect(waitingReasonText({ ...WAIT, runner_last_seen_at: null }, now)).toBe("offline");
    expect(waitingReasonText({ ...WAIT, reason: "runner_busy", runs_ahead: 3 }, now)).toBe("busy, 3 runs ahead");
    expect(waitingReasonText({ ...WAIT, reason: "runner_busy", runs_ahead: 1 }, now)).toBe("busy, 1 run ahead");
    expect(waitingReasonText({ ...WAIT, reason: "runner_busy", runs_ahead: 0 }, now)).toBe("busy");
    expect(waitingReasonText({ ...WAIT, reason: "runner_not_permitted" }, now)).toBe(
      "the runner can't take this issue"
    );
    expect(waitingReasonText({ ...WAIT, reason: "dispatching" }, now)).toBe("dispatching");
  });

  it("reads a paused runner as paused, with the reason, not offline", () => {
    expect(waitingReasonText({ ...WAIT, reason: "runner_paused", pause_reason: "wallet_empty" }, now)).toBe(
      "paused: wallet is empty"
    );
    expect(waitingReasonText({ ...WAIT, reason: "runner_paused", pause_reason: "" }, now)).toBe(
      "paused: paused by its owner"
    );
    expect(waitingReasonText({ ...WAIT, reason: "runner_paused" }, now)).toBe("paused: paused by its owner");
    expect(waitingBadgeLabel({ ...WAIT, reason: "runner_paused" })).toBe("Paused");
  });

  it("surfaces unknown reasons as-is", () => {
    expect(waitingReasonText({ ...WAIT, reason: "tomorrow_problem" }, now)).toBe("tomorrow_problem");
  });
});

describe("waitingBannerText", () => {
  it("names the runner and the reason", () => {
    expect(waitingBannerText("gpu-box", WAIT, Date.parse("2026-10-22T04:00:00Z"))).toBe(
      "Waiting for runner gpu-box — offline, last seen 2 h ago."
    );
  });
});

describe("waitingBadgeLabel", () => {
  it("compacts every reason", () => {
    expect(waitingBadgeLabel(WAIT)).toBe("Offline");
    expect(waitingBadgeLabel({ ...WAIT, reason: "runner_busy", runs_ahead: 2 })).toBe("2 ahead");
    expect(waitingBadgeLabel({ ...WAIT, reason: "runner_busy", runs_ahead: 0 })).toBe("Busy");
    expect(waitingBadgeLabel({ ...WAIT, reason: "runner_not_permitted" })).toBe("Not permitted");
    expect(waitingBadgeLabel({ ...WAIT, reason: "dispatching" })).toBe("Dispatching");
    expect(waitingBadgeLabel({ ...WAIT, reason: "tomorrow_problem" })).toBe("tomorrow_problem");
  });
});

describe("selectWaitingBadge", () => {
  it("badges a waiting issue and clears a quiet one", () => {
    expect(selectWaitingBadge(issueWith(runSummary()))).toBe("Offline");
    expect(selectWaitingBadge(issueWith(null))).toBeUndefined();
  });

  it("badges a run waiting for an offline computer", () => {
    const machine = runSummary({ dispatch_wait: undefined, error_code: "desktop_not_connected" });
    expect(selectWaitingBadge(issueWith(machine))).toBe(MACHINE_WAIT_BADGE);
  });
});

describe("machine waits (built-in engine design §9)", () => {
  const RUNNER_ID = "223e4567-e89b-12d3-a456-426614174001";
  const engine = {
    id: RUNNER_ID,
    name: "builtin-x",
    provisioning: "desktop_bundled",
    dev_machine_detail: { id: "m", label: "Work laptop", host_label: "rich-mbp" },
  };

  it("reads a queued run carrying the waiting code, preferring the issue's runner", () => {
    const run = runSummary({ dispatch_wait: null, error_code: "desktop_not_connected" });
    expect(selectMachineWait({ ...issueWith(run), assigned_runner_id: "other" })?.runnerId).toBe("other");
    expect(selectMachineWait(issueWith(run))?.runnerId).toBe(RUNNER_ID);
    expect(
      selectMachineWait(
        issueWith(runSummary({ pinned_runner: undefined, dispatch_wait: null, error_code: "desktop_not_connected" }))
      )?.runnerId
    ).toBe(null);
  });

  it("ignores runs with a computed wait, other codes or other states", () => {
    expect(selectMachineWait(issueWith(runSummary({ error_code: "desktop_not_connected" })))).toBeNull();
    expect(selectMachineWait(issueWith(runSummary({ dispatch_wait: null })))).toBeNull();
    expect(
      selectMachineWait(
        issueWith(runSummary({ dispatch_wait: null, status: "running", error_code: "desktop_not_connected" }))
      )
    ).toBeNull();
    expect(selectMachineWait(issueWith(null))).toBeNull();
  });

  it("names the machine, then the runner, then a neutral fallback", () => {
    const wait = { run: runSummary(), runnerId: RUNNER_ID };
    expect(machineWaitName(wait, [engine])).toBe("Work laptop");
    expect(
      machineWaitName(wait, [
        { ...engine, dev_machine_detail: null, host_label: "", provisioning: "manual", name: "gpu-box" },
      ])
    ).toBe("gpu-box");
    expect(machineWaitName(wait, [])).toBe(UNKNOWN_MACHINE_NAME);
    expect(machineWaitName({ ...wait, runnerId: null }, [engine])).toBe(UNKNOWN_MACHINE_NAME);
    expect(machineWaitName(wait, undefined)).toBe(UNKNOWN_MACHINE_NAME);
  });

  it("says Waiting for <machine> and that it is offline", () => {
    expect(machineWaitLabel("Work laptop")).toBe("Waiting for Work laptop");
    expect(machineWaitBannerText("Work laptop")).toContain("Waiting for Work laptop — Work laptop is offline.");
    expect(machineWaitBannerText(UNKNOWN_MACHINE_NAME)).toContain("its computer is offline");
  });

  it("tells a paused engine apart from an offline computer", () => {
    const wait = { run: runSummary(), runnerId: RUNNER_ID };
    expect(machineWaitPause(wait, [engine])).toBeNull();
    expect(machineWaitPause(wait, [{ ...engine, paused_at: "2026-11-05T00:00:00Z" }])).toBe("paused by its owner");
    const machine = engine.dev_machine_detail;
    expect(
      machineWaitPause(wait, [
        { ...engine, dev_machine_detail: { ...machine, engine_auto_pause_reason: "wallet_empty" } },
      ])
    ).toBe("wallet is empty");
    expect(
      machineWaitPause(wait, [{ ...engine, dev_machine_detail: { ...machine, engine_paused_by_owner: true } }])
    ).toBe("paused by its owner");
    expect(
      machineWaitPause(wait, [
        {
          ...engine,
          provisioning: "manual",
          dev_machine_detail: { ...machine, engine_paused_by_owner: true },
        },
      ])
    ).toBeNull();
    expect(machineWaitPause(wait, undefined)).toBeNull();
    expect(machineWaitBannerText("Work laptop", "wallet is empty")).toBe(
      "Waiting for Work laptop: wallet is empty. The run starts when it is resumed."
    );
  });
});
