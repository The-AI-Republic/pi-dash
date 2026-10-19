// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { AgentExecutorOption, PodList } from "@pidash/api-client";
import { agentReasonMessage } from "../lib/agentReasons.js";
import {
  buildExecutionTargets,
  executionSelectionId,
  executionTargetLabel,
  executionTargetPatch,
  formatAvailabilityReason,
  parseExecutionSelectionId,
  selectDefaultPod,
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
      agent_executor: "local_runner",
    });
  });

  it("pins the executor and clears the pod on an agent pick", () => {
    expect(executionTargetPatch({ type: "agent", executor: "cloud_agent" })).toEqual({
      assigned_pod_id: null,
      agent_executor: "cloud_agent",
    });
    expect(executionTargetPatch({ type: "agent", executor: "managed_runner" })).toEqual({
      assigned_pod_id: null,
      agent_executor: "managed_runner",
    });
  });
});

describe("execution selection ids", () => {
  it("round-trips selections through option ids", () => {
    expect(executionSelectionId(null)).toBe(null);
    expect(executionSelectionId({ type: "pod", podId: POD_A.id })).toBe(`pod:${POD_A.id}`);
    expect(executionSelectionId({ type: "agent", executor: "cloud_agent" })).toBe("agent:cloud_agent");
    expect(parseExecutionSelectionId(`pod:${POD_A.id}`)).toEqual({ type: "pod", podId: POD_A.id });
    expect(parseExecutionSelectionId("agent:managed_runner")).toEqual({ type: "agent", executor: "managed_runner" });
  });
});
