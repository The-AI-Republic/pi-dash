// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item create dialog (parity/specs/issues.md §11, ISS-117–123 and
// the §21 shell seam SHELL-086/047). The one creation form behind the
// sidebar button, the issues header, the palette command and the
// layout quick-adds: a project selector, the title (which takes focus),
// a rich description and the property strip, with Create-more and a
// discard guard. The rules live in create/createForm.ts, the network
// calls in create/createApi.ts, and the parent search and draft prompt
// in their own modules. The form is create-only; the edit/move context
// is NEWFRONT-631.

import { Button, Dialog, Input, Switch } from "@pidash/kit";
import * as React from "react";

import type { IssueSearchHit } from "@pidash/api-client";

import { edition } from "../../../core/edition/index.js";
import { RichTextEditor } from "../../../shared/editor/index.js";
import {
  CyclePicker,
  usePickerEstimates,
  EstimatePicker,
  MemberPicker,
  ModulePicker,
  PodPicker,
  PriorityPicker,
  ProjectPicker,
  StatePicker,
  usePickerProject,
  usePickerProjects,
  type ExecutionSelection,
  type PriorityLevel,
} from "../../../shared/pickers/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { isAssistantConfigured, useAssistantConfig } from "../../home/index.js";
import { useCreateWorkItem } from "../api/create.js";
import { DiscardDraftDialog } from "../create/DiscardDraftDialog.js";
import {
  DRAFT_SAVE_FAILED_INLINE_TITLE,
  DRAFT_SAVED_INLINE_TITLE,
  TITLE_EMPTY_DESCRIPTION_BODY,
  TITLE_GENERATION_EMPTY_BODY,
  TITLE_GENERATION_FAILED_BODY,
  TITLE_GENERATION_FAILED_TITLE,
  TITLE_GENERATION_TIMED_OUT_BODY,
  TitleTimeoutError,
  MEMBERSHIP_FAILED_BODY,
  MEMBERSHIP_FAILED_TITLE,
  useGenerateTitle,
  useApplyCreateMembership,
  useSaveDraft,
} from "../create/createApi.js";
import {
  buildCreatePayload,
  buildDraftInput,
  canGenerateTitle,
  descriptionText,
  emptyCreateForm,
  hasDraftWorthKeeping,
  hasMembership,
  membershipForForm,
  normalizeTitle,
  resetForCreateMore,
  resolveProjectId,
  selectCreatableProjects,
  targetProjectId,
  titleError,
  type CreateFormState,
} from "../create/createForm.js";
import { DuplicatePanel, ISSUE_DEDUPE_FLAG, useDuplicateMatches } from "../create/dedupe.js";
import { ParentPicker } from "../create/ParentPicker.js";
import { DatePicker } from "../dropdowns/DatePicker.js";
import { useWeekStart } from "../dropdowns/weekStart.js";
import { LabelPicker } from "../labels/LabelPicker.js";
import { clearWorkItemDraft, loadWorkItemDraft, storeWorkItemDraft } from "../lib/createDraft.js";

/** Project rows the dialog may file under. The role/slug fields stay
 * optional so the sidebar's existing title-plus-project callers keep
 * working; an absent role is treated as no create permission. */
export interface WorkItemCreateProject {
  id: string;
  name: string;
  identifier?: string;
  memberRole?: number | null;
  archived?: boolean;
}

export interface WorkItemCreateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  /** Projects offered to the selector (already joined by the caller). */
  projects: WorkItemCreateProject[];
  /** Pre-selected target, e.g. the project whose page opened the dialog. */
  initialProjectId?: string;
  /** Workspace create standing; guests never open the form. */
  canCreate?: boolean;
  /** Fired after a create lands, with the server's row id. */
  onCreated?: (issueId: string) => void;
}

/** Accessible name of the description editor and its heading. */
const DESCRIPTION_LABEL = "Description";

function toOption(project: WorkItemCreateProject) {
  return {
    id: project.id,
    name: project.name,
    identifier: project.identifier ?? project.name.slice(0, 3).toUpperCase(),
    memberRole: project.memberRole ?? null,
    archived: project.archived ?? false,
  };
}

/** A blank form for one project, keeping the workspace's kept title. */
function freshForm(workspaceSlug: string, projectId: string | null, name = ""): CreateFormState {
  const state = emptyCreateForm(projectId);
  state.name = name === "" ? (loadWorkItemDraft(workspaceSlug)?.title ?? "") : name;
  return state;
}

/** One property slot: muted caption above the picker's trigger. */
function FieldSlot({ label, children }: { label: string; children: React.ReactNode }): React.ReactElement {
  return (
    <div className="flex min-w-0 flex-col gap-(--space-1)">
      <span className="text-meta text-(--text-muted)">{label}</span>
      <div className="flex min-w-0 flex-wrap items-center gap-(--space-2)">{children}</div>
    </div>
  );
}

export function WorkItemCreateDialog({
  open,
  onOpenChange,
  workspaceSlug,
  projects,
  initialProjectId,
  canCreate = true,
  onCreated,
}: WorkItemCreateDialogProps): React.ReactElement | null {
  const create = useCreateWorkItem(workspaceSlug);
  const attachMembership = useApplyCreateMembership(workspaceSlug);
  const generateTitle = useGenerateTitle(workspaceSlug);
  const saveDraft = useSaveDraft(workspaceSlug);
  const assistant = useAssistantConfig(workspaceSlug, open);
  const weekStart = useWeekStart();

  const creatable = React.useMemo(() => selectCreatableProjects(projects.map(toOption)), [projects]);
  const [form, setForm] = React.useState<CreateFormState>(() =>
    freshForm(workspaceSlug, resolveProjectId(creatable, initialProjectId ?? null))
  );
  const [fieldError, setFieldError] = React.useState<string | null>(null);
  const [createMore, setCreateMore] = React.useState(false);
  const [parentHit, setParentHit] = React.useState<IssueSearchHit | null>(null);
  const [discardOpen, setDiscardOpen] = React.useState(false);
  const [draftError, setDraftError] = React.useState<string | null>(null);
  const [generating, setGenerating] = React.useState(false);
  // Bumped whenever the description must start over: a different
  // project, Create more. The editor reads its content once at mount,
  // so the key is what actually clears it.
  const [editorGeneration, setEditorGeneration] = React.useState(0);
  // Bumped on the open transition and after Create more; the title
  // takes focus exactly then, never on an ordinary keystroke.
  const [focusRequest, setFocusRequest] = React.useState(0);
  // Mirrors the open prop locally. When the parent ignores onOpenChange
  // the dialog still unmounts its body, so reopening works from here too.
  const titleRef = React.useRef<HTMLInputElement>(null);
  // The busy flag reads from query state, which lags a click by a render:
  // this ref closes the same-tick window so a double submit posts once.
  const submitting = React.useRef(false);

  const projectsQuery = usePickerProjects(workspaceSlug, open);
  const projectConfig = usePickerProject(workspaceSlug, form.projectId ?? "", form.projectId !== null);
  const detail = projectConfig.data;
  const estimates = usePickerEstimates(workspaceSlug, form.projectId ?? "", form.projectId !== null && open);
  const duplicates = useDuplicateMatches({
    workspaceSlug,
    projectId: form.projectId ?? "",
    title: form.name,
    enabled: open && edition.flags[ISSUE_DEDUPE_FLAG] === true,
  });

  // A fresh visit starts from the caller's project and the kept title;
  // reopening never inherits the previous submission's fields.
  // Track the last open session so the reset runs once per visit: a new
  // form object on every render would remount the lazy editor and drop
  // whatever was already typed into it.
  const visit = React.useRef<{ open: boolean; key: string }>({ open: false, key: "" });
  const visitKey = `${workspaceSlug}:${initialProjectId ?? ""}`;
  React.useEffect(() => {
    if (!open) return;
    const freshVisit = !visit.current.open || visit.current.key !== visitKey;
    visit.current = { open: true, key: visitKey };
    if (!freshVisit) return;
    setForm(freshForm(workspaceSlug, resolveProjectId(creatable, initialProjectId ?? null)));
    setFieldError(null);
    setParentHit(null);
    setDiscardOpen(false);
    setDraftError(null);
    setEditorGeneration((current) => current + 1);
    setFocusRequest((current) => current + 1);
  }, [open, visitKey, workspaceSlug, initialProjectId, creatable]);

  // The title owns initial focus. Base UI moves focus into the dialog on
  // a later task (its close button claims it first), so claim the field
  // twice: immediately for tests that query focus synchronously, and
  // again after the dialog's own move has landed.
  React.useEffect(() => {
    if (!open) return;
    titleRef.current?.focus();
    const frame = globalThis.setTimeout(() => titleRef.current?.focus(), 0);
    return () => globalThis.clearTimeout(frame);
  }, [open, focusRequest]);

  const configured = isAssistantConfigured(assistant.data);
  const canGenerate = canGenerateTitle({
    name: form.name,
    descriptionHtml: form.descriptionHtml,
    assistantConfigured: configured,
  });
  // Workspace standing gates the form, and the create-permission project
  // list gates it again, so a member with no eligible project sees the
  // notice instead of a form that cannot submit.
  // The project list is the create-permission gate (ISS-123): a member
  // with no eligible project, or a guest, sees the notice instead of a
  // form whose submit could only fail. `canCreate` carries the shell's
  // own standing check; the roles are identical on both sides.
  const allowed = canCreate && creatable.length > 0;
  const projectId = targetProjectId(form.projectId, creatable);
  // The picker reads the joined-project list itself; hide every project
  // this form may not file under so the selector offers only the rows the
  // dialog would accept (ISS-123).
  const hiddenProjectIds = React.useMemo(() => {
    const allowedIds = new Set(creatable.map((project) => project.id));
    return (projectsQuery.data ?? []).filter((project) => !allowedIds.has(project.id)).map((project) => project.id);
  }, [creatable, projectsQuery.data]);
  const busy = create.isPending || generating || saveDraft.isPending;

  function patch(next: Partial<CreateFormState>): void {
    setForm((current) => ({ ...current, ...next }));
  }

  function updateTitle(name: string): void {
    setFieldError(null);
    patch({ name });
    storeWorkItemDraft(workspaceSlug, { title: name, projectId: form.projectId });
  }

  function pickProject(projectId: string): void {
    if (projectId === form.projectId) return;
    // Dependent fields belong to the old project: reset the strip and the
    // editor, and keep the kept title/project draft in step.
    setForm(freshForm(workspaceSlug, projectId, form.name));
    setFieldError(null);
    setParentHit(null);
    setEditorGeneration((current) => current + 1);
    storeWorkItemDraft(workspaceSlug, { title: form.name, projectId });
  }

  function finishClose(): void {
    setDiscardOpen(false);
    onOpenChange(false);
  }

  function requestClose(): void {
    if (!hasDraftWorthKeeping(form)) {
      finishClose();
      return;
    }
    setDraftError(null);
    setDiscardOpen(true);
  }

  function handleOpenChange(): void {
    requestClose();
  }

  async function submit(event: React.FormEvent): Promise<void> {
    event.preventDefault();
    if (submitting.current) return;
    submitting.current = true;
    // The ref stays set through the write so a second click cannot post
    // until the first create settles; a refusal resets it right away.
    const started = await submitOnce();
    if (!started) submitting.current = false;
  }

  async function submitOnce(): Promise<boolean> {
    if (busy) return false;
    let title = normalizeTitle(form.name);
    if (title === "") {
      if (!canGenerate) {
        // A configured assistant still needs words to work from: say so
        // instead of the plain required-title message (ISS-118).
        const awaitingDescription = configured && descriptionText(form.descriptionHtml) === "";
        setFieldError(awaitingDescription ? TITLE_EMPTY_DESCRIPTION_BODY : titleError(form.name));
        return false;
      }
      setGenerating(true);
      try {
        title = await generateTitle.mutateAsync(form.descriptionHtml);
      } catch (error) {
        notifyError(
          TITLE_GENERATION_FAILED_TITLE,
          error instanceof TitleTimeoutError ? TITLE_GENERATION_TIMED_OUT_BODY : TITLE_GENERATION_FAILED_BODY
        );
        return false;
      } finally {
        setGenerating(false);
      }
      if (title === "") {
        notifyError(TITLE_GENERATION_FAILED_TITLE, TITLE_GENERATION_EMPTY_BODY);
        return false;
      }
    }
    const invalid = titleError(title);
    if (invalid !== null) {
      setFieldError(invalid);
      return false;
    }
    create.mutate(
      {
        projectId,
        payload: buildCreatePayload({ ...form, name: title }),
      },
      {
        onSettled: () => {
          submitting.current = false;
        },
        onSuccess: (created) => {
          clearWorkItemDraft(workspaceSlug);
          onCreated?.(created.id);
          const membership = membershipForForm(form);
          if (hasMembership(membership)) {
            attachMembership.mutate(
              { projectId, issueId: created.id, ...membership },
              {
                onError: () => {
                  notifyError(MEMBERSHIP_FAILED_TITLE, MEMBERSHIP_FAILED_BODY);
                },
              }
            );
          }
          if (!createMore) {
            finishClose();
            return;
          }
          // Keep the project and re-focus the title; everything else is
          // a new item (ISS-119).
          setForm(resetForCreateMore(form));
          setFieldError(null);
          setParentHit(null);
          setEditorGeneration((current) => current + 1);
          setFocusRequest((current) => current + 1);
        },
      }
    );
    return true;
  }

  async function saveAndClose(): Promise<void> {
    setDraftError(null);
    try {
      await saveDraft.mutateAsync(buildDraftInput(form));
    } catch {
      setDraftError("The draft didn't save. Check your connection and try again.");
      return;
    }
    clearWorkItemDraft(workspaceSlug);
    finishClose();
  }

  async function saveInlineDraft(): Promise<void> {
    setDraftError(null);
    try {
      await saveDraft.mutateAsync(buildDraftInput(form));
    } catch {
      notifyError(DRAFT_SAVE_FAILED_INLINE_TITLE, "Nothing was saved — check your connection and try again.");
      return;
    }
    notifySuccess(DRAFT_SAVED_INLINE_TITLE, "Find it in your drafts when you're ready to finish it.");
    clearWorkItemDraft(workspaceSlug);
    finishClose();
  }

  if (!open) return null;

  if (!allowed) {
    return (
      <Dialog
        title="Create new work item"
        description="New work items need a project you can add work to."
        open={open}
        onOpenChange={finishClose}
        actions={
          <Button variant="secondary" onClick={finishClose}>
            Close
          </Button>
        }
      >
        <p className="text-body text-(--text-muted)">Join a project as a member or admin before creating work items.</p>
      </Dialog>
    );
  }

  return (
    <>
      <Dialog
        title="Create new work item"
        description="Set the details now; you can refine them later."
        open={open}
        onOpenChange={handleOpenChange}
        ignoreEscapeKey={busy}
        actions={
          <>
            <Button variant="ghost" onClick={requestClose} disabled={busy}>
              Discard
            </Button>
            <Button variant="secondary" onClick={() => void saveInlineDraft()} disabled={busy}>
              Save to Drafts
            </Button>
            <Button type="submit" form="work-item-create-form" variant="primary" disabled={busy}>
              {busy ? "Saving…" : "Save"}
            </Button>
          </>
        }
      >
        <form
          id="work-item-create-form"
          onSubmit={(event) => {
            void submit(event);
          }}
          className="flex flex-col gap-(--space-4)"
        >
          <div className="flex flex-wrap items-center gap-(--space-2)">
            <ProjectPicker
              workspaceSlug={workspaceSlug}
              value={form.projectId}
              onChange={pickProject}
              excludeProjectIds={hiddenProjectIds}
              disabled={busy}
            />
            <DuplicatePanel matches={duplicates.matches} />
          </div>
          <Input
            id="name"
            inputRef={titleRef}
            label="Title"
            placeholder="Title"
            value={form.name}
            onChange={(event) => updateTitle(event.target.value)}
            {...(fieldError === null ? {} : { error: fieldError })}
            autoComplete="off"
          />
          <div className="flex flex-col gap-(--space-2)">
            <span className="text-body font-medium text-(--text)">{DESCRIPTION_LABEL}</span>
            <div className="rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-2)">
              <React.Suspense fallback={<div aria-hidden="true" className="min-h-24" />}>
                <RichTextEditor
                  key={`${projectId}:${editorGeneration}`}
                  initialHtml={form.descriptionHtml}
                  onChange={(html) => patch({ descriptionHtml: html })}
                  placeholder="Add a description…"
                  label={DESCRIPTION_LABEL}
                />
              </React.Suspense>
            </div>
          </div>
          <div className="flex flex-wrap items-start gap-(--space-4)">
            <FieldSlot label="State">
              <StatePicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                value={form.stateId}
                onChange={(stateId) => patch({ stateId })}
                disabled={busy}
              />
            </FieldSlot>
            <FieldSlot label="Runs on">
              <PodPicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                value={form.execution}
                onChange={(execution: ExecutionSelection) => patch({ execution })}
                disabled={busy}
              />
            </FieldSlot>
            <FieldSlot label="Priority">
              <PriorityPicker
                value={form.priority as PriorityLevel}
                onChange={(priority) => patch({ priority })}
                disabled={busy}
              />
            </FieldSlot>
            <FieldSlot label="Assignees">
              <MemberPicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                multiple
                value={form.assigneeIds}
                onChange={(assigneeIds) => patch({ assigneeIds })}
                disabled={busy}
              />
            </FieldSlot>
            <FieldSlot label="Labels">
              <LabelPicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                value={form.labelIds}
                onChange={(labelIds) => patch({ labelIds })}
                disabled={busy}
              />
            </FieldSlot>
            <FieldSlot label="Start date">
              <DatePicker
                label="Start date"
                value={form.startDate}
                onChange={(startDate) => patch({ startDate })}
                max={form.targetDate}
                weekStart={weekStart}
                disabled={busy}
              />
            </FieldSlot>
            <FieldSlot label="Due date">
              <DatePicker
                label="Due date"
                value={form.targetDate}
                onChange={(targetDate) => patch({ targetDate })}
                min={form.startDate}
                weekStart={weekStart}
                disabled={busy}
              />
            </FieldSlot>
            {detail?.cycle_view === true ? (
              <FieldSlot label="Cycle">
                <CyclePicker
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  value={form.cycleId}
                  onChange={(cycleId) => patch({ cycleId })}
                  disabled={busy}
                />
              </FieldSlot>
            ) : null}
            {detail?.module_view === true ? (
              <FieldSlot label="Modules">
                <ModulePicker
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  multiple
                  value={form.moduleIds}
                  onChange={(moduleIds) => patch({ moduleIds })}
                  disabled={busy}
                />
              </FieldSlot>
            ) : null}
            {estimates.data !== undefined && estimates.data.length > 0 ? (
              <FieldSlot label="Estimate">
                <EstimatePicker
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  value={form.estimatePoint}
                  onChange={(estimatePoint) => patch({ estimatePoint })}
                  disabled={busy}
                />
              </FieldSlot>
            ) : null}
            <FieldSlot label="Parent">
              <ParentPicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                value={parentHit}
                onChange={(hit) => {
                  setParentHit(hit);
                  patch({ parentId: hit?.id ?? null });
                }}
                disabled={busy}
              />
            </FieldSlot>
          </div>
          <div className="flex items-center gap-(--space-2)">
            <Switch label="Create more" checked={createMore} onCheckedChange={setCreateMore} />
            <span aria-hidden="true" className="text-body text-(--text)">
              Create more
            </span>
          </div>
        </form>
      </Dialog>
      <DiscardDraftDialog
        open={discardOpen}
        saving={saveDraft.isPending}
        error={draftError}
        onCancel={() => setDiscardOpen(false)}
        onDiscard={finishClose}
        onSaveDraft={saveAndClose}
      />
    </>
  );
}
