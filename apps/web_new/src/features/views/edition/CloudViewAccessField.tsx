// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud access selector (parity/specs/views.md §5, VIEW-019; Decisions
// D23). Fills the `viewAccess` slot's Field: shared vs owner-only with
// one explanatory line each. The OSS edition leaves the slot absent and
// renders nothing in its place. Both options stay in the tab order,
// matching the filter-section option pattern.

import * as React from "react";

import type { ViewAccessFieldProps, ViewAccessValue } from "../../../core/edition/index.js";
import { OWNER_ACCESS, SHARED_ACCESS } from "../dialog/ViewAccessField.js";

interface AccessOption {
  value: ViewAccessValue;
  label: string;
  explanation: string;
  testId: string;
}

const ACCESS_OPTIONS: readonly AccessOption[] = [
  {
    value: SHARED_ACCESS,
    label: "Shared",
    explanation: "Anyone who can open the project sees this view.",
    testId: "view-access-shared",
  },
  {
    value: OWNER_ACCESS,
    label: "Owner-only",
    explanation: "Only the owner sees this view.",
    testId: "view-access-owner-only",
  },
];

/** The cloud edition's shared-vs-owner-only selector. */
export function CloudViewAccessField({ value, onChange, disabled }: ViewAccessFieldProps): React.ReactElement {
  return (
    <div role="radiogroup" aria-label="Access" className="flex flex-col gap-(--space-2)">
      {ACCESS_OPTIONS.map((option) => {
        const checked = value === option.value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={checked}
            data-testid={option.testId}
            disabled={disabled}
            onClick={() => {
              onChange(option.value);
            }}
            className="flex w-full items-start gap-(--space-3) rounded-(--radius-control) border border-(--border) px-(--space-3) py-(--space-2) text-left disabled:cursor-not-allowed disabled:opacity-60"
          >
            <span
              aria-hidden="true"
              className={`mt-(--space-1) grid size-3.5 shrink-0 place-items-center rounded-full border ${
                checked ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--border)"
              }`}
            >
              {checked ? <span className="size-1.5 rounded-full bg-(--on-accent)" /> : null}
            </span>
            <span className="flex min-w-0 flex-col">
              <span className="text-body font-medium text-(--text)">{option.label}</span>
              <span className="text-meta text-(--text-muted)">{option.explanation}</span>
            </span>
          </button>
        );
      })}
    </div>
  );
}
