// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Epic create/edit dialog (parity/specs/issues.md §20/§23, ISS-232).
// Epics are a cloud capability: the community build renders nothing here
// even when a relation or sub-issue flow reaches for it, which keeps the
// epic branches throughout the issues code inert. A cloud build turns the
// flag on and the dialog writes through the epics contracts, so an epic
// created or edited from relations and sub-items lands on the server.
//
// The form carries the fields the work-item form does — title,
// description, state, priority, assignees, labels and the date window —
// because an epic is a work item stored under its own collection.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import type { Issue } from "@pidash/api-client";
import { createEpic, updateEpic } from "@pidash/api-client";
import { getAppClient } from "../../../core/api/client.js";
import type { PriorityLevel } from "../../../shared/pickers/index.js";
import { MemberPicker, PriorityPicker, StatePicker } from "../../../shared/pickers/index.js";
import { DatePicker } from "../dropdowns/DatePicker.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { LabelPicker } from "../labels/LabelPicker.js";
import {
  canSubmitEpic,
  EPIC_NAME_REQUIRED,
  epicCreateOf,
  epicDraftFrom,
  epicPatchOf,
  epicTitle,
  type EpicDialogValues,
} from "./epicModel.js";
import { EPICS_FLAG, isEditionFlagOn } from "./flags.js";
import { stateOptionGate } from "./workflow.js";

export { EPICS_FLAG };

/** Name field cap, matching the work-item form (ISS-118). */
export const EPIC_NAME_MAX = 255;

export interface EpicDialogProps {
  workspaceSlug: string;
  projectId: string;
  /** The epic being edited; absent for a create. */
  epic?: Issue | undefined;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Fired once the server accepted the write. */
  onSaved?: (() => void) | undefined;
}

export function isEpicDialogEnabled(): boolean {
  return isEditionFlagOn(EPICS_FLAG);
}

export function EpicDialog({
  workspaceSlug,
  projectId,
  epic,
  open,
  onOpenChange,
  onSaved,
}: EpicDialogProps): React.ReactElement | null {
  const [values, setValues] = React.useState<EpicDialogValues>(() => epicDraftFrom(epic));
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const editing = epic !== undefined;
  const enabled = isEpicDialogEnabled();
  // The workflow gate applies here as it does everywhere a state moves
  // (ISS-233); without it this is undefined and the picker stays plain.
  const stateGate = stateOptionGate(projectId, epic?.state_id ?? null);

  // Each open starts from the row it edits (or a blank create).
  React.useEffect(() => {
    if (!open) return;
    setValues(epicDraftFrom(epic));
    setError(null);
  }, [open, epic]);

  if (!enabled) return null;

  const set = <K extends keyof EpicDialogValues>(key: K, value: EpicDialogValues[K]): void => {
    setValues((current) => ({ ...current, [key]: value }));
  };

  async function submit(): Promise<void> {
    if (!canSubmitEpic(values)) {
      setError(EPIC_NAME_REQUIRED);
      return;
    }
    setBusy(true);
    try {
      const client = getAppClient();
      if (editing) {
        await updateEpic(client, workspaceSlug, projectId, epic.id, epicPatchOf(values));
        notifySuccess("Epic updated");
      } else {
        await createEpic(client, workspaceSlug, projectId, epicCreateOf(values));
        notifySuccess("Epic created");
      }
      onOpenChange(false);
      onSaved?.();
    } catch (cause) {
      const message = (cause as Error).message;
      setError(message);
      notifyError("Couldn't save the epic", message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!busy) onOpenChange(next);
      }}
      title={epicTitle(editing)}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => void submit()} disabled={busy}>
            {busy ? "Saving…" : editing ? "Save epic" : "Create epic"}
          </Button>
        </>
      }
    >
      <div data-testid="epic-dialog" className="flex flex-col gap-(--space-4)">
        <Input
          label="Epic name"
          value={values.name}
          onChange={(event) => set("name", event.target.value)}
          disabled={busy}
          autoComplete="off"
          maxLength={EPIC_NAME_MAX}
          {...(error === null ? {} : { error })}
        />
        <Input
          label="Description"
          value={values.descriptionHtml}
          onChange={(event) => set("descriptionHtml", event.target.value)}
          disabled={busy}
          autoComplete="off"
        />
        <div className="grid grid-cols-2 gap-(--space-3)">
          <label className="text-caption flex flex-col gap-(--space-1) text-(--text-muted)">
            State
            <StatePicker
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              value={values.stateId}
              disabled={busy}
              placeholder="Set state"
              preload={values.stateId !== null}
              {...(stateGate === undefined ? {} : { optionGate: stateGate })}
              onChange={(stateId) => set("stateId", stateId)}
            />
          </label>
          <label className="text-caption flex flex-col gap-(--space-1) text-(--text-muted)">
            Priority
            <PriorityPicker
              value={values.priority as PriorityLevel}
              disabled={busy}
              onChange={(priority) => set("priority", priority)}
            />
          </label>
          <label className="text-caption flex flex-col gap-(--space-1) text-(--text-muted)">
            Assignees
            <MemberPicker
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              multiple
              value={values.assigneeIds}
              disabled={busy}
              placeholder="Add assignees"
              preload={values.assigneeIds.length > 0}
              onChange={(ids) => set("assigneeIds", ids)}
            />
          </label>
          <label className="text-caption flex flex-col gap-(--space-1) text-(--text-muted)">
            Labels
            <LabelPicker
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              value={values.labelIds}
              disabled={busy}
              onChange={(ids) => set("labelIds", ids)}
            />
          </label>
          <label className="text-caption flex flex-col gap-(--space-1) text-(--text-muted)">
            Start date
            <DatePicker
              label="Start date"
              value={values.startDate}
              max={values.targetDate}
              disabled={busy}
              onChange={(iso) => set("startDate", iso)}
            />
          </label>
          <label className="text-caption flex flex-col gap-(--space-1) text-(--text-muted)">
            Due date
            <DatePicker
              label="Due date"
              value={values.targetDate}
              min={values.startDate}
              disabled={busy}
              onChange={(iso) => set("targetDate", iso)}
            />
          </label>
        </div>
      </div>
    </Dialog>
  );
}
