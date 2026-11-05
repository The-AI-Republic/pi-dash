// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod membership model (built-in engine design §7, §11). A runner always
// sits in exactly one pod; whether it takes that pod's unpinned work is
// its `pod_dispatch`. "Add to pod" makes it a pod member (moving it first
// when it sits in another pod of the project), "Remove from pod" leaves
// it serving only issues assigned to it, and "Move" changes the pod.
// Only a runner's owner may do any of these, so non-owners see the
// members without actions. Pure, so the panel stays thin.

import { isApiError, type Pod, type Runner, type RunnerPatch, type RunnerPodDispatch } from "@pidash/api-client";

import { isEngineRunner } from "../../shared/lib/runnerIdentity.js";

export const POD_MEMBERSHIP_LABELS: Record<RunnerPodDispatch, string> = {
  pod_member: "Takes pod work",
  pinned_only: "Assigned issues only",
};

/** Own-agent runners predating the field take pod work, as before. */
export function runnerPodDispatch(runner: Pick<Runner, "pod_dispatch" | "provisioning">): RunnerPodDispatch {
  if (runner.pod_dispatch !== undefined) return runner.pod_dispatch;
  return isEngineRunner(runner) ? "pinned_only" : "pod_member";
}

/** Live runners sitting in the pod, members and assigned-only alike. */
export function podRunners(runners: readonly Runner[], podId: string): Runner[] {
  return runners.filter((runner) => runner.pod === podId && runner.revoked_at === null);
}

/** True when the viewer may change this runner's pods. */
export function canManageRunner(runner: Pick<Runner, "owner" | "revoked_at">, userId: string | null): boolean {
  return userId !== null && runner.owner === userId && runner.revoked_at === null;
}

/**
 * The viewer's runners "Add runner to pod" offers: live, in the pod's
 * project, and not already taking this pod's work. Engines included.
 */
export function addablePodRunners(
  runners: readonly Runner[],
  pod: Pick<Pod, "id" | "project">,
  userId: string | null
): Runner[] {
  return runners.filter(
    (runner) =>
      canManageRunner(runner, userId) &&
      runner.pod_detail.project === pod.project &&
      !(runner.pod === pod.id && runnerPodDispatch(runner) === "pod_member")
  );
}

/** The patch for "Add to pod": move when elsewhere, then take pod work. */
export function addToPodPatch(runner: Pick<Runner, "pod">, podId: string): RunnerPatch {
  return runner.pod === podId ? { pod_dispatch: "pod_member" } : { pod: podId, pod_dispatch: "pod_member" };
}

/** The patch for "Remove from pod": keep the pod, serve assigned issues only. */
export const REMOVE_FROM_POD_PATCH: RunnerPatch = { pod_dispatch: "pinned_only" };

/** Other pods of the same project a runner can move to. */
export function moveTargets(pods: readonly Pod[], runner: Pick<Runner, "pod" | "pod_detail">): Pod[] {
  return pods.filter((pod) => pod.project === runner.pod_detail.project && pod.id !== runner.pod);
}

export const RUNNER_UPDATE_FAILED_TITLE = "Couldn't change the runner's pod";
export const RUNNER_UPDATE_FAILED = "The runner is unchanged. Try again.";
export const RUNNER_BUSY_MESSAGE =
  "This runner has a run in progress or reserved for it. Wait for it to finish, or cancel it, then move the runner.";
export const ENGINE_PROJECT_FIXED_MESSAGE = "A built-in engine can only move between pods of its own project.";

/** One issue the server lists as assigned to the runner. */
interface AssignedIssue {
  sequence_id?: unknown;
  name?: unknown;
}

function assignedIssueLabel(issue: AssignedIssue, projectIdentifier: string): string {
  const ref = typeof issue.sequence_id === "number" ? `${projectIdentifier}-${issue.sequence_id}` : "";
  const name = typeof issue.name === "string" ? issue.name : "";
  return [ref, name].filter((part) => part !== "").join(" ");
}

/**
 * What to tell the owner when a pod change is refused. Busy runners,
 * engines leaving their project and runners with assigned issues each
 * get their own sentence; the assigned issues are listed by key.
 */
export function runnerUpdateErrorMessage(error: unknown, projectIdentifier: string): string {
  if (!isApiError(error)) return RUNNER_UPDATE_FAILED;
  const body = (error.body ?? {}) as Record<string, unknown>;
  const code = typeof body["code"] === "string" ? body["code"] : error.code;
  if (code === "runner_busy") return RUNNER_BUSY_MESSAGE;
  if (code === "engine_runner_project_fixed") return ENGINE_PROJECT_FIXED_MESSAGE;
  if (code === "runner_has_assigned_issues") {
    const issues = Array.isArray(body["issues"]) ? (body["issues"] as AssignedIssue[]) : [];
    const listed = issues.map((issue) => assignedIssueLabel(issue, projectIdentifier)).filter((text) => text !== "");
    const head = "Issues are assigned to this runner. Assign them elsewhere before moving it to another project";
    return listed.length === 0 ? `${head}.` : `${head}: ${listed.join(", ")}.`;
  }
  const text = typeof body["error"] === "string" ? body["error"] : "";
  return text === "" ? RUNNER_UPDATE_FAILED : text;
}
