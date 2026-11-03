// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop job create/edit modal (ADM-055/056/057, from
// parity/specs/admin.md §7.5–7.7). The list page mounts it blank for
// creates; the detail page (NEWFRONT-517) mounts it with the job for
// edits. Save gates on slug, both names, and prompt; failures toast
// the server text with a generic fallback and keep the modal open;
// success closes it and the mutations revalidate the list and job.
//
// Parity-driver contract (for the NEWFRONT-521 gate author): the
// dialog carries role="dialog" with an h2 titled "New job"/"Edit
// job"; fields hang off associated labels (Slug, Admin name,
// User-facing name, User-facing description, Prompt, Min role,
// Timezone, Recurrence rule); Min role is the only select, with
// option values 20/15/5; Save/Cancel are exact-name buttons; the
// body testid below is the capped scrolling region.
import { LOOP_MIN_ROLES, type LoopJob } from "@pidash/api-client";
import { Button, Dialog, FieldScaffold, Input, Textarea } from "@pidash/kit";
import * as React from "react";

import { useCreateLoopJob, useUpdateLoopJob } from "../hooks.js";
import { LOOP_MIN_ROLE_LABELS } from "../loopJobs.js";
import {
  buildLoopJobCreateInput,
  buildLoopJobPatchInput,
  isLoopJobSaveEnabled,
  isLoopJobSlugLocked,
  LOOP_JOB_FIELD_LABELS,
  LOOP_JOB_FORM_DEFAULTS,
  LOOP_JOB_MODAL_CREATE_TITLE,
  LOOP_JOB_MODAL_EDIT_TITLE,
  loopJobFormFromJob,
  type LoopJobFormFields,
} from "../loopJobModal.js";

export function AdminLoopJobModal({
  job,
  onClose,
}: {
  /** The job to edit; absent for creates. */
  job?: LoopJob | undefined;
  onClose: () => void;
}): React.ReactElement {
  const [fields, setFields] = React.useState<LoopJobFormFields>(() =>
    job === undefined ? { ...LOOP_JOB_FORM_DEFAULTS } : loopJobFormFromJob(job)
  );
  const creating = useCreateLoopJob();
  const updating = useUpdateLoopJob();
  const saving = creating.isPending || updating.isPending;
  const minRoleId = React.useId();
  const saveDisabled = !isLoopJobSaveEnabled(fields) || saving;

  const set = (patch: Partial<LoopJobFormFields>): void => {
    setFields((current) => ({ ...current, ...patch }));
  };

  const onSubmit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (!isLoopJobSaveEnabled(fields) || saving) return;
    if (job === undefined) {
      creating.mutate(buildLoopJobCreateInput(fields), {
        onSuccess: () => {
          onClose();
        },
      });
    } else {
      updating.mutate(
        { id: job.id, patch: buildLoopJobPatchInput(fields) },
        {
          onSuccess: () => {
            onClose();
          },
        }
      );
    }
  };

  return (
    <Dialog
      open
      onOpenChange={() => {
        onClose();
      }}
      title={job === undefined ? LOOP_JOB_MODAL_CREATE_TITLE : LOOP_JOB_MODAL_EDIT_TITLE}
    >
      <form data-testid="loop-job-modal-form" onSubmit={onSubmit} className="flex flex-col gap-(--space-6)">
        <div
          data-testid="loop-job-modal-body"
          className="flex max-h-[60vh] flex-col gap-(--space-4) overflow-y-auto pr-(--space-2)"
        >
          <Input
            label={LOOP_JOB_FIELD_LABELS.slug}
            value={fields.slug}
            disabled={isLoopJobSlugLocked(job)}
            onChange={(event) => {
              set({ slug: event.target.value });
            }}
            data-testid="loop-job-modal-slug"
          />
          <Input
            label={LOOP_JOB_FIELD_LABELS.name}
            value={fields.name}
            onChange={(event) => {
              set({ name: event.target.value });
            }}
            data-testid="loop-job-modal-name"
          />
          <Input
            label={LOOP_JOB_FIELD_LABELS.publicName}
            value={fields.publicName}
            onChange={(event) => {
              set({ publicName: event.target.value });
            }}
            data-testid="loop-job-modal-public-name"
          />
          <Textarea
            label={LOOP_JOB_FIELD_LABELS.publicDescription}
            value={fields.publicDescription}
            rows={2}
            onChange={(event) => {
              set({ publicDescription: event.target.value });
            }}
            data-testid="loop-job-modal-public-description"
          />
          <Textarea
            label={LOOP_JOB_FIELD_LABELS.prompt}
            value={fields.prompt}
            rows={4}
            onChange={(event) => {
              set({ prompt: event.target.value });
            }}
            data-testid="loop-job-modal-prompt"
          />
          <FieldScaffold label={LOOP_JOB_FIELD_LABELS.minRole} fieldId={minRoleId}>
            {(describedBy) => (
              <select
                id={minRoleId}
                aria-describedby={describedBy}
                value={String(fields.minRole)}
                onChange={(event) => {
                  set({ minRole: Number(event.target.value) });
                }}
                data-testid="loop-job-modal-min-role"
                className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) focus:border-(--accent) focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
              >
                {LOOP_MIN_ROLES.map((role) => (
                  <option key={role} value={role}>
                    {LOOP_MIN_ROLE_LABELS[role]}
                  </option>
                ))}
              </select>
            )}
          </FieldScaffold>
          <Input
            label={LOOP_JOB_FIELD_LABELS.tzid}
            value={fields.tzid}
            onChange={(event) => {
              set({ tzid: event.target.value });
            }}
            data-testid="loop-job-modal-tzid"
          />
          <Input
            label={LOOP_JOB_FIELD_LABELS.rrule}
            value={fields.rrule}
            onChange={(event) => {
              set({ rrule: event.target.value });
            }}
            data-testid="loop-job-modal-rrule"
          />
        </div>
        <div className="flex justify-end gap-(--space-3)">
          <Button type="button" variant="secondary" onClick={onClose} data-testid="loop-job-modal-cancel">
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={saveDisabled} data-testid="loop-job-modal-save">
            Save
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
