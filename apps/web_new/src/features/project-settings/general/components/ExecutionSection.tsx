// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution target and automated-run policy
// (parity/specs/project-settings-general.md §PSET-009). The target picker
// disables families the instance cannot run and affects new runs only. The
// master switch leaves the budget and cadence visible but inert while off.
// The budget keeps whole numbers with a native floor of one and a no-cap
// toggle that remembers the typed cap; the cadence picker lists the presets
// and marks an out-of-band rhythm as custom.

import { Button, Combobox, Input, Switch } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import {
  CADENCE_OPTIONS,
  cadenceLabel,
  CUSTOM_CADENCE_LABEL,
  CUSTOM_CADENCE_VALUE,
  executorChoices,
  type GeneralFieldErrors,
  type GeneralFormValues,
  shapeBudget,
} from "../rules.js";

export interface ExecutionSectionProps {
  values: GeneralFormValues;
  errors: GeneralFieldErrors;
  disabled: boolean;
  /** Per-family availability arriving with the project row. */
  executorOptions: ReadonlyArray<{ kind: string; available: boolean }>;
  onChange: (patch: Partial<GeneralFormValues>) => void;
}

/** Choices for the cadence picker: presets, then the custom marker when it applies. */
export function cadenceChoices(current: string): Array<{ id: string; label: string }> {
  const presets = CADENCE_OPTIONS.map((option) => ({ id: String(option.seconds), label: option.label }));
  return current === CUSTOM_CADENCE_VALUE
    ? [...presets, { id: CUSTOM_CADENCE_VALUE, label: CUSTOM_CADENCE_LABEL }]
    : presets;
}

export function ExecutionSection({
  values,
  errors,
  disabled,
  executorOptions,
  onChange,
}: ExecutionSectionProps): React.ReactElement {
  const executors = executorChoices(executorOptions);
  const controlsDisabled = disabled || !values.ticking;
  return (
    <section aria-label="Execution" data-testid="general-execution" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <span className="text-body font-medium text-(--text)">Default execution target</span>
        <Combobox
          trigger={
            <Button
              variant="secondary"
              disabled={disabled}
              data-testid="general-executor-trigger"
              className="w-full justify-between"
            >
              <span data-testid="general-executor">{executorLabel(executors, values.executor)}</span>
              <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
            </Button>
          }
          options={executors.map((executor) => ({
            id: executor.kind,
            label: executor.label,
            disabled: executor.disabled,
          }))}
          value={values.executor}
          onSelect={(kind) => onChange({ executor: kind })}
          listLabel="Default execution target"
          searchEnabled={false}
          emptyText="No execution targets"
          disabled={disabled}
        />
        <p className="text-meta text-(--text-muted)">
          Changing the target affects new runs only. Queued and running work keeps its original executor.
        </p>
      </div>

      <div className="flex items-start justify-between gap-(--space-4)">
        <div className="flex flex-col gap-(--space-1)">
          <span className="text-body font-medium text-(--text)">Automated runs</span>
          <p className="text-meta text-(--text-muted)">
            While off, nothing in this project runs on a timer. Human-started runs still work.
          </p>
        </div>
        <Switch
          checked={values.ticking}
          disabled={disabled}
          label="Automated runs"
          data-testid="general-ticking"
          onCheckedChange={(next) => onChange({ ticking: next })}
        />
      </div>

      <div className="grid grid-cols-1 gap-(--space-6) md:grid-cols-2">
        <div className="flex flex-col gap-(--space-2)">
          <Input
            id="agent_default_max_ticks"
            name="agent_default_max_ticks"
            label="Run budget per work item"
            type="number"
            min={1}
            step={1}
            inputMode="numeric"
            value={values.budget}
            disabled={controlsDisabled}
            {...(errors.budget === undefined ? {} : { error: errors.budget })}
            onChange={(event) => {
              const budget = shapeBudget(event.target.value);
              onChange({ budget, lastCappedBudget: budget === "" ? values.lastCappedBudget : budget, noCap: false });
            }}
          />
          <label className="text-body flex items-center gap-(--space-2) text-(--text)">
            <input
              type="checkbox"
              checked={values.noCap}
              disabled={controlsDisabled}
              data-testid="general-nocap"
              onChange={(event) => {
                const noCap = event.target.checked;
                onChange({
                  noCap,
                  budget: noCap ? "" : values.lastCappedBudget,
                });
              }}
            />
            No cap
          </label>
          <p className="text-meta text-(--text-muted)">
            A new budget applies from the next run. Work already paused or scheduled keeps its own timing.
          </p>
        </div>

        <div className="flex flex-col gap-(--space-2)">
          <span className="text-body font-medium text-(--text)">Run cadence</span>
          <Combobox
            trigger={
              <Button
                variant="secondary"
                disabled={controlsDisabled}
                data-testid="general-cadence-trigger"
                className="w-full justify-between"
              >
                <span data-testid="general-cadence">{cadenceLabel(values.cadence)}</span>
                <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
              </Button>
            }
            options={cadenceChoices(values.cadence)}
            value={values.cadence}
            onSelect={(choice) => onChange({ cadence: choice })}
            listLabel="Run cadence"
            searchEnabled={false}
            emptyText="No cadence options"
            disabled={controlsDisabled}
          />
          <p className="text-meta text-(--text-muted)">
            One rhythm covers every stage. A new cadence applies from the next run.
          </p>
        </div>
      </div>
    </section>
  );
}

function executorLabel(executors: ReadonlyArray<{ kind: string; label: string }>, kind: string): string {
  return executors.find((executor) => executor.kind === kind)?.label ?? kind;
}
