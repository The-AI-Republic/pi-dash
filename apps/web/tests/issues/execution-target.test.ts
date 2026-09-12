import { describe, expect, it } from "vitest";
import {
  executionTargetPatch,
  executionTargetValue,
  executorKindLabel,
  managedAgentOption,
  managedRunnerReasonCopy,
} from "../../core/components/dropdowns/pod/execution-target";

const echo = (key: string) => key;

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

describe("managed runner reason copy", () => {
  it("stays silent for a project the desktop enrolls on its own", () => {
    expect(managedRunnerReasonCopy("no_managed_runner_for_project", echo)).toBeNull();
  });
  it("tells BYOK users to switch to OpenHub without stranding their key", () => {
    const copy = managedRunnerReasonCopy("byok_not_supported_on_desktop", echo);
    expect(copy).toContain("OpenHub");
    expect(copy).toContain("your own key");
  });
  it("gives a distinct sentence per unavailable reason", () => {
    const codes = [
      "desktop_not_connected",
      "managed_runner_disabled",
      "llm_config_missing",
      "gateway_scopes_missing",
      "byok_not_supported_on_desktop",
    ];
    const messages = codes.map((c) => managedRunnerReasonCopy(c, echo));
    expect(messages.every((m) => typeof m === "string" && m.length > 0)).toBe(true);
    expect(new Set(messages).size).toBe(codes.length);
  });
  it("falls back to the desktop-not-connected copy for an unknown reason", () => {
    expect(managedRunnerReasonCopy("something_new", echo)).toBe(managedRunnerReasonCopy("desktop_not_connected", echo));
  });
});

describe("executor kind label", () => {
  it("names the managed runner as Pi Dash Agent, not a local runner", () => {
    expect(executorKindLabel("managed_runner", echo)).toBe("Pi Dash Agent");
    expect(executorKindLabel("cloud_agent", echo)).toBe("Pi Dash Cloud Agent");
    expect(executorKindLabel("local_runner", echo)).toBe("Local Runner");
  });
});
