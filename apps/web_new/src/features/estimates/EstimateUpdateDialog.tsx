// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-system update dialog (parity/specs/estimates.md §4.1; inventory
// EST-014, EST-015). A cloud-only surface the screen mounts behind the
// edition flag: the system name plus its full value set with the same row
// affordances and validation as creation step two (§3.4–§3.6). Saving
// writes one arrangement across three API shapes — point deletes, point
// creates, then a PATCH of every id — and removing a saved value first
// asks whether affected work items move to another value or are cleared.

import type { Estimate } from "@pidash/api-client";
import { Button, Dialog, IconButton, Input } from "@pidash/kit";
import { ChevronDown, ChevronUp, GripVertical, Pencil, Plus, Trash2 } from "lucide-react";
import * as React from "react";

import { useUpdateEstimate, type UpdateEstimateRemoval } from "./api.js";
import {
  ESTIMATE_VALUE_MAX_LENGTH,
  estimateFamilyOfType,
  estimateValueLabel,
  familySystemName,
  moveValueTo,
  planEstimateUpdate,
  truncateEstimateValue,
  valueSetProblems,
  type ArrangedEstimateValue,
  type EstimateFamily,
  type ValueProblem,
} from "./model.js";

export const UPDATE_DIALOG_TITLE = "Edit estimate system";
export const UPDATE_DIALOG_DESCRIPTION =
  "Rename the system and rearrange its values. Saving writes every change at once.";
export const UPDATE_DIALOG_NAME_LABEL = "System name";
export const UPDATE_DIALOG_NAME_REQUIRED = "Give the system a name.";
export const UPDATE_DIALOG_ADD_LABEL = "Add a value";
export const UPDATE_DIALOG_SAVE_LABEL = "Save changes";
export const UPDATE_DIALOG_REMOVE_LABEL = "Remove value";

export interface EstimateUpdateDialogProps {
  workspaceSlug: string;
  projectId: string;
  system: Estimate;
  onClose: () => void;
}

interface ConfirmingRemoval {
  index: number;
  pointId: string;
  value: string;
}

function initialRows(system: Estimate): ArrangedEstimateValue[] {
  return [...system.points]
    .sort((left, right) => left.key - right.key)
    .map((point) => ({ id: point.id, value: point.value }));
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

function moveControlLabel(direction: "up" | "down", index: number): string {
  return `Move value ${index + 1} ${direction}`;
}

export function EstimateUpdateDialog({
  workspaceSlug,
  projectId,
  system,
  onClose,
}: EstimateUpdateDialogProps): React.ReactElement {
  const family: EstimateFamily = estimateFamilyOfType(system.type);
  const [original] = React.useState(() => system.points.map((point) => ({ id: point.id, key: point.key })));
  const [name, setName] = React.useState(system.name);
  const [nameError, setNameError] = React.useState<string | null>(null);
  const [rows, setRows] = React.useState<ArrangedEstimateValue[]>(() => initialRows(system));
  const [editing, setEditing] = React.useState<{ index: number; draft: string } | null>(null);
  const [editingError, setEditingError] = React.useState<string | null>(null);
  const [dragIndex, setDragIndex] = React.useState<number | null>(null);
  const [submitted, setSubmitted] = React.useState(false);
  const [confirming, setConfirming] = React.useState<ConfirmingRemoval | null>(null);
  const [choice, setChoice] = React.useState<"move" | "clear">("clear");
  const [replacementId, setReplacementId] = React.useState("");
  const [removedChoices, setRemovedChoices] = React.useState<UpdateEstimateRemoval[]>([]);
  const update = useUpdateEstimate(workspaceSlug, projectId);

  const problems = valueSetProblems(
    family,
    rows.map((row) => row.value)
  );
  // Committed rows surface their marker immediately; the row being edited
  // stays quiet until its draft is committed or saving reveals everything.
  const shownProblems = submitted ? problems : problems.filter((problem) => problem.index !== editing?.index);
  const firstProblemIndex = shownProblems[0]?.index;

  React.useEffect(() => {
    if (!submitted || firstProblemIndex === undefined) return;
    document.getElementById(`estimate-edit-value-${firstProblemIndex}-problem`)?.focus();
  }, [submitted, firstProblemIndex]);

  const commitValue = (index: number, draft: string): void => {
    const value = truncateEstimateValue(draft.trim());
    const current = rows[index] as ArrangedEstimateValue;
    // An unchanged non-empty commit needs no write. An empty commit is
    // never clean, even against an empty add placeholder: keep the editor
    // open and leave committed rows untouched (EST-012).
    if (value !== "" && value === current.value.trim()) {
      setEditing(null);
      setEditingError(null);
      return;
    }
    const candidate = rows.map((row, at) => (at === index ? value : row.value));
    const problem = valueSetProblems(family, candidate).find((entry) => entry.index === index);
    if (problem !== undefined) {
      setEditingError(problem.message);
      setSubmitted(true);
      return;
    }
    setRows((prev) => prev.map((row, at) => (at === index ? { ...row, value } : row)));
    setEditing(null);
    setEditingError(null);
  };

  const closeEditor = (index: number): void => {
    // Discarding a blank add placeholder drops it; a saved row keeps its
    // committed value — only the reassignment confirmation removes one.
    setRows((prev) => {
      const row = prev[index] as ArrangedEstimateValue;
      if (row.id === null && row.value.trim() === "") return prev.filter((_, at) => at !== index);
      return prev;
    });
    setEditing(null);
    setEditingError(null);
  };

  const moveRow = (direction: "up" | "down", index: number): void => {
    const target = direction === "up" ? index - 1 : index + 1;
    setRows((prev) => {
      const next = [...prev];
      const moved = next[index] as ArrangedEstimateValue;
      next[index] = next[target] as ArrangedEstimateValue;
      next[target] = moved;
      return next;
    });
    setEditing(null);
    setEditingError(null);
    setDragIndex(null);
  };

  const dragValue = (sourceIndex: number, targetIndex: number, position: "before" | "after"): void => {
    setRows((prev) => moveValueTo(prev, sourceIndex, targetIndex, position));
    setEditing(null);
    setDragIndex(null);
  };

  const beginRemove = (index: number): void => {
    const row = rows[index] as ArrangedEstimateValue;
    // A row that was never saved drops without a confirmation: no work
    // item can carry it. A saved row needs the admin's reassignment
    // choice first (§4.2).
    if (row.id === null) {
      setRows((prev) => prev.filter((_, at) => at !== index));
      setEditing(null);
      setEditingError(null);
      setDragIndex(null);
      return;
    }
    const firstCandidate = rows.find((candidate, at) => at !== index && candidate.id !== null);
    setChoice("clear");
    setReplacementId(firstCandidate?.id ?? "");
    setConfirming({ index, pointId: row.id, value: row.value });
  };

  const confirmRemoval = (): void => {
    // Reachable only from the confirmation view, which renders solely
    // while a removal is pending.
    const pending = confirming as ConfirmingRemoval;
    setRemovedChoices((prev) => [
      ...prev,
      choice === "move" ? { pointId: pending.pointId, newEstimateId: replacementId } : { pointId: pending.pointId },
    ]);
    setRows((prev) => prev.filter((_, at) => at !== pending.index));
    setConfirming(null);
    setEditing(null);
    setEditingError(null);
    setDragIndex(null);
  };

  const save = (): void => {
    setSubmitted(true);
    const cleanName = name.trim();
    if (cleanName === "") {
      setNameError(UPDATE_DIALOG_NAME_REQUIRED);
      return;
    }
    setNameError(null);
    const pending = editing;
    let committed = rows;
    if (pending !== null) {
      const value = truncateEstimateValue(pending.draft.trim());
      const candidate = rows.map((row, at) => (at === pending.index ? value : row.value));
      const problem = valueSetProblems(family, candidate).find((entry) => entry.index === pending.index);
      if (problem !== undefined) {
        setEditingError(problem.message);
        return;
      }
      committed = rows.map((row, at) => (at === pending.index ? { ...row, value } : row));
      setRows(committed);
      setEditing(null);
      setEditingError(null);
    }
    // Every committed row must be clean before the write. A pending edit
    // was folded in above; a blank row left behind by another action
    // still stops the request (EST-012, §3.6/§4.1).
    if (
      valueSetProblems(
        family,
        committed.map((row) => row.value)
      ).length > 0
    )
      return;
    const plan = planEstimateUpdate(original, committed);
    update.mutate(
      {
        estimateId: system.id,
        name: cleanName,
        survivors: plan.survivors,
        adds: plan.adds,
        removals: removedChoices.filter((entry) => plan.removals.includes(entry.pointId)),
      },
      {
        onSuccess: () => onClose(),
      }
    );
  };

  const candidates = confirming === null ? [] : rows.filter((row, at) => at !== confirming.index && row.id !== null);

  return (
    <Dialog
      open
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={UPDATE_DIALOG_TITLE}
      description={UPDATE_DIALOG_DESCRIPTION}
      nonDismissable={update.isPending}
      actions={
        confirming === null ? (
          <>
            <Button variant="secondary" disabled={update.isPending} onClick={onClose}>
              Cancel
            </Button>
            <Button variant="primary" loading={update.isPending} onClick={save}>
              {UPDATE_DIALOG_SAVE_LABEL}
            </Button>
          </>
        ) : (
          <>
            <Button variant="secondary" onClick={() => setConfirming(null)}>
              Cancel
            </Button>
            <Button variant="danger" onClick={confirmRemoval}>
              {UPDATE_DIALOG_REMOVE_LABEL}
            </Button>
          </>
        )
      }
    >
      {confirming === null ? (
        <div className="mt-(--space-4) flex flex-col gap-(--space-4)">
          <div className="flex flex-col gap-(--space-1)">
            <Input
              label={UPDATE_DIALOG_NAME_LABEL}
              value={name}
              disabled={update.isPending}
              onChange={(event) => {
                setNameError(null);
                setName(event.currentTarget.value);
              }}
            />
            {nameError === null ? null : <p className="text-meta text-(--danger)">{nameError}</p>}
          </div>
          <p className="text-emphasis text-(--text)">{familySystemName(family)}</p>
          <ul className="flex flex-col gap-(--space-2)">
            {rows.map((row, index) => {
              const message = problemFor(shownProblems, index);
              const isEditing = editing !== null && editing.index === index;
              return (
                <li
                  key={row.id ?? `new-${index}`}
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
                      draggable={!update.isPending}
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
                          disabled={update.isPending}
                          onChange={(event) => {
                            setEditingError(null);
                            setEditing({ index, draft: truncateEstimateValue(event.currentTarget.value) });
                          }}
                          onKeyDown={(event) => {
                            if (event.key === "Escape") {
                              event.preventDefault();
                              // Base UI listens for Escape on the document;
                              // keep the editor's dismiss from reaching the
                              // dialog and discarding the whole draft.
                              event.stopPropagation();
                              closeEditor(index);
                            }
                          }}
                        />
                        <Button type="submit" size="small" variant="secondary" disabled={update.isPending}>
                          Save
                        </Button>
                        <Button
                          type="button"
                          size="small"
                          variant="ghost"
                          disabled={update.isPending}
                          onClick={() => closeEditor(index)}
                        >
                          Cancel
                        </Button>
                      </form>
                    ) : (
                      <button
                        type="button"
                        data-testid="estimate-edit-value"
                        className="text-body min-w-0 flex-1 truncate text-left text-(--text)"
                        onDoubleClick={() => {
                          if (!update.isPending) {
                            setEditingError(null);
                            setEditing({ index, draft: row.value });
                          }
                        }}
                      >
                        {estimateValueLabel(family, row.value)}
                      </button>
                    )}
                    {isEditing ? null : (
                      <IconButton
                        label={`Edit value ${index + 1}`}
                        disabled={update.isPending}
                        onClick={() => {
                          setEditingError(null);
                          setEditing({ index, draft: row.value });
                        }}
                      >
                        <Pencil size={14} aria-hidden="true" />
                      </IconButton>
                    )}
                    <IconButton
                      label={moveControlLabel("up", index)}
                      disabled={index === 0 || update.isPending}
                      onClick={() => moveRow("up", index)}
                    >
                      <ChevronUp size={14} aria-hidden="true" />
                    </IconButton>
                    <IconButton
                      label={moveControlLabel("down", index)}
                      disabled={index === rows.length - 1 || update.isPending}
                      onClick={() => moveRow("down", index)}
                    >
                      <ChevronDown size={14} aria-hidden="true" />
                    </IconButton>
                    {rows.length <= 2 ? null : (
                      <IconButton
                        label={`Remove value ${index + 1}`}
                        disabled={update.isPending}
                        onClick={() => beginRemove(index)}
                      >
                        <Trash2 size={14} aria-hidden="true" />
                      </IconButton>
                    )}
                  </div>
                  <ProblemText
                    message={isEditing ? (editingError ?? message) : message}
                    id={`estimate-edit-value-${index}-problem`}
                  />
                </li>
              );
            })}
          </ul>
          {rows.length >= 6 ? null : (
            <Button
              variant="secondary"
              disabled={update.isPending}
              onClick={() => {
                setRows((prev) => [...prev, { id: null, value: "" }]);
                setEditing({ index: rows.length, draft: "" });
                setEditingError(null);
                setDragIndex(null);
                setSubmitted(false);
              }}
            >
              <Plus size={14} aria-hidden="true" />
              {UPDATE_DIALOG_ADD_LABEL}
            </Button>
          )}
        </div>
      ) : (
        <div className="mt-(--space-4) flex flex-col gap-(--space-4)">
          <p className="text-emphasis text-(--text)">Remove {estimateValueLabel(family, confirming.value)}?</p>
          <p className="text-body text-(--text-muted)">
            Work items using this value need somewhere to go. Move them to another value of this system, or clear their
            estimate.
          </p>
          {candidates.length > 0 ? (
            <>
              <label className="text-body flex items-center gap-(--space-2) text-(--text)">
                <input
                  type="radio"
                  name="estimate-value-delete-choice"
                  checked={choice === "move"}
                  onChange={() => setChoice("move")}
                />
                Move affected work items to another value
              </label>
              <div className="flex flex-col gap-(--space-2)">
                <label htmlFor="estimate-value-delete-replacement" className="text-body font-medium text-(--text)">
                  Replacement value
                </label>
                <select
                  id="estimate-value-delete-replacement"
                  value={replacementId}
                  disabled={choice !== "move"}
                  className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
                  onChange={(event) => setReplacementId(event.currentTarget.value)}
                >
                  {candidates.map((candidate) => (
                    <option key={candidate.id as string} value={candidate.id as string}>
                      {estimateValueLabel(family, candidate.value)}
                    </option>
                  ))}
                </select>
              </div>
            </>
          ) : (
            <p className="text-body text-(--text-muted)">
              No other saved value exists yet, so clearing is the only choice.
            </p>
          )}
          <label className="text-body flex items-center gap-(--space-2) text-(--text)">
            <input
              type="radio"
              name="estimate-value-delete-choice"
              checked={choice === "clear"}
              onChange={() => setChoice("clear")}
            />
            Clear the estimate on affected work items
          </label>
        </div>
      )}
    </Dialog>
  );
}
