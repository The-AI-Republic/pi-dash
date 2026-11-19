// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution target and automated-run policy
// (parity/specs/project-settings-general.md §PSET-009). There is no
// executor picker: projects run on runners and the default pod decides who
// runs the work (Pi Dash Agent design §6), so the section only says so.
// The master switch leaves the budget and cadence visible but inert while off.
// The budget keeps whole numbers with a native floor of one and a no-cap
// toggle that remembers the typed cap; the cadence picker lists the presets
// and marks an out-of-band rhythm as custom. The Stages block picks which
// stages run an agent: Auto lets the agent decide per work item; Customize
// toggles In Review and In Test (In Progress always runs).

import { Button, Combobox, Input, Menu, Switch } from "@pidash/kit";
import { Check, ChevronDown } from "lucide-react";
import * as React from "react";

import {
  CADENCE_OPTIONS,
  cadenceLabel,
  CUSTOM_CADENCE_VALUE,
  EXECUTION_TARGET_NOTE,
  type GeneralFieldErrors,
  type GeneralFormValues,
  shapeBudget,
  STAGES_HELP,
  STAGES_MODE_OPTIONS,
  type StagesMode,
} from "../rules.js";

export interface ExecutionSectionProps {
  values: GeneralFormValues;
  errors: GeneralFieldErrors;
  disabled: boolean;
  onChange: (patch: Partial<GeneralFormValues>) => void;
}

/**
 * Choices for the cadence picker: the presets only. A stored rhythm outside
 * them is described by the Custom caption on the trigger, never offered as
 * a pick (it names no interval, so picking it could only write garbage).
 */
export function cadenceChoices(): Array<{ id: string; label: string }> {
  return CADENCE_OPTIONS.map((option) => ({ id: String(option.seconds), label: option.label }));
}

/** Short label for the stage-toggle trigger: the stages that run. */
export function enabledStagesLabel(values: Pick<GeneralFormValues, "reviewStage" | "testStage">): string {
  const names = ["In Progress"];
  if (values.reviewStage) names.push("In Review");
  if (values.testStage) names.push("In Test");
  return names.join(", ");
}

function StagesBlock({
  values,
  disabled,
  onChange,
}: Pick<ExecutionSectionProps, "values" | "disabled" | "onChange">): React.ReactElement {
  const modeLabel = STAGES_MODE_OPTIONS.find((option) => option.id === values.stagesMode)?.label ?? "";
  const mark = (checked: boolean): React.ReactNode =>
    checked ? <Check size={16} strokeWidth={1.5} /> : <span className="inline-block w-4" />;
  return (
    <div className="flex flex-col gap-(--space-2)" data-testid="general-stages">
      <span className="text-body font-medium text-(--text)">Stages</span>
      <Combobox
        trigger={
          <Button
            variant="secondary"
            disabled={disabled}
            data-testid="general-stages-mode-trigger"
            className="w-full justify-between"
          >
            <span data-testid="general-stages-mode">{modeLabel}</span>
            <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
          </Button>
        }
        options={STAGES_MODE_OPTIONS.map((option) => ({ id: option.id, label: option.label }))}
        value={values.stagesMode}
        onSelect={(choice) => onChange({ stagesMode: choice as StagesMode })}
        listLabel="Stages"
        searchEnabled={false}
        emptyText="No stage options"
        disabled={disabled}
      />
      {values.stagesMode === "custom" ? (
        <Menu
          trigger={
            <Button
              variant="secondary"
              disabled={disabled}
              aria-label="Stages that run an agent"
              data-testid="general-stages-toggle-trigger"
              className="w-full justify-between"
            >
              <span data-testid="general-stages-enabled">{enabledStagesLabel(values)}</span>
              <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
            </Button>
          }
          items={[
            {
              label: "In Progress",
              description: "Always runs",
              icon: mark(true),
              disabled: true,
              onSelect: () => undefined,
            },
            {
              label: "In Review",
              icon: mark(values.reviewStage),
              closeOnClick: false,
              onSelect: () => onChange({ reviewStage: !values.reviewStage }),
            },
            {
              label: "In Test",
              icon: mark(values.testStage),
              closeOnClick: false,
              onSelect: () => onChange({ testStage: !values.testStage }),
            },
          ]}
        />
      ) : null}
      <p className="text-meta text-(--text-muted)">{STAGES_HELP}</p>
    </div>
  );
}

export function ExecutionSection({ values, errors, disabled, onChange }: ExecutionSectionProps): React.ReactElement {
  const controlsDisabled = disabled || !values.ticking;
  return (
    <section aria-label="Execution" data-testid="general-execution" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-1)">
        <span className="text-body font-medium text-(--text)">Runs on</span>
        <p className="text-meta text-(--text-muted)" data-testid="general-execution-target-note">
          {EXECUTION_TARGET_NOTE}
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
            options={cadenceChoices()}
            value={values.cadence}
            onSelect={(choice) => {
              if (choice !== CUSTOM_CADENCE_VALUE) onChange({ cadence: choice });
            }}
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

      <StagesBlock values={values} disabled={controlsDisabled} onChange={onChange} />
    </section>
  );
}
