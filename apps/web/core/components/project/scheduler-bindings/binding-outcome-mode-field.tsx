/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Controller } from "react-hook-form";
import type { Control, FieldValues, Path } from "react-hook-form";
import { useTranslation } from "@pi-dash/i18n";
import type { SchedulerOutcomeMode } from "@pi-dash/services";

// English source strings double as i18n keys (see packages/i18n: en is empty,
// t() falls back to the key). Each option's label/help is translated per-locale.
const OUTCOME_MODE_OPTIONS: ReadonlyArray<{ value: SchedulerOutcomeMode; label: string; help: string }> = [
  {
    value: "create_issue",
    label: "Create issues",
    help: "File a Pi Dash issue for each finding, skipping ones already tracked by an open issue.",
  },
  {
    value: "apply_fix",
    label: "Apply fix",
    help: "Implement the fix and open a pull request for review (never merged automatically). Risky or ambiguous findings become issues instead.",
  },
  {
    value: "fix_and_review",
    label: "Fix & open for review",
    help: "File an issue for each finding, implement the fix, open a pull request (never merged automatically), and move the issue straight to In Review for a human.",
  },
];

// Matches the backend default and pre-existing builtin behavior.
export const DEFAULT_OUTCOME_MODE: SchedulerOutcomeMode = "create_issue";

const outcomeModeHelp = (value: SchedulerOutcomeMode): string =>
  (OUTCOME_MODE_OPTIONS.find((o) => o.value === value) ??
    OUTCOME_MODE_OPTIONS.find((o) => o.value === DEFAULT_OUTCOME_MODE)!).help;

type Props<T extends FieldValues> = {
  control: Control<T>;
  /** RHF field holding the SchedulerOutcomeMode value. */
  name: Path<T>;
};

/**
 * Outcome-mode selector for the install/edit binding modals — what a run of
 * THIS install does with its findings. Lives on the binding (not the workspace
 * Scheduler), so the same scheduler can file issues on one project and open fix
 * PRs on another. Generic over the form values type so both modals reuse it.
 */
export function BindingOutcomeModeField<T extends FieldValues>({ control, name }: Props<T>) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-2">
      <span className="text-13 font-medium text-primary">{t("What to do with findings")}</span>
      <Controller
        control={control}
        name={name}
        render={({ field: { value, onChange } }) => {
          const current = (value as SchedulerOutcomeMode) ?? DEFAULT_OUTCOME_MODE;
          return (
            <div className="flex flex-col gap-2">
              <div className="flex flex-wrap gap-2" role="radiogroup" aria-label={t("What to do with findings")}>
                {OUTCOME_MODE_OPTIONS.map((opt) => {
                  const active = current === opt.value;
                  return (
                    <button
                      key={opt.value}
                      type="button"
                      role="radio"
                      aria-checked={active}
                      onClick={() => onChange(opt.value)}
                      className={`rounded-md border px-3 py-1.5 text-13 ${
                        active
                          ? "border-primary bg-primary/10 font-medium text-primary"
                          : "border-subtle text-secondary hover:text-primary"
                      }`}
                    >
                      {t(opt.label)}
                    </button>
                  );
                })}
              </div>
              <p className="text-12 text-secondary">{t(outcomeModeHelp(current))}</p>
            </div>
          );
        }}
      />
    </div>
  );
}
