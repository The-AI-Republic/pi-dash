// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { AgentExecutorOption, PodList, RunnerList, WorkspaceMemberList } from "@pidash/api-client";
import { agentReasonMessage } from "../lib/agentReasons.js";
import {
  buildExecutionTargets,
  executionSelectionId,
  executionTargetLabel,
  executionTargetPatch,
  formatAvailabilityReason,
  formatRunnerSeenAgo,
  parseExecutionSelectionId,
  podOptionLabel,
  runnerReasonMessage,
  runnerStatusLine,
  runnerWaitNote,
  selectDefaultPod,
  selectRunnerOwnerName,
  selectRunnerWaitNote,
  selectTriggerPodName,
  selectTriggerText,
} from "./pods.js";

const POD_A = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  name: "ACME-Backend",
  description: "",
  is_default: true,
  workspace: "323e4567-e89b-12d3-a456-426614174002",
  project: "223e4567-e89b-12d3-a456-426614174001",
  project_identifier: "ACME",
  created_by: null,
  runner_count: 1,
  created_at: "2026-01-02T03:04:05Z",
  updated_at: "2026-02-03T04:05:06Z",
};

const POD_B = { ...POD_A, id: "423e4567-e89b-12d3-a456-426614174003", name: "ACME-Tier", is_default: false };

function option(kind: string, available: boolean, reason_code: string): AgentExecutorOption {
  return { kind, available, reason_code };
}

describe("formatAvailabilityReason", () => {
  it("renders every known code as a sentence", () => {
    expect(formatAvailabilityReason("")).toBe("");
    expect(formatAvailabilityReason("cloud_agent_unavailable")).toContain("Cloud Agent");
    expect(formatAvailabilityReason("llm_config_missing")).toContain("model");
    // The code serves both agent rows, so its sentence must not claim the
    // refusal belongs to one of them.
    expect(formatAvailabilityReason("llm_config_missing")).not.toMatch(/built-in|Cloud Agent/);
    expect(formatAvailabilityReason("no_local_runner")).toContain("runner");
    expect(formatAvailabilityReason("managed_runner_disabled")).toContain("switched off");
    expect(formatAvailabilityReason("desktop_not_connected")).toContain("not connected");
    expect(formatAvailabilityReason("gateway_scopes_missing")).toContain("gateway");
    expect(formatAvailabilityReason("byok_not_supported_on_desktop")).toContain("provider key");
    expect(formatAvailabilityReason("no_managed_runner_for_project")).toContain("enrolled");
  });

  it("surfaces unknown codes as-is", () => {
    expect(formatAvailabilityReason("tomorrow_problem")).toBe("tomorrow_problem");
  });

  it("uses the same sentences as the status notice", () => {
    expect(formatAvailabilityReason("gateway_unavailable")).toBe(agentReasonMessage("gateway_unavailable"));
    expect(formatAvailabilityReason("desktop_session_required")).toBe(agentReasonMessage("desktop_session_required"));
  });
});

describe("buildExecutionTargets", () => {
  it("lists offered agent rows first, then pods in server order", () => {
    expect(
      buildExecutionTargets([POD_A, POD_B] as PodList, [
        option("cloud_agent", true, ""),
        option("local_runner", false, "no_local_runner"),
        option("managed_runner", false, "desktop_not_connected"),
      ])
    ).toEqual([
      { kind: "agent", executor: "cloud_agent", label: "Pi Dash Cloud Agent", available: true, reason: "" },
      {
        kind: "agent",
        executor: "managed_runner",
        label: "Pi Dash Agent (desktop)",
        available: false,
        reason: "This computer is not connected to the server yet. Sign in again through the desktop app to link it.",
      },
      { kind: "pod", podId: POD_A.id, label: "ACME-Backend", isDefault: true },
      { kind: "pod", podId: POD_B.id, label: "ACME-Tier", isDefault: false },
    ]);
  });

  it("renders no row for an agent the config does not offer", () => {
    expect(buildExecutionTargets([POD_A] as PodList, [option("managed_runner", true, "")])).toEqual([
      { kind: "agent", executor: "managed_runner", label: "Pi Dash Agent (desktop)", available: true, reason: "" },
      { kind: "pod", podId: POD_A.id, label: "ACME-Backend", isDefault: true },
    ]);
    expect(buildExecutionTargets([] as PodList, [])).toEqual([]);
  });
});

describe("selectDefaultPod", () => {
  it("picks the default pod, if one exists", () => {
    expect(selectDefaultPod([POD_B, POD_A] as PodList)).toMatchObject({ id: POD_A.id });
    expect(selectDefaultPod([POD_B] as PodList)).toBe(null);
    expect(selectDefaultPod([] as PodList)).toBe(null);
  });
});

describe("executionTargetLabel", () => {
  it("reads Runs on once a cloud agent is offered, else Pod", () => {
    expect(executionTargetLabel([option("cloud_agent", false, "llm_config_missing")])).toBe("Runs on");
    expect(executionTargetLabel([option("managed_runner", true, "")])).toBe("Pod");
    expect(executionTargetLabel([])).toBe("Pod");
  });
});

describe("executionTargetPatch", () => {
  it("pins the pod plus the local executor on a pod pick", () => {
    expect(executionTargetPatch({ type: "pod", podId: POD_B.id })).toEqual({
      assigned_pod_id: POD_B.id,
      assigned_runner_id: null,
      agent_executor: "local_runner",
    });
  });

  it("pins the runner inside its pod on a runner pick", () => {
    expect(executionTargetPatch({ type: "runner", runnerId: "runner-1", podId: POD_B.id })).toEqual({
      assigned_pod_id: POD_B.id,
      assigned_runner_id: "runner-1",
      agent_executor: "local_runner",
    });
  });

  it("pins the executor and clears the pod on an agent pick", () => {
    expect(executionTargetPatch({ type: "agent", executor: "cloud_agent" })).toEqual({
      assigned_pod_id: null,
      assigned_runner_id: null,
      agent_executor: "cloud_agent",
    });
    expect(executionTargetPatch({ type: "agent", executor: "managed_runner" })).toEqual({
      assigned_pod_id: null,
      assigned_runner_id: null,
      agent_executor: "managed_runner",
    });
  });
});

describe("execution selection ids", () => {
  it("round-trips selections through option ids", () => {
    expect(executionSelectionId(null)).toBe(null);
    expect(executionSelectionId({ type: "pod", podId: POD_A.id })).toBe(`pod:${POD_A.id}`);
    expect(executionSelectionId({ type: "agent", executor: "cloud_agent" })).toBe("agent:cloud_agent");
    expect(executionSelectionId({ type: "runner", runnerId: "runner-1", podId: POD_A.id })).toBe("runner:runner-1");
    expect(parseExecutionSelectionId(`pod:${POD_A.id}`)).toEqual({ type: "pod", podId: POD_A.id });
    expect(parseExecutionSelectionId("agent:managed_runner")).toEqual({ type: "agent", executor: "managed_runner" });
    expect(parseExecutionSelectionId("runner:runner-1", [{ id: "runner-1", pod: POD_A.id }])).toEqual({
      type: "runner",
      runnerId: "runner-1",
      podId: POD_A.id,
    });
  });

  it("keeps a stale runner id with an empty pod instead of throwing", () => {
    expect(parseExecutionSelectionId("runner:gone", [{ id: "runner-1", pod: POD_A.id }])).toEqual({
      type: "runner",
      runnerId: "gone",
      podId: "",
    });
  });
});

const RUNNER_BASE = {
  id: "623e4567-e89b-12d3-a456-426614174005",
  name: "gpu-box",
  status: "online",
  host_label: "ana-desktop",
  provisioning: "manual",
  os: "linux",
  arch: "x86_64",
  runner_version: "1.2.3",
  dev_metadata: {},
  protocol_version: 4,
  capabilities: [],
  owner: "523e4567-e89b-12d3-a456-426614174004",
  dev_machine: null,
  dev_machine_detail: null,
  visibility: 0,
  pod_detail: {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "ACME-Backend",
    is_default: true,
    project: "223e4567-e89b-12d3-a456-426614174001",
    project_identifier: "ACME",
  },
  live_state: null,
  enrolled_at: "2026-01-02T03:04:05Z",
  revoked_at: null,
  revoked_reason: "",
  created_at: "2026-01-02T03:04:05Z",
  updated_at: "2026-02-03T04:05:06Z",
};

function runner(overrides: Record<string, unknown> = {}) {
  return { ...RUNNER_BASE, ...overrides };
}

describe("runnerReasonMessage", () => {
  it("renders every eligibility code as a sentence", () => {
    expect(runnerReasonMessage("")).toBe("");
    expect(runnerReasonMessage("runner_revoked")).toContain("revoked");
    expect(runnerReasonMessage("runner_desktop_bundled")).toContain("Desktop");
    expect(runnerReasonMessage("runner_owner_not_on_issue")).toContain("assignee");
    expect(runnerReasonMessage("runner_wrong_project")).toContain("another project");
    expect(runnerReasonMessage("runner_pod_mismatch")).toContain("pod");
    expect(runnerReasonMessage("executor_not_local")).toContain("local runner");
    expect(runnerReasonMessage("active_run_in_progress")).toContain("in progress");
    expect(runnerReasonMessage("runner_not_found")).toContain("no longer available");
  });

  it("surfaces unknown codes as-is", () => {
    expect(runnerReasonMessage("tomorrow_problem")).toBe("tomorrow_problem");
  });
});

describe("formatRunnerSeenAgo", () => {
  const now = Date.parse("2026-10-22T04:00:00Z");

  it("compacts heartbeat ages", () => {
    expect(formatRunnerSeenAgo("2026-10-22T03:59:30Z", now)).toBe("just now");
    expect(formatRunnerSeenAgo("2026-10-22T03:30:00Z", now)).toBe("30 m ago");
    expect(formatRunnerSeenAgo("2026-10-22T02:00:00Z", now)).toBe("2 h ago");
    expect(formatRunnerSeenAgo("2026-10-20T04:00:00Z", now)).toBe("2 d ago");
    expect(formatRunnerSeenAgo("2026-10-08T04:00:00Z", now)).toBe("2 w ago");
    expect(formatRunnerSeenAgo("2026-08-22T04:00:00Z", now)).toBe("2 mo ago");
    expect(formatRunnerSeenAgo("2024-10-22T04:00:00Z", now)).toBe("2 y ago");
  });

  it("returns null for unparseable stamps and clamps the future", () => {
    expect(formatRunnerSeenAgo("not-a-date", now)).toBe(null);
    expect(formatRunnerSeenAgo("2026-10-22T05:00:00Z", now)).toBe("just now");
  });
});

describe("runnerStatusLine", () => {
  const now = Date.parse("2026-10-22T04:00:00Z");

  it("reads idle, queued, busy and offline", () => {
    expect(
      runnerStatusLine(
        runner({ status: "online", is_busy: false, queue_depth: 0, last_heartbeat_at: null }) as never,
        now
      )
    ).toBe("online · idle");
    expect(
      runnerStatusLine(
        runner({ status: "online", is_busy: false, queue_depth: 2, last_heartbeat_at: null }) as never,
        now
      )
    ).toBe("online · 2 waiting");
    expect(
      runnerStatusLine(runner({ status: "busy", is_busy: true, queue_depth: 3, last_heartbeat_at: null }) as never, now)
    ).toBe("busy · 3 waiting");
    expect(runnerStatusLine(runner({ status: "busy", last_heartbeat_at: "2026-10-22T03:00:00Z" }) as never, now)).toBe(
      "busy"
    );
    expect(
      runnerStatusLine(runner({ status: "offline", last_heartbeat_at: "2026-10-22T02:00:00Z" }) as never, now)
    ).toBe("offline · last seen 2 h ago");
  });

  it("degrades without heartbeats, enrollments or queue answers", () => {
    expect(runnerStatusLine(runner({ status: "offline", last_heartbeat_at: null }) as never, now)).toBe("offline");
    expect(runnerStatusLine(runner({ status: "offline", last_heartbeat_at: "bogus" }) as never, now)).toBe("offline");
    expect(
      runnerStatusLine(runner({ status: "online", enrolled_at: null, last_heartbeat_at: null }) as never, now)
    ).toBe("offline");
    expect(runnerStatusLine(runner({ status: "revoked", last_heartbeat_at: null }) as never, now)).toBe("revoked");
  });
});

describe("runnerWaitNote", () => {
  it("warns on busy or queued runners, stays quiet on free ones", () => {
    expect(runnerWaitNote(runner({ status: "online", queue_depth: 0 }) as never)).toBe(null);
    expect(runnerWaitNote(runner({ status: "online" }) as never)).toBe(null);
    expect(runnerWaitNote(runner({ status: "busy", queue_depth: 3 }) as never)).toBe(
      "gpu-box runs one issue at a time. This issue will wait behind 3 others."
    );
    expect(runnerWaitNote(runner({ status: "online", queue_depth: 1 }) as never)).toBe(
      "gpu-box runs one issue at a time. This issue will wait behind 1 other."
    );
    expect(runnerWaitNote(runner({ status: "busy", queue_depth: 0 }) as never)).toBe(
      "gpu-box is busy. This issue will wait until it is free."
    );
  });
});

describe("selectRunnerOwnerName", () => {
  const directory = [
    {
      member: { id: "owner-1", display_name: "Ana", avatar_url: null },
      role: 15,
      is_active: true,
    },
  ] as unknown as WorkspaceMemberList;

  it("resolves the owner display name from the directory", () => {
    expect(selectRunnerOwnerName(directory, "owner-1")).toBe("Ana");
    expect(selectRunnerOwnerName(directory, "owner-9")).toBe(null);
    expect(selectRunnerOwnerName(undefined, "owner-1")).toBe(null);
  });
});

describe("podOptionLabel", () => {
  it("names the squad choice", () => {
    expect(podOptionLabel("ACME-Backend")).toBe("Any runner in ACME-Backend");
  });
});

describe("buildExecutionTargets with runners", () => {
  const online = runner({
    id: "623e4567-e89b-12d3-a456-426614174005",
    name: "gpu-box",
    status: "online",
    pod: POD_A.id,
    last_heartbeat_at: "2026-10-22T03:59:00Z",
    is_busy: false,
    queue_depth: 0,
    assignable: true,
    not_assignable_code: "",
    owner: "owner-1",
  });
  const offline = runner({
    id: "723e4567-e89b-12d3-a456-426614174006",
    name: "mac-mini",
    status: "offline",
    pod: POD_A.id,
    last_heartbeat_at: null,
    assignable: false,
    not_assignable_code: "runner_owner_not_on_issue",
    owner: "owner-9",
  });

  it("nests each pod's runners under its squad row", () => {
    const targets = buildExecutionTargets(
      [POD_A, POD_B] as PodList,
      [],
      [online, offline] as unknown as RunnerList,
      (ownerId) => (ownerId === "owner-1" ? "Ana" : null)
    );
    expect(targets).toEqual([
      { kind: "pod", podId: POD_A.id, label: "ACME-Backend", isDefault: true },
      {
        kind: "runner",
        runnerId: online.id,
        podId: POD_A.id,
        label: "gpu-box",
        statusLine: "online · idle",
        ownerName: "Ana",
        assignable: true,
        reason: "",
      },
      {
        kind: "runner",
        runnerId: offline.id,
        podId: POD_A.id,
        label: "mac-mini",
        statusLine: "offline",
        ownerName: "",
        assignable: false,
        reason: "Add the runner's owner as an assignee to choose it.",
      },
      { kind: "pod", podId: POD_B.id, label: "ACME-Tier", isDefault: false },
    ]);
  });

  it("keeps runners without eligibility answers pickable", () => {
    const targets = buildExecutionTargets([POD_A] as PodList, [], [
      runner({ id: "823e4567-e89b-12d3-a456-426614174007", pod: POD_A.id }),
    ] as unknown as RunnerList);
    expect(targets[1]).toMatchObject({ kind: "runner", assignable: true, reason: "" });
  });

  it("disables an unassignable runner without a code and no reason", () => {
    const targets = buildExecutionTargets([POD_A] as PodList, [], [
      runner({ id: "923e4567-e89b-12d3-a456-426614174008", pod: POD_A.id, assignable: false }),
    ] as unknown as RunnerList);
    expect(targets[1]).toMatchObject({ kind: "runner", assignable: false, reason: "" });
  });
});

describe("selectTriggerText", () => {
  const targets = buildExecutionTargets([POD_A] as PodList, [], [
    runner({ id: "runner-1", name: "gpu-box", pod: POD_A.id }),
  ] as unknown as RunnerList);

  it("names pods, runners and agents", () => {
    expect(selectTriggerText({ type: "pod", podId: POD_A.id }, targets, true, "Select")).toBe("ACME-Backend");
    expect(selectTriggerText({ type: "runner", runnerId: "runner-1", podId: POD_A.id }, targets, true, "Select")).toBe(
      "gpu-box"
    );
    expect(selectTriggerText({ type: "agent", executor: "cloud_agent" }, targets, true, "Select")).toBe(
      "Pi Dash Cloud Agent"
    );
  });

  it("falls back to the prompt and the unknown labels", () => {
    expect(selectTriggerText(null, targets, true, "Select")).toBe("Select");
    expect(selectTriggerText({ type: "agent", executor: "local_runner" }, targets, true, "Select")).toBe("Select");
    expect(selectTriggerText({ type: "pod", podId: POD_A.id }, targets, false, "Select")).toBe("Select");
    expect(selectTriggerText({ type: "pod", podId: "missing" }, targets, true, "Select")).toBe("Unknown pod");
    expect(selectTriggerText({ type: "runner", runnerId: "missing", podId: POD_A.id }, targets, true, "Select")).toBe(
      "Unknown runner"
    );
  });
});

describe("selectTriggerPodName", () => {
  const targets = buildExecutionTargets([POD_A] as PodList, [], [
    runner({ id: "runner-1", name: "gpu-box", pod: POD_A.id }),
  ] as unknown as RunnerList);

  it("names the pod behind a runner selection", () => {
    expect(selectTriggerPodName({ type: "runner", runnerId: "runner-1", podId: POD_A.id }, targets)).toBe(
      "ACME-Backend"
    );
  });

  it("reads null without a runner or without its pod", () => {
    expect(selectTriggerPodName(null, targets)).toBe(null);
    expect(selectTriggerPodName({ type: "pod", podId: POD_A.id }, targets)).toBe(null);
    expect(selectTriggerPodName({ type: "agent", executor: "cloud_agent" }, targets)).toBe(null);
    expect(selectTriggerPodName({ type: "runner", runnerId: "missing", podId: POD_A.id }, targets)).toBe(null);
    const stray = [
      {
        kind: "runner",
        runnerId: "runner-9",
        podId: POD_B.id,
        label: "stray",
        statusLine: "online · idle",
        ownerName: "",
        assignable: true,
        reason: "",
      },
    ] as const;
    expect(selectTriggerPodName({ type: "runner", runnerId: "runner-9", podId: POD_B.id }, [...stray])).toBe(null);
  });
});

describe("selectRunnerWaitNote", () => {
  it("reads the note for the selected runner", () => {
    const runners = [
      runner({ id: "runner-1", status: "busy", is_busy: true, queue_depth: 2 }),
    ] as unknown as RunnerList;
    expect(selectRunnerWaitNote(runners, "runner-1")).toContain("wait behind 2 others");
    expect(selectRunnerWaitNote(runners, "missing")).toBe(null);
    expect(selectRunnerWaitNote(undefined, "runner-1")).toBe(null);
    const free = [runner({ id: "runner-2", status: "online", queue_depth: 0 })] as unknown as RunnerList;
    expect(selectRunnerWaitNote(free, "runner-2")).toBe(null);
  });
});
