// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What a scheduler install does with its findings (parity/specs/agents.md
// §4, AGT-018). One choice per install, with a help line that follows it;
// filing issues is the default. The same field serves the install, edit
// and bulk dialogs.

import type { SchedulerBinding } from "@pidash/api-client";
import * as React from "react";

export type OutcomeMode = SchedulerBinding["outcome_mode"];

interface OutcomeOption {
  value: OutcomeMode;
  label: string;
  help: string;
}

/** The three modes in display order; the first is the default. */
export const OUTCOME_OPTIONS: readonly OutcomeOption[] = [
  {
    value: "create_issue",
    label: "Create issues",
    help: "Each run turns its findings into issues on this project and leaves them for someone to pick up.",
  },
  {
    value: "apply_fix",
    label: "Apply fix",
    help: "Each run edits the code and opens a pull request, so a person reviews the change before it lands.",
  },
  {
    value: "fix_and_review",
    label: "Fix & open for review",
    help: "Each run files the finding, moves it to In Progress and starts an agent on it straight away.",
  },
];

/** The default mode a new install starts on. */
export const DEFAULT_OUTCOME_MODE: OutcomeMode = "create_issue";

const OUTCOME_HELP: Record<OutcomeMode, string> = {
  create_issue: "Each run turns its findings into issues on this project and leaves them for someone to pick up.",
  apply_fix: "Each run edits the code and opens a pull request, so a person reviews the change before it lands.",
  fix_and_review: "Each run files the finding, moves it to In Progress and starts an agent on it straight away.",
};

/** The help line for a mode. */
export function outcomeHelp(mode: OutcomeMode): string {
  return OUTCOME_HELP[mode];
}

export interface OutcomeModeFieldProps {
  idPrefix: string;
  value: OutcomeMode;
  disabled?: boolean;
  onChange: (mode: OutcomeMode) => void;
}

export function OutcomeModeField({
  idPrefix,
  value,
  disabled = false,
  onChange,
}: OutcomeModeFieldProps): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-2)">
      <div role="radiogroup" aria-label="What to do with findings" className="flex flex-wrap gap-(--space-4)">
        {OUTCOME_OPTIONS.map((option) => (
          <label key={option.value} className="text-body flex items-center gap-(--space-2) text-(--text)">
            <input
              type="radio"
              name={`${idPrefix}-outcome`}
              value={option.value}
              checked={value === option.value}
              disabled={disabled}
              onChange={() => onChange(option.value)}
            />
            {option.label}
          </label>
        ))}
      </div>
      <p className="text-meta text-(--text-muted)">{outcomeHelp(value)}</p>
    </div>
  );
}
