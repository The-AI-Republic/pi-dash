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
    description: "Queued runs on the lost runner fail, and their issues move to Backlog.",
  },
  [DEDICATED_RUNNER_LOST_FALL_BACK]: {
    label: "Fall back to the pod",
    description: "Queued runs continue on any runner in the pod instead.",
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
