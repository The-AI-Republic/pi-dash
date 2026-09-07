import { describe, expect, it } from "vitest";
import {
  executionTargetPatch,
  executionTargetValue,
  managedAgentOption,
} from "../../core/components/dropdowns/pod/execution-target";

describe("managed issue execution target", () => {
  it("shows a desktop pin even when the issue still has its local pod", () => {
    expect(executionTargetValue({ agent_executor: "managed_runner", assigned_pod_id: "pod-1" }, null)).toBe(
      "managed_runner"
    );
  });
  it("changes the issue executor without changing its pod or project default", () => {
    expect(executionTargetPatch("managed_runner")).toEqual({ agent_executor: "managed_runner" });
    expect(executionTargetPatch("cloud_agent")).toEqual({ agent_executor: "cloud_agent" });
  });
  it("clears a desktop pin when the user selects a local pod", () => {
    expect(executionTargetPatch("pod-1")).toEqual({ agent_executor: "local_runner", assigned_pod_id: "pod-1" });
  });
  it("keeps desktop selection unavailable until the server reports it", () => {
    expect(managedAgentOption(null)).toEqual({ available: false, reasonCode: "desktop_not_connected" });
  });
});
