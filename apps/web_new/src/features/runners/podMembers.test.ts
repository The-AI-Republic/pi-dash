// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError, type Pod, type Runner } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  ENGINE_PROJECT_FIXED_MESSAGE,
  RUNNER_BUSY_MESSAGE,
  RUNNER_UPDATE_FAILED,
  addToPodPatch,
  addablePodRunners,
  canManageRunner,
  moveTargets,
  podRunners,
  runnerPodDispatch,
  runnerUpdateErrorMessage,
} from "./podMembers.js";

function runner(overrides: Partial<Runner> = {}): Runner {
  return {
    id: "r-1",
    name: "codex-1",
    provisioning: "manual",
    owner: "me",
    pod: "pod-a",
    pod_detail: { id: "pod-a", name: "WEB_a", is_default: true, project: "prj", project_identifier: "WEB" },
    revoked_at: null,
    ...overrides,
  } as Runner;
}

const podA = { id: "pod-a", name: "WEB_a", project: "prj", project_identifier: "WEB" } as Pod;
const podB = { id: "pod-b", name: "WEB_b", project: "prj", project_identifier: "WEB" } as Pod;
const other = { id: "pod-x", name: "API_x", project: "prj-2", project_identifier: "API" } as Pod;

describe("pod membership", () => {
  it("reads pod dispatch with a per-kind default for older servers", () => {
    expect(runnerPodDispatch(runner({ pod_dispatch: "pinned_only" }))).toBe("pinned_only");
    expect(runnerPodDispatch(runner())).toBe("pod_member");
    expect(runnerPodDispatch(runner({ provisioning: "desktop_bundled" }))).toBe("pinned_only");
  });

  it("lists live runners in the pod", () => {
    const rows = [
      runner(),
      runner({ id: "r-2", pod: "pod-b" }),
      runner({ id: "r-3", revoked_at: "2026-01-01T00:00:00Z" }),
    ];
    expect(podRunners(rows, "pod-a").map((row) => row.id)).toEqual(["r-1"]);
  });

  it("lets only the owner manage a live runner", () => {
    expect(canManageRunner(runner(), "me")).toBe(true);
    expect(canManageRunner(runner(), "you")).toBe(false);
    expect(canManageRunner(runner(), null)).toBe(false);
    expect(canManageRunner(runner({ revoked_at: "2026-01-01T00:00:00Z" }), "me")).toBe(false);
  });

  it("offers the owner's project runners that do not already take this pod's work", () => {
    const rows = [
      runner({ id: "member", pod_dispatch: "pod_member" }),
      runner({ id: "assigned-only", provisioning: "desktop_bundled", pod_dispatch: "pinned_only" }),
      runner({ id: "elsewhere", pod: "pod-b" }),
      runner({ id: "theirs", owner: "you", pod: "pod-b" }),
      runner({ id: "other-project", pod: "pod-x", pod_detail: { ...runner().pod_detail, project: "prj-2" } }),
    ];
    expect(addablePodRunners(rows, podA, "me").map((row) => row.id)).toEqual(["assigned-only", "elsewhere"]);
  });

  it("moves a runner in only when it sits elsewhere", () => {
    expect(addToPodPatch({ pod: "pod-a" }, "pod-a")).toEqual({ pod_dispatch: "pod_member" });
    expect(addToPodPatch({ pod: "pod-b" }, "pod-a")).toEqual({ pod: "pod-a", pod_dispatch: "pod_member" });
  });

  it("offers the other pods of the runner's project as move targets", () => {
    expect(moveTargets([podA, podB, other], runner()).map((pod) => pod.id)).toEqual(["pod-b"]);
  });
});

describe("runnerUpdateErrorMessage", () => {
  const refusal = (status: number, body: Record<string, unknown>) =>
    new ApiError({ status, code: String(body["code"] ?? status), message: "refused", body });

  it("explains busy runners and engines leaving their project", () => {
    expect(runnerUpdateErrorMessage(refusal(409, { code: "runner_busy" }))).toBe(RUNNER_BUSY_MESSAGE);
    expect(runnerUpdateErrorMessage(refusal(400, { code: "engine_runner_project_fixed" }))).toBe(
      ENGINE_PROJECT_FIXED_MESSAGE
    );
  });

  it("falls back to the server sentence, then the generic one", () => {
    expect(runnerUpdateErrorMessage(refusal(400, { error: "pod is in a different workspace" }))).toBe(
      "pod is in a different workspace"
    );
    expect(runnerUpdateErrorMessage(new Error("offline"))).toBe(RUNNER_UPDATE_FAILED);
  });
});
