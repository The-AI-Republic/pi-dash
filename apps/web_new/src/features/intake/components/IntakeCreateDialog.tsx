// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake request create dialog (parity/specs/intake.md INT-013–017,
// INT-033). A title, the shared description panel and the inline
// property strip — intake state, priority, assignees, labels, due date.
// Saving files the request through the intake-create endpoint, links
// the description images parked while composing, and either opens the
// new request or resets for the next one. While the description panel
// reports work in flight, close, submit and Escape all wait on it with
// a notice instead of discarding the draft. Composing also feeds the
// shared duplicate seam behind the cloud flag (INT-018).

import { Button, Dialog, Input, Switch } from "@pidash/kit";
import { ISSUE_DESCRIPTION_ENTITY_TYPE } from "@pidash/api-client";
import * as React from "react";

import { edition, ISSUE_DEDUPE_FLAG } from "../../../core/edition/index.js";
import { IntakeStatePicker, MemberPicker, PriorityPicker } from "../../../shared/pickers/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useAppInstanceBootstrap } from "../../instance/index.js";
import {
  DatePicker,
  LabelPicker,
  ModalDescription,
  useAttachDescriptionAssets,
  useDuplicateMatches,
  useWeekStart,
} from "../../issues/index.js";
import { useCreateIntakeRequest } from "../api/create.js";
import {
  buildIntakeCreatePayload,
  emptyIntakeCreateForm,
  intakeTitleError,
  isIntakeTitleSubmittable,
  type IntakeCreateForm,
} from "../lib/createForm.js";
import { IntakeDuplicateIndicator } from "./IntakeDuplicateIndicator.js";

export const INTAKE_CREATE_DIALOG_TITLE = "New intake request";
export const INTAKE_CREATE_SUBMIT_LABEL = "Create request";
export const INTAKE_CREATE_SAVING_LABEL = "Creating…";
export const INTAKE_CREATE_CANCEL_LABEL = "Cancel";
export const INTAKE_CREATE_MORE_LABEL = "Create another";
/** Success notice after the request is filed. */
export const INTAKE_CREATED_TITLE = "Request created";
export const INTAKE_CREATED_BODY = "The request is waiting in the Open tab.";
/** Failure notice; the dialog stays open with the draft intact. */
export const INTAKE_CREATE_FAILED_TITLE = "Couldn't create the request";
export const INTAKE_CREATE_FAILED_BODY = "Nothing was created. Check the form and try again.";
/** Wait notice while the description panel still has work in flight. */
export const INTAKE_EDITOR_BUSY_TITLE = "Editor still processing";
export const INTAKE_EDITOR_BUSY_BODY = "The description is still processing. Wait a moment, then try again.";

export interface IntakeCreateDialogProps {
  workspaceSlug: string;
  projectId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The created request's work-item id, after the dialog closes. */
  onCreated: (issueId: string) => void;
}

function notifyWait(): void {
  notifyError(INTAKE_EDITOR_BUSY_TITLE, INTAKE_EDITOR_BUSY_BODY);
}

export function IntakeCreateDialog({
  workspaceSlug,
  projectId,
  open,
  onOpenChange,
  onCreated,
}: IntakeCreateDialogProps): React.ReactElement {
  const [form, setForm] = React.useState<IntakeCreateForm>(() => emptyIntakeCreateForm());
  const [createMore, setCreateMore] = React.useState(false);
  const [descriptionReady, setDescriptionReady] = React.useState(true);
  const [assetIds, setAssetIds] = React.useState<string[]>([]);
  const [editorGeneration, setEditorGeneration] = React.useState(0);
  const titleRef = React.useRef<HTMLInputElement>(null);
  const submitting = React.useRef(false);

  const create = useCreateIntakeRequest(workspaceSlug, projectId);
  const attach = useAttachDescriptionAssets(workspaceSlug);
  const instance = useAppInstanceBootstrap();
  const weekStart = useWeekStart();
  const writingHelp = instance.data?.bootstrap?.config.has_llm_configured === true;

  const busy = create.isPending;
  const titleHint = intakeTitleError(form.name);
  const submittable = isIntakeTitleSubmittable(form.name);
  // Compose-time duplicate detection (INT-018): the shared seam reads
  // the draft title while the dialog is open. With the flag off (every
  // OSS build) the hook short-circuits: no read, no indicator.
  const duplicates = useDuplicateMatches({
    workspaceSlug,
    projectId,
    title: form.name,
    enabled: open && edition.flags[ISSUE_DEDUPE_FLAG] === true,
  });

  function patch(next: Partial<IntakeCreateForm>): void {
    setForm((current) => ({ ...current, ...next }));
  }

  /** A cleared form for the next request; the toggle outlives the save. */
  function resetForNext(): void {
    setForm(emptyIntakeCreateForm());
    setAssetIds([]);
    setEditorGeneration((current) => current + 1);
  }

  function requestClose(): void {
    if (!descriptionReady) {
      notifyWait();
      return;
    }
    resetForNext();
    setCreateMore(false);
    onOpenChange(false);
  }

  async function submitNow(): Promise<void> {
    if (submitting.current) return;
    if (!descriptionReady) {
      notifyWait();
      return;
    }
    if (!submittable) return;
    submitting.current = true;
    create.mutate(buildIntakeCreatePayload(form), {
      onSettled: () => {
        submitting.current = false;
      },
      onSuccess: (made) => {
        if (assetIds.length > 0) {
          attach.mutate({ projectId, issueId: made.issue.id, assetIds });
        }
        notifySuccess(INTAKE_CREATED_TITLE, INTAKE_CREATED_BODY);
        if (!createMore) {
          resetForNext();
          setCreateMore(false);
          onOpenChange(false);
          onCreated(made.issue.id);
          return;
        }
        resetForNext();
      },
      onError: () => {
        notifyError(INTAKE_CREATE_FAILED_TITLE, INTAKE_CREATE_FAILED_BODY);
      },
    });
  }

  return (
    <Dialog
      title={INTAKE_CREATE_DIALOG_TITLE}
      description="Describe the request; it lands in the Open tab for triage."
      open={open}
      onOpenChange={() => requestClose()}
      className="max-w-2xl"
      initialFocus={titleRef}
      actions={
        <>
          <Button variant="ghost" onClick={requestClose} disabled={busy}>
            {INTAKE_CREATE_CANCEL_LABEL}
          </Button>
          <Button
            type="submit"
            form="intake-create-form"
            variant="primary"
            disabled={!submittable || busy}
            data-testid="intake-create-submit"
          >
            {busy ? INTAKE_CREATE_SAVING_LABEL : INTAKE_CREATE_SUBMIT_LABEL}
          </Button>
        </>
      }
    >
      <form
        id="intake-create-form"
        data-testid="intake-create-form"
        onSubmit={(event) => {
          event.preventDefault();
          void submitNow();
        }}
        className="flex flex-col gap-(--space-4)"
      >
        <Input
          id="intake-create-title"
          inputRef={titleRef}
          label="Title"
          placeholder="Title"
          value={form.name}
          onChange={(event) => patch({ name: event.target.value })}
          {...(titleHint === null ? {} : { error: titleHint })}
          disabled={busy}
          autoComplete="off"
          data-testid="intake-create-title"
        />
        <IntakeDuplicateIndicator matches={duplicates.matches} />
        <ModalDescription
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={form.descriptionHtml}
          onChange={(html) => patch({ descriptionHtml: html })}
          disabled={busy}
          title={form.name}
          assistantConfigured={writingHelp}
          assetScope={{ entityType: ISSUE_DESCRIPTION_ENTITY_TYPE, entityId: "" }}
          onAssetUploaded={(assetId) => setAssetIds((ids) => [...ids, assetId])}
          generation={editorGeneration}
          onReadinessChange={setDescriptionReady}
        />
        <div className="flex flex-wrap items-center gap-(--space-2)">
          <div role="group" aria-label="Intake state">
            <IntakeStatePicker
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              value={form.stateId}
              onChange={(id) => patch({ stateId: id })}
              disabled={busy}
            />
          </div>
          <div role="group" aria-label="Priority">
            <PriorityPicker value={form.priority} onChange={(level) => patch({ priority: level })} disabled={busy} />
          </div>
          <div role="group" aria-label="Assignees">
            <MemberPicker
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              multiple
              value={form.assigneeIds}
              onChange={(ids) => patch({ assigneeIds: ids })}
              placeholder="Assignees"
              disabled={busy}
            />
          </div>
          <div role="group" aria-label="Labels">
            <LabelPicker
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              value={form.labelIds}
              onChange={(ids) => patch({ labelIds: ids })}
              disabled={busy}
            />
          </div>
          <div role="group" aria-label="Due date">
            <DatePicker
              value={form.targetDate}
              onChange={(iso) => patch({ targetDate: iso })}
              weekStart={weekStart}
              label="Due date"
              disabled={busy}
            />
          </div>
        </div>
        <div role="group" aria-label={INTAKE_CREATE_MORE_LABEL} className="flex items-center gap-(--space-2)">
          <Switch label={INTAKE_CREATE_MORE_LABEL} checked={createMore} onCheckedChange={setCreateMore} />
          <span aria-hidden="true" className="text-body text-(--text)">
            {INTAKE_CREATE_MORE_LABEL}
          </span>
        </div>
      </form>
    </Dialog>
  );
}
