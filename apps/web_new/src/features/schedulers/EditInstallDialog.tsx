// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edit one project install (parity/specs/agents.md §3, AGT-010). The same
// dialog opens from the list row, the install detail and the calendar
// drawer: start, zone, recurrence, project context, enabled flag, what
// runs do with their findings and the execution pod. It pre-fills from
// the stored binding, saves the whole set in one PATCH, and leaves itself
// open with a notice when the server refuses, so nothing typed is lost.

import type { SchedulerBinding } from "@pidash/api-client";
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { DEFAULT_TZID, isoToLocalStart, type ScheduleValue } from "./schedule.js";
import { ScheduleFields, useScheduleDraft } from "./ScheduleFields.js";
import { OutcomeModeField } from "./OutcomeModeField.js";
import { PodField } from "./PodField.js";
import { useUpdateSchedulerBinding } from "./installData.js";

/** The editable draft of one install, ready for the schedule editor. */
export interface InstallDraft {
  schedule: ScheduleValue;
  outcomeMode: SchedulerBinding["outcome_mode"];
  pod: string | null;
}

/**
 * The stored row as a draft. The start arrives as a UTC instant and is
 * shown as browser-local wall time; an install that stored no zone reads
 * as the default the server would apply anyway; the rule string is
 * passed through untouched so the builder can decide whether it is
 * expressible.
 */
export function installDraftFromBinding(binding: SchedulerBinding): InstallDraft {
  return {
    schedule: {
      startsAt: isoToLocalStart(binding.dtstart),
      timeZone: binding.tzid || DEFAULT_TZID,
      rrule: binding.rrule,
      projectContext: binding.extra_context,
      enabled: binding.enabled,
    },
    outcomeMode: binding.outcome_mode,
    pod: binding.pod,
  };
}

/**
 * The PATCH payload for a draft; an empty pod means the project default,
 * and a blank zone means the default zone. The rule travels exactly as
 * it sits in the draft: a stored rule the person never edited is saved
 * byte for byte, and a typed one is saved as typed.
 */
export function installUpdatePayload(draft: InstallDraft): {
  dtstart: string;
  tzid: string;
  rrule: string;
  extra_context: string;
  enabled: boolean;
  outcome_mode: SchedulerBinding["outcome_mode"];
  pod: string | null;
} {
  return {
    dtstart: new Date(draft.schedule.startsAt).toISOString(),
    tzid: draft.schedule.timeZone.trim() || DEFAULT_TZID,
    rrule: draft.schedule.rrule,
    extra_context: draft.schedule.projectContext.trim(),
    enabled: draft.schedule.enabled,
    outcome_mode: draft.outcomeMode,
    pod: draft.pod,
  };
}

export interface EditInstallDialogProps {
  workspaceSlug: string;
  projectId: string;
  /** The install being edited; null keeps the dialog closed. */
  binding: SchedulerBinding | null;
  onClose: () => void;
  /** Runs after a successful save so each caller can settle its own view. */
  onSaved?: (() => void) | undefined;
}

interface EditFormProps {
  workspaceSlug: string;
  projectId: string;
  binding: SchedulerBinding;
  onClose: () => void;
  onSaved: (() => void) | undefined;
}

function EditForm({ workspaceSlug, projectId, binding, onClose, onSaved }: EditFormProps): React.ReactElement {
  const update = useUpdateSchedulerBinding(workspaceSlug);
  const [draft] = React.useState<InstallDraft>(() => installDraftFromBinding(binding));
  const schedule = useScheduleDraft(draft.schedule);
  const [outcomeMode, setOutcomeMode] = React.useState(draft.outcomeMode);
  const [pod, setPod] = React.useState<string | null>(draft.pod);
  const pending = update.isPending;
  const idPrefix = `edit-install-${binding.id}`;

  async function submit(): Promise<void> {
    if (pending) return;
    if (!schedule.validate()) return;
    const next = installUpdatePayload({ schedule: schedule.value, outcomeMode, pod });
    try {
      await update.mutateAsync({ projectId, bindingId: binding.id, input: next });
      onSaved?.();
      onClose();
    } catch {
      // The mutation already raised the notice; the dialog stays open so
      // the typed values survive and can be corrected.
    }
  }

  return (
    <form
      className="flex flex-col gap-(--space-5)"
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <ScheduleFields
        idPrefix={idPrefix}
        value={schedule.value}
        errors={schedule.errors}
        disabled={pending}
        onChange={schedule.change}
      />
      <OutcomeModeField value={outcomeMode} disabled={pending} onChange={setOutcomeMode} />
      <PodField workspaceSlug={workspaceSlug} projectId={projectId} value={pod} disabled={pending} onChange={setPod} />
      <div className="flex justify-end gap-(--space-3)">
        <Button variant="ghost" disabled={pending} onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" loading={pending} disabled={pending}>
          {pending ? "Saving…" : "Save changes"}
        </Button>
      </div>
    </form>
  );
}

export function EditInstallDialog({
  workspaceSlug,
  projectId,
  binding,
  onClose,
  onSaved,
}: EditInstallDialogProps): React.ReactElement {
  return (
    <Dialog
      open={binding !== null}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title="Edit scheduler install"
      description={
        binding === null
          ? ""
          : `${binding.scheduler_name} on this project. Saving changes the schedule of every later run.`
      }
      className="max-w-3xl"
    >
      {binding === null ? null : (
        <EditForm
          key={binding.id}
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          binding={binding}
          onClose={onClose}
          onSaved={onSaved}
        />
      )}
    </Dialog>
  );
}
