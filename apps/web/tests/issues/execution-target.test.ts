import { describe, expect, it } from "vitest";
import type { IProject, IRunner } from "@pi-dash/types";
import {
  POD_PREFIX,
  RUNNER_PREFIX,
  cloudAgentOption,
  executionSelectionId,
  executionTargetPatch,
  executionTargetPatchForSelection,
  executionTargetValue,
  executionTargetSelection,
  managedAgentOption,
  parseExecutionSelectionId,
  runnerDisplayLabel,
  runnerReasonMessage,
  runnerStatusLine,
} from "../../core/components/dropdowns/pod/execution-target";

function projectWith(options: IProject["agent_executor_options"]): IProject {
  return { agent_executor_options: options } as IProject;
}

function runner(over: Partial<IRunner> = {}): IRunner {
  return {
    id: "r1",
    name: "gpu-box",
    status: "online",
    provisioning: "manual",
    pod: "pod-1",
    ...over,
  } as unknown as IRunner;
}

describe("dedicated-runner execution target (§14.4, §23.6)", () => {
  it("round-trips pod selection through tagged ids", () => {
    expect(executionSelectionId({ type: "pod", podId: "p1" })).toBe(`${POD_PREFIX}p1`);
    expect(parseExecutionSelectionId(`${POD_PREFIX}p1`)).toEqual({ type: "pod", podId: "p1" });
    expect(parseExecutionSelectionId("p1")).toEqual({ type: "pod", podId: "p1" });
  });
  it("round-trips runner selection and resolves its pod", () => {
    expect(parseExecutionSelectionId(`${RUNNER_PREFIX}r1`, [runner()])).toEqual({
      type: "runner",
      runnerId: "r1",
      podId: "pod-1",
    });
    expect(executionSelectionId({ type: "runner", runnerId: "r1", podId: "pod-1" })).toBe(`${RUNNER_PREFIX}r1`);
  });
  it("reads the issue's runner as a runner selection regardless of executor", () => {
    const sel = executionTargetSelection(
      { assigned_runner_id: "r1", assigned_pod_id: "pod-1", agent_executor: "local_runner" },
      null
    );
    expect(sel).toEqual({ type: "runner", runnerId: "r1", podId: "pod-1" });
    expect(executionTargetValue({ assigned_runner_id: "r1", assigned_pod_id: "pod-1" }, null)).toBe(
      `${RUNNER_PREFIX}r1`
    );
    expect(executionTargetValue({ assigned_pod_id: "pod-2" }, null)).toBe(`${POD_PREFIX}pod-2`);
  });
  it("maps selections to patches: pod clears runner, runner sets both", () => {
    expect(executionTargetPatchForSelection({ type: "pod", podId: "p2" })).toEqual({
      agent_executor: "local_runner",
      assigned_pod_id: "p2",
      assigned_runner_id: null,
    });
    expect(executionTargetPatchForSelection({ type: "runner", runnerId: "r1", podId: "p1" })).toEqual({
      agent_executor: "local_runner",
      assigned_pod_id: "p1",
      assigned_runner_id: "r1",
    });
    expect(executionTargetPatch(`${POD_PREFIX}p2`)).toMatchObject({ assigned_pod_id: "p2", assigned_runner_id: null });
    expect(executionTargetPatch(`${RUNNER_PREFIX}r1`)).toMatchObject({ assigned_runner_id: "r1" });
  });
  it("hides deprecated executor options after #2903", () => {
    const project = projectWith([
      { kind: "managed_runner", available: true, reason_code: "", deprecated: true },
      { kind: "cloud_agent", available: true, reason_code: "", deprecated: true },
    ]);
    // executor sentinels no longer drive the dropdown value
    expect(executionTargetValue({ agent_executor: "managed_runner", assigned_pod_id: "pod-1" }, project)).toBe(
      `${POD_PREFIX}pod-1`
    );
    expect(managedAgentOption(project)).toBeUndefined();
    expect(cloudAgentOption(project)).toBeUndefined();
  });
  it("keeps legacy executor patch shapes for older clients", () => {
    expect(executionTargetPatch("cloud_agent")).toHaveProperty("agent_executor", "local_runner");
  });
  it("labels engines as Built-in engine · <machine>", () => {
    expect(runnerDisplayLabel(runner({ provisioning: "desktop_bundled", machine_display_name: "laptop" }))).toBe(
      "Built-in engine · laptop"
    );
    expect(
      runnerDisplayLabel(runner({ provisioning: "desktop_bundled", dev_machine_detail: { label: "desk" } as never }))
    ).toBe("Built-in engine · desk");
    expect(runnerDisplayLabel(runner({ name: "gpu-box" }))).toBe("gpu-box");
  });
  it("explains not-assignable codes without executor_not_local", () => {
    expect(runnerReasonMessage("runner_owner_not_on_issue")).toContain("assignee");
    expect(runnerReasonMessage("unknown_code")).toBe("unknown_code");
  });
  it("renders the runner status line", () => {
    expect(runnerStatusLine({ status: "online", is_busy: false, queue_depth: 0 } as never)).toBe("online · idle");
    expect(runnerStatusLine({ status: "online", is_busy: true, queue_depth: 3 } as never)).toBe("busy · 3 waiting");
    expect(runnerStatusLine({ status: "offline", last_heartbeat_at: null } as never)).toBe("offline");
  });
});
