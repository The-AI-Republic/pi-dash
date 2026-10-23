// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-estimate-system dialog (parity/specs/estimates.md §3; inventory
// EST-008–013). Step one picks the
// family and a starter card, step two arranges the values. Back returns
// to step one and drops the draft; Cancel and Escape close without a
// write; reopening starts over. Values validate inline and submit is
// blocked while any row is unclean.

import type { Estimate } from "@pidash/api-client";
import { Button, Dialog, IconButton, Input } from "@pidash/kit";
import { ArrowLeft, ChevronDown, ChevronUp, GripVertical, Pencil, Plus, Trash2 } from "lucide-react";
import * as React from "react";

import { edition } from "../../core/edition/index.js";
import { useCreateEstimate } from "./api.js";
import {
  ESTIMATE_FAMILY_POINTS,
  ESTIMATE_FAMILY_CATEGORIES,
  ESTIMATE_VALUE_MAX_LENGTH,
  VALUE_DUPLICATE,
  VALUE_NOT_CATEGORY,
  VALUE_NOT_NUMBER,
  VALUE_NOT_POSITIVE,
  VALUE_REQUIRED,
  CUSTOM_STARTER_ID,
  estimateFamilies,
  estimateFamilyEnabled,
  estimatePreview,
  estimateStarters,
  estimateValueLabel,
  familySystemName,
  moveValueTo,
  truncateEstimateValue,
  valueSetProblems,
  type EstimateFamily,
  type EstimateFamilyOption,
  type EstimateStarter,
  type ValueProblem,
} from "./model.js";

export const ESTIMATE_DIALOG_TITLE = "New estimate system";
export const ESTIMATE_DIALOG_DESCRIPTION = "Choose how your team measures effort, then arrange the values.";
export const ESTIMATE_STEP_ONE_HEADING = "Measurement family";
export const ESTIMATE_SCRATCH_HEADING = "Start from scratch";
export const ESTIMATE_TEMPLATE_HEADING = "Choose a template";
export const ESTIMATE_ADD_LABEL = "Add a value";
export const ESTIMATE_SUBMIT_LABEL = "Create estimate";

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
    <p id={id} tabIndex={-1} className="text-meta text-(--danger)">
      {message}
    </p>
  );
}

interface MoveAction {
  direction: "up" | "down";
  index: number;
}

function moveControlLabel(direction: "up" | "down", index: number): string {
  return `Move value ${index + 1} ${direction}`;
}

/** The first rule a draft breaks, or undefined when it can be committed. */
function draftProblem(
  family: EstimateFamily,
  values: readonly string[],
  index: number,
  draft: string
): string | undefined {
  const value = truncateEstimateValue(draft.trim());
  if (value === "") return VALUE_REQUIRED;
  const numeric = family !== ESTIMATE_FAMILY_CATEGORIES;
  if (numeric) {
    const parsed = Number(value);
    if (!Number.isFinite(parsed)) return VALUE_NOT_NUMBER;
    if (parsed <= 0) return VALUE_NOT_POSITIVE;
  } else if (!Number.isNaN(Number(value))) {
    return VALUE_NOT_CATEGORY;
  }
  const duplicates = values.some((other, at) => {
    if (at === index) return false;
    const committed = other.trim();
    return numeric ? Number(committed) === Number(value) : committed === value;
  });
  return duplicates ? VALUE_DUPLICATE : undefined;
}

export function EstimateCreateDialog({
  workspaceSlug,
  projectId,
  open,
  onOpenChange,
  onCreated,
}: EstimateCreateDialogProps): React.ReactElement {
  const families = React.useMemo(() => estimateFamilies(edition.flags), []);
  const [step, setStep] = React.useState<1 | 2>(1);
  const [family, setFamily] = React.useState<EstimateFamily>(ESTIMATE_FAMILY_POINTS);
  const [values, setValues] = React.useState<string[]>([]);
  const [editing, setEditing] = React.useState<{ index: number; draft: string } | null>(null);
  const [editingError, setEditingError] = React.useState<string | null>(null);
  const [dragIndex, setDragIndex] = React.useState<number | null>(null);
  const [submitted, setSubmitted] = React.useState(false);
  const create = useCreateEstimate(workspaceSlug, projectId);

  // Every opening is a fresh draft.
  React.useEffect(() => {
    if (!open) return;
    setStep(1);
    setFamily(ESTIMATE_FAMILY_POINTS);
    setValues([]);
    setEditing(null);
    setEditingError(null);
    setDragIndex(null);
    setSubmitted(false);
    create.reset();
    // create.reset is stable; re-running on open is the point.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const problems = valueSetProblems(family, values);
  // Committed rows surface their marker immediately; the row being edited
  // stays quiet until its draft is committed or submit reveals everything.
  const shownProblems = submitted ? problems : problems.filter((problem) => problem.index !== editing?.index);
  const firstProblemIndex = shownProblems[0]?.index;

  React.useEffect(() => {
    if (!submitted || firstProblemIndex === undefined) return;
    document.getElementById(`estimate-value-${firstProblemIndex}-problem`)?.focus();
  }, [submitted, firstProblemIndex]);

  const begin = (nextFamily: EstimateFamily, starter: EstimateStarter): void => {
    if (!estimateFamilyEnabled(nextFamily, edition.flags)) return;
    setFamily(nextFamily);
    setValues([...starter.values]);
    setEditing(null);
    setEditingError(null);
    setDragIndex(null);
    setSubmitted(false);
    setStep(2);
  };

  const back = (): void => {
    setValues([]);
    setEditing(null);
    setDragIndex(null);
    setSubmitted(false);
    setStep(1);
  };

  const commitValue = (index: number, draft: string): void => {
    const value = truncateEstimateValue(draft.trim());
    // An unchanged non-empty commit needs no write. An empty commit is
    // never clean, even when the row already holds an empty add placeholder:
    // keep the editor open and leave committed rows untouched (EST-012).
    if (value !== "" && value === (values[index] as string).trim()) {
      setEditing(null);
      setEditingError(null);
      return;
    }
    const problem = draftProblem(family, values, index, draft);
    if (problem !== undefined) {
      setEditingError(problem);
      setSubmitted(true);
      return;
    }
    setValues((current) => current.map((entry, at) => (at === index ? value : entry)));
    setEditing(null);
    setEditingError(null);
  };

  const closeEditor = (index: number): void => {
    if ((values[index] as string).trim() === "") {
      setValues((current) => current.filter((_, at) => at !== index));
    }
    setEditing(null);
    setEditingError(null);
  };

  const moveRow = ({ direction, index }: MoveAction): void => {
    const target = direction === "up" ? index - 1 : index + 1;
    setValues((current) => {
      const next = [...current];
      const moved = next[index] as string;
      next[index] = next[target] as string;
      next[target] = moved;
      return next;
    });
    setEditing(null);
    setEditingError(null);
    setDragIndex(null);
  };

  const dragValue = (sourceIndex: number, targetIndex: number, position: "before" | "after"): void => {
    setValues((current) => moveValueTo(current, sourceIndex, targetIndex, position));
    setEditing(null);
    setDragIndex(null);
  };

  const submit = (): void => {
    setSubmitted(true);
    const pending = editing;
    let committed = values;
    if (pending !== null) {
      const problem = draftProblem(family, values, pending.index, pending.draft);
      if (problem !== undefined) {
        setEditingError(problem);
        return;
      }
      committed = values.map((entry, at) =>
        at === pending.index ? truncateEstimateValue(pending.draft.trim()) : entry
      );
      setValues(committed);
      setEditing(null);
      setEditingError(null);
    }
    // Every committed row must be clean before the write. A pending edit was
    // folded in above; a blank row left behind by another action still stops
    // the request (EST-012, §3.6/§3.7).
    if (valueSetProblems(family, committed).length > 0) return;
    if (!estimateFamilyEnabled(family, edition.flags)) return;
    create.mutate(
      { name: familySystemName(family), type: family, values: committed.map((value) => value.trim()) },
      {
        onSuccess: (estimate) => {
          onCreated?.(estimate);
          onOpenChange(false);
        },
      }
    );
  };

  const starters = estimateStarters(family);
  const templates = starters.filter((starter) => starter.id !== CUSTOM_STARTER_ID);

  const renderFamilyRadio = (choice: EstimateFamilyOption): React.ReactElement => (
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
  );

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onOpenChange(false);
      }}
      title={ESTIMATE_DIALOG_TITLE}
      description={ESTIMATE_DIALOG_DESCRIPTION}
      nonDismissable={create.isPending}
      actions={
        step === 1 ? (
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
        ) : (
          <>
            <Button variant="secondary" disabled={create.isPending} onClick={back}>
              <ArrowLeft size={14} aria-hidden="true" />
              Back
            </Button>
            <Button variant="secondary" disabled={create.isPending} onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button variant="primary" loading={create.isPending} onClick={submit}>
              {ESTIMATE_SUBMIT_LABEL}
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
            <legend className="text-emphasis text-(--text)">{ESTIMATE_STEP_ONE_HEADING}</legend>
            <div className="flex flex-wrap gap-(--space-6)">{families.map(renderFamilyRadio)}</div>
          </fieldset>

          <div className="flex flex-col gap-(--space-2)">
            <p className="text-emphasis text-(--text)">{ESTIMATE_SCRATCH_HEADING}</p>
            {starters
              .filter((starter) => starter.id === CUSTOM_STARTER_ID)
              .map((starter) => (
                <button
                  key={starter.id}
                  type="button"
                  onClick={() => begin(family, starter)}
                  className="flex flex-col gap-(--space-1) rounded-(--radius-control) border border-(--border) p-(--space-3) text-left hover:bg-(--subtle)"
                >
                  <span className="text-emphasis text-(--text)">{starter.title}</span>
                  <span className="text-meta text-(--text-muted)">{estimatePreview(family, starter.values)}</span>
                </button>
              ))}
            <p className="text-emphasis mt-(--space-4) text-(--text)">{ESTIMATE_TEMPLATE_HEADING}</p>
            <div className="grid grid-cols-1 gap-(--space-2) sm:grid-cols-2">
              {templates.map((starter) => (
                <button
                  key={starter.id}
                  type="button"
                  onClick={() => begin(family, starter)}
                  className="flex flex-col gap-(--space-1) rounded-(--radius-control) border border-(--border) p-(--space-3) text-left hover:bg-(--subtle)"
                >
                  <span className="text-emphasis text-(--text)">{starter.title}</span>
                  <span className="text-meta text-(--text-muted)">{estimatePreview(family, starter.values)}</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      ) : (
        <div className="mt-(--space-4) flex flex-col gap-(--space-4)">
          <p className="text-emphasis text-(--text)">{familySystemName(family)}</p>
          <ul className="flex flex-col gap-(--space-2)">
            {values.map((value, index) => {
              const message = problemFor(shownProblems, index);
              const isEditing = editing !== null && editing.index === index;
              return (
                <li
                  key={index}
                  className="flex flex-col gap-(--space-1)"
                  onDragOver={(event) => {
                    event.preventDefault();
                  }}
                  onDrop={(event) => {
                    event.preventDefault();
                    if (dragIndex === null || dragIndex === index) {
                      setDragIndex(null);
                      return;
                    }
                    const bounds = event.currentTarget.getBoundingClientRect();
                    const after = bounds.height > 0 && event.clientY > bounds.top + bounds.height / 2;
                    dragValue(dragIndex, index, after ? "after" : "before");
                  }}
                >
                  <div className="flex items-center gap-(--space-2)">
                    <button
                      type="button"
                      aria-label={`Reorder value ${index + 1}`}
                      title="Drag to reorder"
                      draggable={!create.isPending}
                      onDragStart={() => {
                        setDragIndex(index);
                        setEditing(null);
                      }}
                      onDragEnd={() => setDragIndex(null)}
                      className="cursor-grab p-px text-(--text-muted)"
                    >
                      <GripVertical size={14} strokeWidth={2} aria-hidden="true" />
                    </button>
                    <span className="text-meta w-6 text-(--text-muted)">{index + 1}</span>
                    {isEditing ? (
                      <form
                        className="flex min-w-0 flex-1 items-center gap-(--space-2)"
                        onSubmit={(event) => {
                          event.preventDefault();
                          commitValue(editing.index, editing.draft);
                        }}
                      >
                        <Input
                          label={`Value ${index + 1}`}
                          value={editing.draft}
                          autoFocus
                          maxLength={ESTIMATE_VALUE_MAX_LENGTH}
                          disabled={create.isPending}
                          onChange={(event) => {
                            setEditingError(null);
                            setEditing({ index, draft: truncateEstimateValue(event.currentTarget.value) });
                          }}
                          onKeyDown={(event) => {
                            if (event.key === "Escape") {
                              event.preventDefault();
                              closeEditor(index);
                            }
                          }}
                        />
                        <Button type="submit" size="small" variant="secondary" disabled={create.isPending}>
                          Save
                        </Button>
                        <Button
                          type="button"
                          size="small"
                          variant="ghost"
                          disabled={create.isPending}
                          onClick={() => closeEditor(index)}
                        >
                          Cancel
                        </Button>
                      </form>
                    ) : (
                      <button
                        type="button"
                        data-testid="estimate-value"
                        className="text-body min-w-0 flex-1 truncate text-left text-(--text)"
                        onDoubleClick={() => {
                          if (!create.isPending) {
                            setEditingError(null);
                            setEditing({ index, draft: value });
                          }
                        }}
                      >
                        {estimateValueLabel(family, value)}
                      </button>
                    )}
                    {isEditing ? null : (
                      <IconButton
                        label={`Edit value ${index + 1}`}
                        disabled={create.isPending}
                        onClick={() => {
                          setEditingError(null);
                          setEditing({ index, draft: value });
                        }}
                      >
                        <Pencil size={14} aria-hidden="true" />
                      </IconButton>
                    )}
                    <IconButton
                      label={moveControlLabel("up", index)}
                      disabled={index === 0 || create.isPending}
                      onClick={() => moveRow({ direction: "up", index })}
                    >
                      <ChevronUp size={14} aria-hidden="true" />
                    </IconButton>
                    <IconButton
                      label={moveControlLabel("down", index)}
                      disabled={index === values.length - 1 || create.isPending}
                      onClick={() => moveRow({ direction: "down", index })}
                    >
                      <ChevronDown size={14} aria-hidden="true" />
                    </IconButton>
                    {values.length <= 2 ? null : (
                      <IconButton
                        label={`Remove value ${index + 1}`}
                        disabled={create.isPending}
                        onClick={() => {
                          setValues((current) => current.filter((_, at) => at !== index));
                          setEditing(null);
                          setEditingError(null);
                          setDragIndex(null);
                        }}
                      >
                        <Trash2 size={14} aria-hidden="true" />
                      </IconButton>
                    )}
                  </div>
                  <ProblemText
                    message={isEditing ? (editingError ?? message) : message}
                    id={`estimate-value-${index}-problem`}
                  />
                </li>
              );
            })}
          </ul>
          {values.length >= 6 ? null : (
            <Button
              variant="secondary"
              disabled={create.isPending}
              onClick={() => {
                setValues((current) => [...current, ""]);
                setEditing({ index: values.length, draft: "" });
                setEditingError(null);
                setDragIndex(null);
                setSubmitted(false);
              }}
            >
              <Plus size={14} aria-hidden="true" />
              {ESTIMATE_ADD_LABEL}
            </Button>
          )}
        </div>
      )}
    </Dialog>
  );
}
