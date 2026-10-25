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
    help: "Each run files an issue for every finding it has not already tracked in an open issue, and leaves it for someone to pick up.",
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
  create_issue:
    "Each run files an issue for every finding it has not already tracked in an open issue, and leaves it for someone to pick up.",
  apply_fix: "Each run edits the code and opens a pull request, so a person reviews the change before it lands.",
  fix_and_review: "Each run files the finding, moves it to In Progress and starts an agent on it straight away.",
};

/** The help line for a mode. */
export function outcomeHelp(mode: OutcomeMode): string {
  return OUTCOME_HELP[mode];
}

export interface OutcomeModeFieldProps {
  value: OutcomeMode;
  disabled?: boolean;
  onChange: (mode: OutcomeMode) => void;
}

export function OutcomeModeField({ value, disabled = false, onChange }: OutcomeModeFieldProps): React.ReactElement {
  const groupRef = React.useRef<HTMLDivElement>(null);

  // Arrow keys move the selection and focus together, as a radio group
  // does; only the checked option sits in the tab order.
  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>): void => {
    if (disabled) return;
    const forward = event.key === "ArrowRight" || event.key === "ArrowDown";
    const backward = event.key === "ArrowLeft" || event.key === "ArrowUp";
    const step = forward ? 1 : backward ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const current = OUTCOME_OPTIONS.findIndex((option) => option.value === value);
    const next = OUTCOME_OPTIONS[(current + step + OUTCOME_OPTIONS.length) % OUTCOME_OPTIONS.length];
    if (next === undefined) return;
    onChange(next.value);
    const buttons = groupRef.current?.querySelectorAll<HTMLButtonElement>('[role="radio"]');
    buttons?.[OUTCOME_OPTIONS.indexOf(next)]?.focus();
  };

  return (
    <div className="flex flex-col gap-(--space-2)">
      <div
        ref={groupRef}
        role="radiogroup"
        aria-label="What to do with findings"
        className="flex flex-wrap gap-(--space-2)"
        onKeyDown={handleKeyDown}
      >
        {OUTCOME_OPTIONS.map((option) => (
          <button
            key={option.value}
            type="button"
            role="radio"
            value={option.value}
            aria-checked={value === option.value}
            aria-label={option.label}
            tabIndex={value === option.value ? 0 : -1}
            disabled={disabled}
            className={
              value === option.value
                ? "text-body rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-3) py-(--space-1) font-medium text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
                : "text-body rounded-(--radius-control) border border-transparent px-(--space-3) py-(--space-1) text-(--text-muted) hover:text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
            }
            onClick={() => onChange(option.value)}
          >
            {option.label}
          </button>
        ))}
      </div>
      <p className="text-meta text-(--text-muted)">{outcomeHelp(value)}</p>
    </div>
  );
}
