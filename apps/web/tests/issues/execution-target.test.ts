import { describe, expect, it } from "vitest";
import type { IProject } from "@pi-dash/types";
import {
  cloudAgentOption,
  executionTargetPatch,
  executionTargetValue,
  managedAgentOption,
} from "../../core/components/dropdowns/pod/execution-target";

function projectWith(options: IProject["agent_executor_options"]): IProject {
  return { agent_executor_options: options } as IProject;
}

describe("managed issue execution target", () => {
  it("shows a desktop pin when the server still offers the built-in agent", () => {
    const project = projectWith([{ kind: "managed_runner", available: true, reason_code: "" }]);
    expect(executionTargetValue({ agent_executor: "managed_runner", assigned_pod_id: "pod-1" }, project)).toBe(
      "managed_runner"
    );
  });
  it("reads a deprecated executor as the issue's pod", () => {
    const project = projectWith([
      { kind: "managed_runner", available: true, reason_code: "", deprecated: true },
      { kind: "cloud_agent", available: true, reason_code: "", deprecated: true },
    ]);
    expect(executionTargetValue({ agent_executor: "managed_runner", assigned_pod_id: "pod-1" }, project)).toBe("pod-1");
    expect(executionTargetValue({ agent_executor: "cloud_agent", assigned_pod_id: "pod-2" }, project)).toBe("pod-2");
  });
  it("hides deprecated executor options", () => {
    const project = projectWith([
      { kind: "managed_runner", available: true, reason_code: "", deprecated: true },
      { kind: "cloud_agent", available: true, reason_code: "", deprecated: true },
    ]);
    expect(managedAgentOption(project)).toBeUndefined();
    expect(cloudAgentOption(project)).toBeUndefined();
  });
  it("changes the issue executor without changing its pod or project default", () => {
    expect(executionTargetPatch("managed_runner")).toEqual({ agent_executor: "managed_runner" });
    expect(executionTargetPatch("cloud_agent")).toEqual({ agent_executor: "cloud_agent" });
  });
  it("clears a desktop pin when the user selects a local pod", () => {
    expect(executionTargetPatch("pod-1")).toEqual({ agent_executor: "local_runner", assigned_pod_id: "pod-1" });
  });
  it("offers no desktop row until the server reports it", () => {
    expect(managedAgentOption(null)).toBeUndefined();
  });
  it("keeps the desktop reason when the offered row is unavailable", () => {
    const project = projectWith([{ kind: "managed_runner", available: false, reason_code: "" }]);
    expect(managedAgentOption(project)).toEqual({ available: false, reasonCode: "desktop_not_connected" });
  });
});
