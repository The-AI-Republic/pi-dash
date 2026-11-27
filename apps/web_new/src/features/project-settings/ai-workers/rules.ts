// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lost-runner rule shaping for the AI Workers tab (hybrid assignment,
// design §6.6). Pure: the two rules the project offers, reading the
// stored setting onto the form (absent or unknown reads as the
// default), and the project write the form saves.
import {
  DEDICATED_RUNNER_LOST_FAIL_TO_BACKLOG,
  DEDICATED_RUNNER_LOST_FALL_BACK,
  type ProjectUpdateInput,
} from "@pidash/api-client";

/** The two lost-runner rules, in display order. */
export type LostRunnerRule = "fail_to_backlog" | "fall_back";

/** The rule a project without the setting behaves by. */
export const DEFAULT_LOST_RUNNER_RULE: LostRunnerRule = DEDICATED_RUNNER_LOST_FAIL_TO_BACKLOG;

export interface LostRunnerRuleOption {
  label: string;
  description: string;
}

/** The two rules in display order. */
export const LOST_RUNNER_RULE_ORDER: readonly LostRunnerRule[] = [
  DEDICATED_RUNNER_LOST_FAIL_TO_BACKLOG,
  DEDICATED_RUNNER_LOST_FALL_BACK,
];

export const LOST_RUNNER_RULE_OPTIONS: Record<LostRunnerRule, LostRunnerRuleOption> = {
  [DEDICATED_RUNNER_LOST_FAIL_TO_BACKLOG]: {
    label: "Fail the run and move the issue to Backlog",
    description:
      "The queued run fails, the assignment is cleared, and the issue moves to Backlog with a comment naming the runner. A person must choose a new runner or the pod.",
  },
  [DEDICATED_RUNNER_LOST_FALL_BACK]: {
    label: "Fall back to the pod",
    description:
      "The issue falls back to its own pod, then the project's default pod, then bounces to Backlog when neither has a runner that can serve it. The system comment names the runner and the pod used.",
  },
};

/**
 * The form value for the stored setting. Anything but the known
 * fall-back value reads as the default, so older rows and future
 * rules never strand the form.
 */
export function lostRunnerRuleFrom(stored: string | undefined): LostRunnerRule {
  return stored === DEDICATED_RUNNER_LOST_FALL_BACK
    ? DEDICATED_RUNNER_LOST_FALL_BACK
    : DEDICATED_RUNNER_LOST_FAIL_TO_BACKLOG;
}

/** Trigger label for a rule value. */
export function lostRunnerRuleLabel(value: LostRunnerRule): string {
  return LOST_RUNNER_RULE_OPTIONS[value].label;
}

/** Explainer for a rule value. */
export function lostRunnerRuleDescription(value: LostRunnerRule): string {
  return LOST_RUNNER_RULE_OPTIONS[value].description;
}

/** The project write a rule pick saves. */
export function buildLostRunnerRulePayload(value: LostRunnerRule): ProjectUpdateInput {
  return { dedicated_runner_lost_action: value };
}

/** Who may move an issue's state or run it (NEWPIDASH-261). */
export type IssueRunPolicyValue = "creator_and_assignees" | "project_members";

/** The policy a project without the setting behaves by (default mode). */
export const DEFAULT_ISSUE_RUN_POLICY: IssueRunPolicyValue = "creator_and_assignees";

/** The two policies in display order. */
export const ISSUE_RUN_POLICY_ORDER: readonly IssueRunPolicyValue[] = ["creator_and_assignees", "project_members"];

export interface IssueRunPolicyOption {
  label: string;
  description: string;
}

export const ISSUE_RUN_POLICY_OPTIONS: Record<IssueRunPolicyValue, IssueRunPolicyOption> = {
  creator_and_assignees: {
    label: "Issue creator and assignees",
    description:
      "Only the issue's creator and its current assignees may move its state or start, cancel or release an agent run. Each person still uses only their own runners.",
  },
  project_members: {
    label: "All project members",
    description:
      "Any project member (Member or Admin) may move an issue's state or start, cancel or release an agent run. Guests still cannot.",
  },
};

/**
 * The form value for the stored setting. Anything but the known
 * `project_members` value reads as the default, so older rows and
 * unknown values never strand the form.
 */
export function issueRunPolicyFrom(stored: string | undefined): IssueRunPolicyValue {
  return stored === "project_members" ? "project_members" : "creator_and_assignees";
}

/** Trigger label for a policy value. */
export function issueRunPolicyLabel(value: IssueRunPolicyValue): string {
  return ISSUE_RUN_POLICY_OPTIONS[value].label;
}

/** Explainer for a policy value. */
export function issueRunPolicyDescription(value: IssueRunPolicyValue): string {
  return ISSUE_RUN_POLICY_OPTIONS[value].description;
}

/**
 * The form save: only the fields the admin changed are written, so a
 * lost-runner-only edit never re-sends the run policy (and vice
 * versa). A no-change submit sends an empty partial.
 */
export function buildAiWorkersPayload(
  storedLost: string | undefined,
  lost: LostRunnerRule,
  storedPolicy: string | undefined,
  policy: IssueRunPolicyValue
): ProjectUpdateInput {
  const payload: ProjectUpdateInput = {};
  if (lostRunnerRuleFrom(storedLost) !== lost) {
    payload.dedicated_runner_lost_action = lost;
  }
  if (issueRunPolicyFrom(storedPolicy) !== policy) {
    payload.settings = { issue_run_policy: policy };
  }
  return payload;
}
