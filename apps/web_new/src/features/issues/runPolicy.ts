// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Client-side "Who can run issues" gate (NEWPIDASH-261). Pure mirror of
// the server's `can_run_issue_pure`: default mode allows only the
// issue's creator and its current assignees (no admin bypass) while
// `project_members` allows any project Member (15) or Admin (20).
// Guests (5) never pass. The server is the authority; this only
// shapes the affordances (disabled controls + reason), and the 403
// `not_issue_runner` refusal stays the backstop.
import { issueRunPolicyOrDefault } from "@pidash/api-client";

import { ROLE_MEMBER } from "../../core/session/permissions.js";

/** Disabled-control tooltip when the run policy refuses (mirrors the API). */
export const RUN_POLICY_BLOCKED_REASON =
  "Only the issue's creator and assignees can do this in this project. Ask the creator to assign you, or ask a project admin to change the project's 'Who can run issues' setting.";

export interface RunPolicyInput {
  /** Stored `projects.settings.issue_run_policy`; absent reads as the default. */
  policy?: string | undefined;
  /** Signed-in user id; null when unknown (denies). */
  viewerId: string | null | undefined;
  /** Issue creator id; null when the row has none (denies default). */
  creatorId?: string | null | undefined;
  /** Current (live) assignee ids. */
  assigneeIds?: readonly string[] | undefined;
  /** Active project membership role (20/15/5); null when not a member. */
  memberRole?: number | null | undefined;
}

/** Whether the viewer may move the issue's state or run it. */
export function canRunIssue(input: RunPolicyInput): boolean {
  const policy = issueRunPolicyOrDefault(input.policy);
  if (policy === "project_members") {
    return (input.memberRole ?? 0) >= ROLE_MEMBER;
  }
  if (input.viewerId == null) return false;
  if (input.creatorId != null && input.creatorId === input.viewerId) return true;
  return (input.assigneeIds ?? []).includes(input.viewerId);
}

/** The tooltip for a refused affordance, or undefined when allowed. */
export function runBlockedReason(input: RunPolicyInput): string | undefined {
  return canRunIssue(input) ? undefined : RUN_POLICY_BLOCKED_REASON;
}
