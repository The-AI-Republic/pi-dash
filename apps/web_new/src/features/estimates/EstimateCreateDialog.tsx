// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-estimate-system dialog (parity/specs/project-settings-part-f.md
// §Estimates entry, PSET-049; inventory EST-008–013). Step one picks the
// family and a starter card, step two arranges the values. Back returns
// to step one and drops the draft; Cancel and Escape close without a
// write; reopening starts over. Values validate inline and submit is
// blocked while any row is unclean.

import type { Estimate } from "@pidash/api-client";
import { Button, Dialog, IconButton, Input } from "@pidash/kit";
import { ArrowLeft, Pencil, Plus, Trash2 } from "lucide-react";
import * as React from "react";

import { useCreateEstimate } from "./api.js";
import {
  ESTIMATE_FAMILIES,
  ESTIMATE_VALUE_MAX_LENGTH,
  estimateStarters,
  familySystemName,
  renumberValues,
  valueSetProblems,
  type EstimateFamily,
  type EstimateStarter,
  type ValueProblem,
} from "./model.js";

export const ESTIMATE_DIALOG_TITLE = "New estimate system";
export const ESTIMATE_DIALOG_DESCRIPTION = "Choose how your team measures effort, then arrange the values.";

export interface EstimateCreateDialogProps {
  workspaceSlug: string;
  projectId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated?: ((estimate: Estimate) => void) | undefined;
}

function problemFor(problems: readonly ValueProblem[], index: number): string | undefined {
  return problems.find((problem) => problem.index === index)?.message;
}

function ProblemText({ message, id }: { message: string | undefined; id: string }): React.ReactElement | null {
  if (message === undefined) return null;
  return (
    <p id={id} className="text-meta text-(--danger)">
      {message}
    </p>
  );
}

export function EstimateCreateDialog({
  workspaceSlug,
  projectId,
  open,
  onOpenChange,
  onCreated,
}: EstimateCreateDialogProps): React.ReactElement {
  const [step, setStep] = React.useState<1 | 2>(1);
  const [family, setFamily] = React.useState<EstimateFamily>("points");
  const [starterId, setStarterId] = React.useState<string | null>(null);
  const [values, setValues] = React.useState<string[]>([]);
  const [editing, setEditing] = React.useState<{ index: number; draft: string } | null>(null);
  const [submitted, setSubmitted] = React.useState(false);
  const create = useCreateEstimate(workspaceSlug, projectId);

  // Every opening is a fresh draft.
  React.useEffect(() => {
    if (!open) return;
    setStep(1);
    setFamily("points");
    setStarterId(null);
    setValues([]);
    setEditing(null);
    setSubmitted(false);
    create.reset();
    // create.reset is stable; re-running on open is the point.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const problems = valueSetProblems(family, values);
  const blocked = problems.length > 0;

  const begin = (nextFamily: EstimateFamily, starter: EstimateStarter): void => {
    setFamily(nextFamily);
    setStarterId(starter.id);
    setValues([...starter.values]);
    setEditing(null);
    setSubmitted(false);
    setStep(2);
  };

  const back = (): void => {
    setStarterId(null);
    setValues([]);
    setEditing(null);
    setSubmitted(false);
    setStep(1);
  };

  const commitValue = (index: number, draft: string): void => {
    setValues((current) => current.map((value, at) => (at === index ? draft : value)));
    setEditing(null);
  };

  const submit = (): void => {
    setSubmitted(true);
    if (blocked || starterId === null) return;
    create.mutate(
      { name: familySystemName(family), type: family, values: values.map((value) => value.trim()) },
      {
        onSuccess: (estimate) => {
          onCreated?.(estimate);
          onOpenChange(false);
        },
      }
    );
  };

  const starters = estimateStarters(family);

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onOpenChange(false);
      }}
      title={ESTIMATE_DIALOG_TITLE}
      description={ESTIMATE_DIALOG_DESCRIPTION}
      actions={
        step === 1 ? (
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
        ) : (
          <>
            <Button variant="secondary" onClick={back}>
              <ArrowLeft size={14} aria-hidden="true" />
              Back
            </Button>
            <Button variant="secondary" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button variant="primary" loading={create.isPending} disabled={submitted && blocked} onClick={submit}>
              Create estimate
            </Button>
          </>
        )
      }
    >
      <p className="text-meta text-(--text-muted)" data-testid="estimate-dialog-step">
        Step {step} of 2
      </p>

      {step === 1 ? (
        <div className="mt-(--space-4) flex flex-col gap-(--space-6)">
          <fieldset className="flex flex-col gap-(--space-2)">
            <legend className="text-emphasis text-(--text)">Choose an estimate system</legend>
            <div className="flex flex-wrap gap-(--space-6)">
              {ESTIMATE_FAMILIES.map((choice) => (
                <label key={choice.value} className="text-body flex items-center gap-(--space-2) text-(--text)">
                  <input
                    type="radio"
                    name="estimate-family"
                    value={choice.value}
                    checked={family === choice.value}
                    onChange={() => setFamily(choice.value)}
                  />
                  {choice.label}
                </label>
              ))}
            </div>
          </fieldset>

          <div className="flex flex-col gap-(--space-2)">
            <p className="text-emphasis text-(--text)">Pick a starting point</p>
            <div className="grid grid-cols-1 gap-(--space-3) sm:grid-cols-2">
              {starters.map((starter) => (
                <button
                  key={starter.id}
                  type="button"
                  onClick={() => begin(family, starter)}
                  className="flex flex-col gap-(--space-1) rounded-(--radius-control) border border-(--border) p-(--space-3) text-left hover:bg-(--subtle)"
                >
                  <span className="text-emphasis text-(--text)">{starter.title}</span>
                  <span className="text-meta text-(--text-muted)">{starter.values.join(", ")}</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      ) : (
        <div className="mt-(--space-4) flex flex-col gap-(--space-4)">
          <ul className="flex flex-col gap-(--space-2)">
            {values.map((value, index) => {
              const message = submitted ? problemFor(problems, index) : undefined;
              return (
                <li key={index} className="flex flex-col gap-(--space-1)">
                  <div className="flex items-center gap-(--space-2)">
                    <span className="text-meta w-6 text-(--text-muted)">{index + 1}</span>
                    {editing !== null && editing.index === index ? (
                      <Input
                        label={`Value ${index + 1}`}
                        value={editing.draft}
                        maxLength={ESTIMATE_VALUE_MAX_LENGTH}
                        onChange={(event) => setEditing({ index, draft: event.currentTarget.value })}
                        onBlur={() => commitValue(editing.index, editing.draft)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") commitValue(editing.index, editing.draft);
                        }}
                      />
                    ) : (
                      <button
                        type="button"
                        data-testid="estimate-value"
                        className="text-body min-w-0 flex-1 truncate text-left text-(--text)"
                        onDoubleClick={() => setEditing({ index, draft: value })}
                      >
                        {value}
                      </button>
                    )}
                    <IconButton label={`Edit value ${index + 1}`} onClick={() => setEditing({ index, draft: value })}>
                      <Pencil size={14} aria-hidden="true" />
                    </IconButton>
                    <IconButton
                      label={`Remove value ${index + 1}`}
                      disabled={values.length <= 2}
                      onClick={() => {
                        setValues((current) =>
                          renumberValues(current.filter((_, at) => at !== index)).map((p) => p.value)
                        );
                        setEditing(null);
                      }}
                    >
                      <Trash2 size={14} aria-hidden="true" />
                    </IconButton>
                  </div>
                  <ProblemText message={message} id={`estimate-value-${index}-problem`} />
                </li>
              );
            })}
          </ul>
          <Button
            variant="secondary"
            disabled={values.length >= 6}
            onClick={() => {
              setValues((current) => [...current, ""]);
              setEditing(null);
            }}
          >
            <Plus size={14} aria-hidden="true" />
            Add a value
          </Button>
        </div>
      )}
    </Dialog>
  );
}
