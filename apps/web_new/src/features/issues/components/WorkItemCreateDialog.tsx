// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item create and edit dialog (parity/specs/issues.md §11,
// ISS-117–130 and the §21 shell seam SHELL-086/047). One form serves the
// sidebar button, the issues header, the palette command, the layout
// quick-adds and the edit entry: a project selector, the title (which
// takes focus), a rich description and the property strip, with
// Create-more, a discard guard, cycle/module context, the advanced git
// branch, and — for a draft — a publish action. The rules live in
// create/createForm.ts and create/editForm.ts, the network calls in
// create/createApi.ts, the description panel in ModalDescription.tsx,
// and the parent search and draft prompt in their own modules.

import { Button, Dialog, Input, Switch } from "@pidash/kit";
import * as React from "react";

import {
  DRAFT_DESCRIPTION_ENTITY_TYPE,
  ISSUE_DESCRIPTION_ENTITY_TYPE,
  type DraftIssueCreateInput,
  type IssueSearchHit,
} from "@pidash/api-client";

import { edition } from "../../../core/edition/index.js";
import {
  CyclePicker,
  toProjectOption,
  EstimatePicker,
  MemberPicker,
  ModulePicker,
  PodPicker,
  PriorityPicker,
  ProjectPicker,
  StatePicker,
  usePickerProject,
  type ExecutionSelection,
  type PriorityLevel,
} from "../../../shared/pickers/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { isAssistantConfigured, useAssistantConfig } from "../../home/index.js";
import { useAppInstanceBootstrap } from "../../instance/index.js";
import { useCreateWorkItem } from "../api/create.js";
import { executionTargetFieldError } from "../api/executionTarget.js";
import { createStateRefusal, creationStateGate, stateOptionGate } from "../edition/workflow.js";
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
  useApplyCreateMembership,
  useAttachDescriptionAssets,
  useEditIssueDetail,
  useGenerateTitle,
  usePublishDraft,
  useSaveDraft,
  useWriteIssueEdit,
} from "../create/createApi.js";
import {
  buildCreatePayload,
  buildDraftInput,
  canGenerateTitle,
  descriptionText,
  emptyCreateForm,
  GIT_BRANCH_MAX_LENGTH,
  gitBranchError,
  hasDraftWorthKeeping,
  hasMembership,
  isCreatableProjectId,
  membershipForForm,
  normalizeTitle,
  resetForCreateMore,
  resolveProjectId,
  selectCreatableProjects,
  targetProjectId,
  toCreateProjectOption,
  titleError,
  visibleDescriptionHtml,
  type CreateFormInitialValues,
  type CreateFormState,
  type CreateMembership,
} from "../create/createForm.js";
import {
  baselineFor,
  changedDraftProject,
  buildEditPayload,
  buildPublishInput,
  editMembership,
  formFromSnapshot,
  withStoredDescription,
  withStoredBranch,
  withStoredBranchBaseline,
  type EditBaseline,
} from "../create/editForm.js";
import { ModalDescription } from "../create/ModalDescription.js";
import {
  getIssueTypeExtension,
  type IssueTypeExtension,
  type IssueTypeExtensionProps,
  type TemplatePrefill,
} from "../create/issueTypeSeam.js";
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

/** What the edit entry hands the dialog (ISS-126): the row to change. */
export interface WorkItemEditTarget {
  kind: "issue" | "draft";
  id: string;
  /** The row's project; drafts without one open on the first choice. */
  projectId: string | null;
  /** Normalized row fields as the caller already holds them. */
  snapshot: unknown;
  /** Closing or saving hands the parent the row it must re-read. */
  onChanged?: (() => void) | undefined;
}

/** Cycle or module page the modal opened from (ISS-124). */
export interface WorkItemCreateContext {
  cycleId?: string | null;
  moduleId?: string | null;
}

export interface WorkItemCreateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  /** Projects offered to the selector (already joined by the caller). */
  projects: WorkItemCreateProject[];
  /** Pre-selected target, e.g. the project whose page opened the dialog. */
  initialProjectId?: string;
  /** Prefix for the success toast's "View work item" action. */
  projectIdentifier?: string | undefined;
  /** Workspace create standing; guests never open the form. */
  canCreate?: boolean;
  /** Fired after a create lands, with the server's row id. */
  onCreated?: (issueId: string) => void;
  /** Listing prefill: the section's grouping value and default state (ISS-011/ISS-012). */
  initialValues?: CreateFormInitialValues | undefined;
  /** Edit the named row instead of creating one (ISS-126). */
  edit?: WorkItemEditTarget | null;
  /** Route context preseeding a new item's cycle or module (ISS-124). */
  context?: WorkItemCreateContext;
  /** Publish entry: the draft id, when the form shows a stored draft (ISS-130). */
  draftId?: string | null;
  /**
   * Fixed project (ISS-175): the selector shows it but cannot move the
   * form elsewhere. The caller also passes the row in `projects`.
   */
  lockedProjectId?: string | undefined;
  /**
   * Keep the preset parent fixed (ISS-175): the create modal opened from
   * the sub-issues widget shows the parent as context but does not let a
   * child be re-parented through its picker.
   */
  lockedParent?: boolean | undefined;
  /**
   * Parent preset (ISS-175): a new sub-work item starts attached to this
   * row. The parent picker shows it and removing it clears the choice.
   */
  initialParent?: CreateParentPreset | undefined;
  /**
   * Draft entry (ISS-139/141): a new draft starts from this payload — the
   * duplicated row a copy carries — and the footer stores it instead of
   * creating an issue. An absent seed is a blank draft.
   */
  draftSeed?: DraftSeed | null | undefined;
  /**
   * Called with the payload to store when a draft entry saves. The
   * caller owns the write and its toast, so a copy on the drafts page
   * re-reads that page rather than switching the form to create mode.
   * It resolves with the stored row's address so the dialog can link
   * the description images uploaded during the visit.
   */
  onCopySaved?: ((input: DraftIssueCreateInput) => Promise<SavedDraftRef>) | undefined;
  /**
   * Move-to-project entry (ISS-139): the edited draft is promoted through
   * the caller's publish write, and the footer names that action.
   */
  moveToProject?: boolean | undefined;
  /** Promote write the move entry runs with the named draft payload. */
  onPublish?: ((input: DraftIssueCreateInput) => Promise<void>) | undefined;
  /** A guest may edit their own draft but may not convert it (DRAFT-025). */
  canPublish?: boolean | undefined;
  /**
   * Edit-only heading override (INT-020): the intake accept entry names
   * the request being added instead of the generic update heading.
   */
  editTitle?: string | undefined;
  /** Edit-only save label override (INT-020): the accept entry's add action. */
  editSaveLabel?: string | undefined;
  /**
   * Edit-only gate (INT-020): runs after validation, before the
   * work-item write. A rejection aborts the submit and the dialog stays
   * open; the callback reports its own failure to the viewer.
   */
  onBeforeEditSubmit?: (() => Promise<void>) | undefined;
  /**
   * Edit save hand-back (INT-020): the changed work-item field keys,
   * fired when an edit save lands. Closing or cancelling never fires
   * it, so the caller can tell a save from a close.
   */
  onEditSaved?: ((changedFields: readonly string[]) => void) | undefined;
  /**
   * Copy entry (ISS-060): the row a duplicate starts from. The form opens
   * on that row's fields with the suffixed title and files a new work
   * item; nothing is written until Save.
   */
  copySeed?: CopySeed | null | undefined;
}

/** The row a "Make a copy" entry opens prefilled (ISS-060). */
export interface CopySeed {
  title: string;
  snapshot: unknown;
}

/** The draft payload a copy or a blank draft entry opens with. */
export interface DraftSeed {
  title: string;
  input: DraftIssueCreateInput;
}

/** The stored row a page-owned draft write hands back: its address. */
export interface SavedDraftRef {
  id: string;
  project_id: string | null;
}

/** A parent preset the dialog seeds as if the picker had chosen it. */
export interface CreateParentPreset {
  id: string;
  name: string;
  identifier: string;
  sequenceId: number;
}

/**
 * Keyboard positions inside the dialog (ISS-125). The order follows the
 * old form: the title leads, then the description and the property
 * strip, with the footer last. Pickers take their index from this table
 * so a conditional slot never renumbers the ones after it.
 */
export const MODAL_TAB_ORDER = {
  title: 1,
  description: 2,
  state: 4,
  priority: 5,
  assignees: 6,
  labels: 7,
  startDate: 8,
  dueDate: 9,
  cycle: 10,
  modules: 11,
  estimate: 12,
  parent: 13,
  createMore: 14,
  discard: 15,
  save: 16,
} as const;

/** Heading of the create form and of the edit form. */
export const CREATE_DIALOG_TITLE = "Create new work item";
export const EDIT_DIALOG_TITLE = "Update work item";
export const PUBLISH_BUTTON_LABEL = "Publish issue";
export const CREATE_DRAFT_DIALOG_TITLE = "Create a draft";
export const SAVE_DRAFT_BUTTON_LABEL = "Save to Drafts";
export const MOVE_TO_PROJECT_BUTTON_LABEL = "Add to project";
export const PUBLISH_SUCCESS_TITLE = "Draft published to project.";
export const PUBLISH_SUCCESS_BODY = "The work item now lives in its project.";
export const PUBLISH_FAILED_TITLE = "Couldn't publish the draft";
export const PUBLISH_FAILED_BODY = "Pick a project this draft can move into, then try again.";
export const PUBLISH_NEEDS_PROJECT_TITLE = "Couldn't publish the draft";
export const PUBLISH_NEEDS_PROJECT_BODY = "Select a project before publishing this draft.";
export const UPDATE_SUCCESS_TITLE = "Work item updated successfully";
export const UPDATE_FAILED_TITLE = "Couldn't update the work item";
export const UPDATE_FAILED_BODY = "Check your connection and try again.";
export const EDITOR_BUSY_TITLE = "Couldn't close the form";
export const EDITOR_BUSY_BODY = "The description is still processing a change. Wait a moment, then close.";
export const ADVANCED_GIT_LABEL = "Advanced — git";
export const GIT_BRANCH_LABEL = "Work branch";
export const GIT_BRANCH_PLACEHOLDER = "Leave empty to create a new branch";

/** A blank form for one project, keeping the workspace's kept title. */
function freshForm(workspaceSlug: string, projectId: string | null, name = ""): CreateFormState {
  const state = emptyCreateForm(projectId);
  state.name = name === "" ? (loadWorkItemDraft(workspaceSlug)?.title ?? "") : name;
  return state;
}

/**
 * A blank form seeded from a cycle or module page (ISS-124), plus the
 * listing prefill a section hands the modal (ISS-011/ISS-012): its
 * grouping value and the project's default state. Route context wins
 * over a prefill cycle/module, since the page decides where the item
 * lands.
 */
function contextForm(
  workspaceSlug: string,
  projectId: string | null,
  context: WorkItemCreateContext | undefined,
  initial: CreateFormInitialValues | undefined
): CreateFormState {
  const state = freshForm(workspaceSlug, projectId);
  if (initial?.stateId != null) state.stateId = initial.stateId;
  if (initial?.priority !== undefined && initial.priority !== "none") state.priority = initial.priority;
  if (initial?.assigneeIds !== undefined) state.assigneeIds = [...initial.assigneeIds];
  if (initial?.labelIds !== undefined) state.labelIds = [...initial.labelIds];
  if (initial?.cycleId !== undefined) state.cycleId = initial.cycleId;
  if (initial?.moduleIds !== undefined) state.moduleIds = [...initial.moduleIds];
  if (initial?.targetDate != null) state.targetDate = initial.targetDate;
  if (context?.cycleId != null) state.cycleId = context.cycleId;
  if (context?.moduleId != null) state.moduleIds = [context.moduleId];
  return state;
}

/** One property slot: muted caption above the picker's trigger. */
function FieldSlot({
  label,
  hideLabel = false,
  children,
}: {
  label: string;
  /** True when the control itself names the slot (dates, cycle, modules). */
  hideLabel?: boolean;
  children: React.ReactNode;
}): React.ReactElement {
  return (
    <div className="flex min-w-0 flex-col gap-(--space-1)">
      {hideLabel ? null : <span className="text-meta text-(--text-muted)">{label}</span>}
      <div className="flex min-w-0 flex-wrap items-center gap-(--space-2)">{children}</div>
    </div>
  );
}

/**
 * A trigger that only participates in the dialog's tab order as a
 * position marker: the oracle reads the `[tabindex]` elements of the
 * modal, while the picker inside keeps its own accessible control.
 */
function TabStop({
  index,
  label,
  children,
}: {
  index: number;
  label: string;
  children: React.ReactNode;
}): React.ReactElement {
  return (
    <div role="group" aria-label={label} tabIndex={index} className="flex items-center">
      {children}
    </div>
  );
}

/** Draft id the edit write targets, when the form shows a stored draft. */
function editDraftId(edit: WorkItemEditTarget | null, draftId: string | null | undefined): string | null {
  if (edit?.kind === "draft") return edit.id;
  return draftId ?? null;
}

/**
 * The stored row an edit opens with, normalized, or null. A draft with
 * no project keeps that state: the publish entry needs a project before
 * the draft can move, so the dialog must not silently adopt the first
 * creatable one on its behalf (ISS-130).
 */
function editStart(edit: WorkItemEditTarget, projectId: string | null): CreateFormState | null {
  if (projectId === null) {
    return edit.kind === "draft" ? formFromSnapshot(edit.snapshot, null) : null;
  }
  return formFromSnapshot(edit.snapshot, projectId);
}

export function WorkItemCreateDialog({
  open,
  onOpenChange,
  workspaceSlug,
  projects,
  initialProjectId,
  projectIdentifier,
  canCreate = true,
  onCreated,
  initialValues,
  edit = null,
  context,
  draftId = null,
  lockedProjectId,
  lockedParent = false,
  initialParent,
  draftSeed = null,
  onCopySaved,
  moveToProject = false,
  onPublish,
  canPublish = true,
  editTitle,
  editSaveLabel,
  onBeforeEditSubmit,
  onEditSaved,
  copySeed = null,
}: WorkItemCreateDialogProps): React.ReactElement | null {
  const editing = edit !== null;
  // A draft entry (ISS-139/141): the create form writes a draft instead of
  // an issue. The stored-draft edit keeps its own publish entry.
  const drafting = edit === null && (draftSeed !== null || onCopySaved !== undefined);
  const moving = editing && edit?.kind === "draft" && moveToProject;
  const create = useCreateWorkItem(workspaceSlug);
  const attachMembership = useApplyCreateMembership(workspaceSlug);
  const generateTitle = useGenerateTitle(workspaceSlug);
  const saveDraft = useSaveDraft(workspaceSlug);
  const writeEdit = useWriteIssueEdit(workspaceSlug);
  const publish = usePublishDraft(workspaceSlug);
  const attachAssets = useAttachDescriptionAssets(workspaceSlug);
  const assistant = useAssistantConfig(workspaceSlug, open);
  const instance = useAppInstanceBootstrap();
  // An issue edit opens from a list snapshot, which carries no body: the
  // stored description comes from the detail read (ISS-126). A draft's
  // snapshot already holds it.
  const detailEdit = useEditIssueDetail(
    workspaceSlug,
    edit?.kind === "issue" ? edit.projectId : null,
    edit?.kind === "issue" ? edit.id : null,
    open
  );
  const weekStart = useWeekStart();

  const creatable = React.useMemo(() => selectCreatableProjects(projects.map(toCreateProjectOption)), [projects]);
  // A fixed project is seeded as the only offered row, so neither the
  // visit reset nor a later open can move the form elsewhere (ISS-175).
  const lockedProject = React.useMemo(
    () => (lockedProjectId === undefined ? undefined : creatable.find((project) => project.id === lockedProjectId)),
    [creatable, lockedProjectId]
  );
  const offered = React.useMemo(
    () => (lockedProject === undefined ? creatable : [lockedProject]),
    [creatable, lockedProject]
  );
  const seededParent = React.useMemo((): IssueSearchHit | null => {
    if (initialParent === undefined) return null;
    return {
      id: initialParent.id,
      name: initialParent.name,
      sequence_id: initialParent.sequenceId,
      project__name: "",
      project__identifier: initialParent.identifier,
      project_id: lockedProjectId ?? initialProjectId ?? "",
      workspace__slug: workspaceSlug,
      state__name: null,
      state__group: null,
      state__color: null,
      start_date: null,
    };
  }, [initialParent, initialProjectId, lockedProjectId, workspaceSlug]);
  const seededForm = React.useCallback(
    (projectId: string | null, name = ""): CreateFormState => {
      const next = freshForm(workspaceSlug, projectId, name);
      if (seededParent !== null) next.parentId = seededParent.id;
      return next;
    },
    [seededParent, workspaceSlug]
  );
  const editProjectId = edit?.projectId ?? null;
  const seededDraftForm = React.useMemo((): CreateFormState | null => {
    if (draftSeed === null) return null;
    return formFromSnapshot(
      {
        id: "draft-copy",
        // A copy opens under its duplicated title (ISS-139), not the
        // original row name its payload carried before the suffix.
        name: draftSeed.title,
        description_html: draftSeed.input.description_html ?? "",
        state_id: draftSeed.input.state_id ?? null,
        priority: draftSeed.input.priority ?? "none",
        assignee_ids: draftSeed.input.assignee_ids ?? [],
        label_ids: draftSeed.input.label_ids ?? [],
        start_date: draftSeed.input.start_date ?? null,
        target_date: draftSeed.input.target_date ?? null,
        cycle_id: draftSeed.input.cycle_id ?? null,
        module_ids: draftSeed.input.module_ids ?? [],
        estimate_point: draftSeed.input.estimate_point ?? null,
        parent_id: draftSeed.input.parent_id ?? null,
      },
      draftSeed.input.project_id ?? initialProjectId ?? null
    );
  }, [draftSeed, initialProjectId]);
  // A duplicate opens on the source row's fields under its "(copy)" title;
  // its fields seed the form but the submit always creates.
  const seededCopyForm = React.useMemo((): CreateFormState | null => {
    if (copySeed === null) return null;
    const row = typeof copySeed.snapshot === "object" && copySeed.snapshot !== null ? copySeed.snapshot : {};
    // A snapshot the form cannot read still opens under the copy title,
    // so the duplicate is recognisable even from a sparse row.
    return (
      formFromSnapshot({ ...row, name: copySeed.title }, initialProjectId ?? null) ??
      seededForm(initialProjectId ?? null, copySeed.title)
    );
  }, [copySeed, initialProjectId, seededForm]);
  const [form, setForm] = React.useState<CreateFormState>(() =>
    edit !== null
      ? (editStart(edit, resolveProjectId(offered, initialProjectId ?? editProjectId ?? null)) ?? seededForm(null))
      : (seededDraftForm ?? seededCopyForm ?? seededForm(resolveProjectId(offered, initialProjectId ?? null)))
  );
  // What the edit opened with: every submit diffs against it. A create
  // never has one, so its submit always builds a full payload.
  const [baseline, setBaseline] = React.useState<EditBaseline | null>(null);
  const [fieldError, setFieldError] = React.useState<string | null>(null);
  const [executionError, setExecutionError] = React.useState<string | null>(null);
  // A create whose chosen state the workflow refuses: the picker greys the
  // row, and this is where a stale pick that slipped through is named
  // before anything is posted (ISS-233).
  const [stateError, setStateError] = React.useState<string | null>(null);
  const [createMore, setCreateMore] = React.useState(false);
  const [parentHit, setParentHit] = React.useState<IssueSearchHit | null>(seededParent);
  const [discardOpen, setDiscardOpen] = React.useState(false);
  const [draftError, setDraftError] = React.useState<string | null>(null);
  const [generating, setGenerating] = React.useState(false);
  // The advanced git disclosure (ISS-128): collapsed unless the row
  // already carries a branch.
  const [gitOpen, setGitOpen] = React.useState(false);
  const [gitError, setGitError] = React.useState<string | null>(null);
  // The description panel keeps its own upload state out of `busy`, so it
  // reports whether a close may proceed (ISS-125).
  const [descriptionReady, setDescriptionReady] = React.useState(true);
  // The memberships the route seeded for this visit: the discard guard
  // treats them as context, not as something the user entered (ISS-124).
  const routeMembership = React.useRef<CreateMembership>({ cycleId: null, moduleIds: [] });
  // The description the edit opened with, so a late detail read can tell
  // an untouched editor from one the user already typed into.
  const openedDescription = React.useRef<string | null>(null);
  // The branch the edit opened with, for the same late-detail comparison.
  const openedBranch = React.useRef("");
  const adoptedBranch = React.useRef<string | null>(null);
  // The stored description already folded into the form; the detail read
  // may answer more than once (a refetch), and only the first fold needs
  // to rebuild the editor.
  const adoptedDescription = React.useRef<string | null>(null);
  // Description images uploaded here; the submit links them to the row.
  const [assetIds, setAssetIds] = React.useState<string[]>([]);
  // Bumped whenever the description must start over: a different
  // project, Create more. The editor reads its content once at mount,
  // so the key is what actually clears it.
  const [editorGeneration, setEditorGeneration] = React.useState(0);
  // Bumped on the open transition and after Create more; the title
  // takes focus exactly then, never on an ordinary keystroke.
  const [focusRequest, setFocusRequest] = React.useState(0);
  const titleRef = React.useRef<HTMLInputElement>(null);
  // The busy flag reads from query state, which lags a click by a render:
  // this ref closes the same-tick window so a double submit posts once.
  const submitting = React.useRef(false);
  const draftTargetId = editDraftId(edit, draftId);
  const issueType = getIssueTypeExtension();
  const [typeId, setTypeId] = React.useState<string | null>(null);
  const [propertyValues, setPropertyValues] = React.useState<Record<string, unknown>>({});

  const projectConfig = usePickerProject(workspaceSlug, form.projectId ?? "", form.projectId !== null);
  const detail = projectConfig.data;
  const duplicates = useDuplicateMatches({
    workspaceSlug,
    projectId: form.projectId ?? "",
    title: form.name,
    enabled: open && !editing && edition.flags[ISSUE_DEDUPE_FLAG] === true,
  });

  // A fresh visit starts from the caller's project and the kept title;
  // reopening never inherits the previous submission's fields. An edit
  // visit starts from the row snapshot and its baseline. Track the last
  // open session so the reset runs once per visit: a new form object on
  // every render would remount the lazy editor and drop whatever was
  // already typed into it.
  const visit = React.useRef<{ open: boolean; key: string }>({ open: false, key: "" });
  const visitKey = [
    workspaceSlug,
    initialProjectId ?? "",
    edit?.kind ?? "",
    edit?.id ?? "",
    edit?.projectId ?? "",
    draftSeed?.title ?? "",
    draftSeed?.input.project_id ?? "",
    copySeed?.title ?? "",
    context?.cycleId ?? "",
    context?.moduleId ?? "",
  ].join(":");
  React.useEffect(() => {
    if (!open) return;
    const freshVisit = !visit.current.open || visit.current.key !== visitKey;
    visit.current = { open: true, key: visitKey };
    if (!freshVisit) return;
    // A draft may arrive without a project; only a create falls back to
    // the first creatable row (a draft must pick one before publishing).
    // A fixed project also wins over the visit's initial one, so the
    // seeded parent and the form can never land in different projects.
    const projectId =
      edit?.kind === "draft" && edit.projectId === null
        ? null
        : resolveProjectId(offered, lockedProjectId ?? initialProjectId ?? editProjectId ?? null);
    const starting = edit === null ? null : editStart(edit, projectId);
    // A copy opens on the duplicated payload (ISS-139); every other entry
    // keeps the listing prefill the base introduced.
    let next =
      starting ??
      seededDraftForm ??
      seededCopyForm ??
      contextForm(workspaceSlug, projectId, editing ? undefined : context, editing ? undefined : initialValues);
    if (seededParent !== null) next = { ...next, parentId: seededParent.id };
    routeMembership.current = membershipForForm(next);
    openedDescription.current = starting?.descriptionHtml ?? null;
    openedBranch.current = starting?.gitWorkBranch ?? "";
    adoptedBranch.current = starting?.gitWorkBranch ?? "";
    setForm(next);
    // A draft without a project still has a baseline: Save must be able
    // to store a project picked later (or the other changed fields), and
    // reporting success for a write that stores nothing is worse than
    // sending the project itself (ISS-126/130).
    setBaseline(starting === null ? null : baselineFor(starting));
    setGitOpen(next.gitWorkBranch.trim() !== "");
    setGitError(null);
    setFieldError(null);
    setExecutionError(null);
    setParentHit(seededParent);
    setDiscardOpen(false);
    setDraftError(null);
    setAssetIds([]);
    setTypeId(null);
    setPropertyValues({});
    setEditorGeneration((current) => current + 1);
    setFocusRequest((current) => current + 1);
  }, [
    open,
    visitKey,
    workspaceSlug,
    initialProjectId,
    lockedProjectId,
    offered,
    seededParent,
    edit,
    editing,
    editProjectId,
    context,
    initialValues,
    seededCopyForm,
  ]);

  // The detail read answers after the first paint: adopt its description
  // unless the user already started typing, and make it the baseline the
  // submit diffs against either way.
  const storedDescription = edit?.kind === "issue" ? detailEdit.data?.description_html : undefined;
  React.useEffect(() => {
    if (storedDescription === undefined || !open || edit?.kind !== "issue") return;
    if (adoptedDescription.current === storedDescription) return;
    const untouched = openedDescription.current === null || form.descriptionHtml === openedDescription.current;
    openedDescription.current = storedDescription;
    adoptedDescription.current = storedDescription;
    if (untouched) {
      setForm((current) => withStoredDescription(current, storedDescription));
      setEditorGeneration((current) => current + 1);
    }
    const normalized = visibleDescriptionHtml(storedDescription);
    setBaseline((current) =>
      current === null ? current : { ...current, form: { ...current.form, descriptionHtml: normalized } }
    );
  }, [storedDescription, open, edit?.kind, form.descriptionHtml]);

  // The detail read also carries the pinned branch. Adopt it only while
  // the field is still what the edit opened with; a branch the user is
  // typing stays theirs.
  const storedBranch = edit?.kind === "issue" ? detailEdit.data?.git_work_branch : undefined;
  React.useEffect(() => {
    if (storedBranch === undefined || !open || edit?.kind !== "issue") return;
    if (adoptedBranch.current === storedBranch) return;
    const untouched = form.gitWorkBranch === openedBranch.current;
    openedBranch.current = storedBranch;
    adoptedBranch.current = storedBranch;
    if (untouched) {
      setForm((current) => withStoredBranch(current, storedBranch));
      setGitOpen(storedBranch.trim() !== "");
    }
    setBaseline((current) => withStoredBranchBaseline(current, storedBranch));
  }, [storedBranch, open, edit?.kind, form.gitWorkBranch]);

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
  // The description's writing helper (and its feeling-lucky draft) read
  // the instance's LLM key, not the member's provider key; title naming
  // below keeps the member key (AGT-063).
  const writingHelp = instance.data?.bootstrap?.config.has_llm_configured === true;
  const canGenerate = canGenerateTitle({
    name: form.name,
    descriptionHtml: form.descriptionHtml,
    assistantConfigured: configured,
  });
  // Workspace standing gates the form, and the create-permission project
  // list gates it again, so a member with no eligible project sees the
  // notice instead of a form that cannot submit. An edit may keep a
  // project it no longer has create permission for, so it only needs
  // an offered row.
  const allowed = canCreate && (offered.length > 0 || drafting);
  // A create (and an issue edit) always files under a project, so the
  // first creatable row backs a null value. A draft may genuinely hold
  // none: it stays empty until the user picks one to publish into.
  const projectId =
    form.projectId ?? (edit?.kind === "draft" || drafting ? "" : targetProjectId(form.projectId, offered));
  const creatableIds = React.useMemo(() => offered.map((project) => project.id), [offered]);
  // The picker's option list resolves lazily on open, but its trigger
  // must name the picked project right away: the dialog already holds
  // the create-permission rows, so hand them over as display labels
  // instead of waiting for the picker's own read (ISS-117/ISS-123).
  const projectLabels = React.useMemo(
    () =>
      offered.map((project) =>
        toProjectOption({
          id: project.id,
          name: project.name,
          identifier: project.identifier,
        })
      ),
    [offered]
  );
  // The state picker carries the workflow gate: a create asks the rules
  // about a first state, an edit about the row it opened with, and a
  // project-less draft has nothing to ask (ISS-233).
  const stateGate =
    projectId === ""
      ? undefined
      : edit !== null
        ? stateOptionGate(projectId, baseline?.form.stateId ?? null)
        : creationStateGate(projectId);
  const busy =
    create.isPending ||
    writeEdit.isPending ||
    publish.isPending ||
    generating ||
    saveDraft.isPending ||
    attachAssets.isPending;

  function patch(next: Partial<CreateFormState>): void {
    setForm((current) => ({ ...current, ...next }));
  }

  function updateTitle(name: string): void {
    setFieldError(null);
    patch({ name });
    if (!editing) storeWorkItemDraft(workspaceSlug, { title: name, projectId: form.projectId });
  }

  function updateBranch(branch: string): void {
    setGitError(gitBranchError(branch));
    patch({ gitWorkBranch: branch });
  }

  function pickProject(nextProjectId: string): void {
    if (nextProjectId === form.projectId) return;
    // The form may only file where it holds create permission; a value
    // from outside the offered rows is ignored even if a caller renders
    // the picker with its own list.
    if (!isCreatableProjectId(nextProjectId, offered)) return;
    // An edit keeps the row it opened with; only the filed project moves
    // (a draft choosing where it publishes, ISS-130).
    if (editing) {
      // The baseline keeps the project the row was stored under: a save
      // diffs against it and PATCHes the pick for a draft (ISS-130).
      setForm({ ...form, projectId: nextProjectId });
      return;
    }
    // Dependent fields belong to the old project: reset the strip and the
    // editor, and keep the kept title/project draft in step. An edit is
    // pinned to its row's project instead.
    setForm(seededForm(nextProjectId, form.name));
    setFieldError(null);
    setExecutionError(null);
    setStateError(null);
    setParentHit(seededParent);
    setEditorGeneration((current) => current + 1);
    if (!editing) storeWorkItemDraft(workspaceSlug, { title: form.name, projectId: nextProjectId });
  }

  function finishClose(): void {
    setDiscardOpen(false);
    onOpenChange(false);
    edit?.onChanged?.();
  }

  // Discard throws the form away, including the kept title that the
  // next open would otherwise restore (ISS-120).
  function discardAndClose(): void {
    clearWorkItemDraft(workspaceSlug);
    finishClose();
  }

  function requestClose(): void {
    if (!descriptionReady) {
      notifyError(EDITOR_BUSY_TITLE, EDITOR_BUSY_BODY);
      return;
    }
    // An edit never prompts: there is a stored row to fall back on.
    if (editing || !hasDraftWorthKeeping(form, routeMembership.current)) {
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
    await submitNow();
  }

  /** One submit run from either the form or the description's Enter. */
  async function submitNow(): Promise<void> {
    if (submitting.current) return;
    submitting.current = true;
    const started = await submitOnce();
    if (!started) submitting.current = false;
  }

  async function resolveTitle(): Promise<string | null> {
    let title = normalizeTitle(form.name);
    if (title === "" && !editing) {
      if (!canGenerate) {
        // A configured assistant still needs words to work from: say so
        // instead of the plain required-title message (ISS-118).
        const awaitingDescription = configured && descriptionText(form.descriptionHtml) === "";
        setFieldError(awaitingDescription ? TITLE_EMPTY_DESCRIPTION_BODY : titleError(form.name));
        return null;
      }
      setGenerating(true);
      try {
        title = await generateTitle.mutateAsync(form.descriptionHtml);
      } catch (error) {
        notifyError(
          TITLE_GENERATION_FAILED_TITLE,
          error instanceof TitleTimeoutError ? TITLE_GENERATION_TIMED_OUT_BODY : TITLE_GENERATION_FAILED_BODY
        );
        return null;
      } finally {
        setGenerating(false);
      }
      if (title === "") {
        notifyError(TITLE_GENERATION_FAILED_TITLE, TITLE_GENERATION_EMPTY_BODY);
        return null;
      }
    }
    const invalid = titleError(title);
    if (invalid !== null) {
      setFieldError(invalid);
      return null;
    }
    const branchInvalid = gitBranchError(form.gitWorkBranch.trim());
    if (branchInvalid !== null) {
      setGitError(branchInvalid);
      setGitOpen(true);
      return null;
    }
    return title;
  }

  async function submitOnce(): Promise<boolean> {
    if (busy) return false;
    if (drafting) {
      // A draft has no title rule: an unnamed one is stored as "Untitled"
      // and picked up later (ISS-141). The project stays optional so a
      // draft may start before it is aimed anywhere.
      createDraftEntry({ ...form, name: normalizeTitle(form.name) });
      return true;
    }
    const title = await resolveTitle();
    if (title === null) return false;
    const named = { ...form, name: title };
    if (edit !== null) {
      return submitEdit(named);
    }
    // Creation obeys the same transition rules as an edit (ISS-233): a
    // state the workflow refuses is reported here instead of being filed.
    // The form only reaches here under a project (a draft never does), so
    // there is a project to ask.
    const refusal = createStateRefusal({ projectId, stateId: named.stateId });
    if (refusal !== null) {
      setStateError(refusal.reason);
      return false;
    }
    create.mutate(
      {
        projectId,
        payload: buildCreatePayload(named, typeId),
        projectIdentifier: offered.find((project) => project.id === projectId)?.identifier ?? projectIdentifier,
      },
      {
        onSettled: () => {
          submitting.current = false;
        },
        onError: (error) => {
          setExecutionError(executionTargetFieldError(error));
        },
        onSuccess: (created) => {
          clearWorkItemDraft(workspaceSlug);
          onCreated?.(created.id);
          const membership = membershipForForm(named);
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
          if (assetIds.length > 0) {
            attachAssets.mutate({ projectId, issueId: created.id, assetIds });
          }
          if (!createMore) {
            finishClose();
            return;
          }
          // Keep the project and the route's cycle/module context, then
          // re-focus the title; everything else is a new item (ISS-119).
          // A seeded parent stays attached to the next item (ISS-175).
          setForm(
            seededParent === null
              ? resetForCreateMore(named)
              : { ...resetForCreateMore(named), parentId: seededParent.id }
          );
          setFieldError(null);
          setExecutionError(null);
          setParentHit(seededParent);
          setAssetIds([]);
          setEditorGeneration((current) => current + 1);
          setFocusRequest((current) => current + 1);
        },
      }
    );
    return true;
  }

  /**
   * Link this visit's description uploads to a freshly stored draft.
   * A new draft's uploads parked under the draft scope with no row id
   * yet; the bulk call hands them to the row the create just stored.
   * The stored row's project wins, falling back to the project the
   * visit filed (and parked the uploads) under.
   */
  function linkDraftAssets(created: SavedDraftRef): void {
    if (assetIds.length === 0) return;
    attachAssets.mutate({ projectId: created.project_id ?? projectId, issueId: created.id, assetIds });
  }

  /** Store a new draft (ISS-139/141): the caller handles the write. */
  function createDraftEntry(named: CreateFormState): void {
    if (onCopySaved === undefined) {
      // Standalone draft entry (a page that seeds no write): store it
      // through the same save write the inline footer uses.
      saveDraft.mutate(buildDraftInput(named), {
        onSettled: () => {
          submitting.current = false;
        },
        onSuccess: (created) => {
          linkDraftAssets(created);
          notifySuccess(DRAFT_SAVED_INLINE_TITLE, "Find it in your drafts when you're ready to finish it.");
          clearWorkItemDraft(workspaceSlug);
          finishClose();
        },
        onError: () => {
          submitting.current = false;
          notifyError(DRAFT_SAVE_FAILED_INLINE_TITLE, "Nothing was saved — check your connection and try again.");
        },
      });
      return;
    }
    void onCopySaved(buildDraftInput(named)).then(
      (created) => {
        submitting.current = false;
        linkDraftAssets(created);
        clearWorkItemDraft(workspaceSlug);
        finishClose();
      },
      () => {
        submitting.current = false;
        notifyError(DRAFT_SAVE_FAILED_INLINE_TITLE, "Nothing was saved — check your connection and try again.");
      }
    );
  }

  /**
   * Submit an edit: only the fields that changed travel (ISS-126). The
   * pre-submit gate runs first when one is set; a rejection aborts the
   * write and the form stays open. True when the write started.
   */
  async function submitEdit(named: CreateFormState): Promise<boolean> {
    if (baseline === null) return false;
    if (onBeforeEditSubmit !== undefined) {
      try {
        await onBeforeEditSubmit();
      } catch {
        return false;
      }
    }
    const target =
      edit?.kind === "draft"
        ? ({ kind: "draft", draftId: edit.id } as const)
        : ({ kind: "issue", projectId: baseline.projectId ?? "", issueId: edit?.id ?? "" } as const);
    const patch = buildEditPayload(named, baseline);
    writeEdit.mutate(
      {
        target,
        patch,
        membership: editMembership(named, baseline),
        draftProject: edit?.kind === "draft" ? changedDraftProject(named, baseline) : undefined,
      },
      {
        onSettled: () => {
          submitting.current = false;
        },
        onSuccess: () => {
          // The hand-back runs before the close cascade, so the caller
          // learns about the save ahead of onChanged (INT-020).
          onEditSaved?.(Object.keys(patch));
          const liveId = edit?.kind === "issue" ? edit.id : null;
          if (liveId !== null && assetIds.length > 0) {
            attachAssets.mutate({ projectId: baseline.projectId ?? "", issueId: liveId, assetIds });
          }
          notifySuccess(UPDATE_SUCCESS_TITLE);
          finishClose();
        },
        onError: (error) => {
          // The ref opens again so a fix can resubmit.
          submitting.current = false;
          const fieldError = executionTargetFieldError(error);
          if (fieldError === null) {
            notifyError(UPDATE_FAILED_TITLE, UPDATE_FAILED_BODY);
          } else {
            setExecutionError(fieldError);
          }
        },
      }
    );
    return true;
  }

  async function publishDraftOnce(): Promise<void> {
    if (busy) return;
    if (!canPublish) return;
    const id = draftTargetId;
    if (id === null) return;
    if (form.projectId === null || projectId === "") {
      notifyError(PUBLISH_NEEDS_PROJECT_TITLE, PUBLISH_NEEDS_PROJECT_BODY);
      return;
    }
    const title = await resolveTitle();
    if (title === null) return;
    const named = { ...form, name: title };
    if (moving && onPublish !== undefined) {
      // The drafts page owns the promote write, its toast and the list
      // refresh (ISS-139); a failure keeps the form open for a retry.
      submitting.current = true;
      try {
        await onPublish(buildPublishInput(named));
        finishClose();
      } catch {
        submitting.current = false;
      }
      return;
    }
    try {
      const promoted = await publish.mutateAsync({
        draftId: id,
        input: buildPublishInput(named),
        storedProjectId: edit?.projectId ?? null,
      });
      if (assetIds.length > 0) {
        attachAssets.mutate({ projectId: promoted.project_id, issueId: promoted.id, assetIds });
      }
      notifySuccess(PUBLISH_SUCCESS_TITLE, PUBLISH_SUCCESS_BODY);
      finishClose();
    } catch {
      notifyError(PUBLISH_FAILED_TITLE, PUBLISH_FAILED_BODY);
    }
  }

  async function saveAndClose(): Promise<void> {
    setDraftError(null);
    try {
      const created = await saveDraft.mutateAsync(buildDraftInput(form));
      linkDraftAssets(created);
    } catch {
      setDraftError("The draft didn't save. Check your connection and try again.");
      return;
    }
    notifySuccess(DRAFT_SAVED_INLINE_TITLE, "Find it in your drafts when you're ready to finish it.");
    clearWorkItemDraft(workspaceSlug);
    finishClose();
  }

  async function saveInlineDraft(): Promise<void> {
    setDraftError(null);
    try {
      const created = await saveDraft.mutateAsync(buildDraftInput(form));
      linkDraftAssets(created);
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
        title={CREATE_DIALOG_TITLE}
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

  const branchValue = form.gitWorkBranch;
  // A new draft stores as a draft, not as an issue: the submit names that
  // action (ISS-139/141), matching the standalone Save to Drafts entry.
  const saveLabel = editing && editSaveLabel !== undefined ? editSaveLabel : "Save";
  const submitLabel = busy ? "Saving…" : drafting ? SAVE_DRAFT_BUTTON_LABEL : saveLabel;
  // An edit diffs against the row it opened with; without one (a draft
  // or issue whose snapshot did not parse) there is nothing to save.
  const saveDisabled = busy || (editing && baseline === null);
  // A draft visit parks its description images under the draft scope:
  // a new draft has no row id yet, so the save links them afterwards,
  // while a stored draft's id travels on the upload itself.
  const assetScope = {
    entityType: drafting || draftTargetId !== null ? DRAFT_DESCRIPTION_ENTITY_TYPE : ISSUE_DESCRIPTION_ENTITY_TYPE,
    entityId: draftTargetId ?? "",
  } as const;
  // The context surface hides the membership pickers while the route
  // supplies them (ISS-124): the cycle page seeds a cycle, the module
  // page a module, and the page's own switch now lives in the layout.
  const hasContext = !editing && (context?.cycleId != null || context?.moduleId != null);
  const showCycle = detail?.cycle_view === true && !hasContext;
  const showModules = detail?.module_view === true && !hasContext;
  // The estimate field follows the project link (EST-018), never the
  // systems list: a project with systems but no link shows no field.
  // The picker inside reads lazily on open, preloading only when a
  // stored point needs its name (EST-017), as the sidebar row does.
  const showEstimate = detail?.estimate != null;

  return (
    <>
      <Dialog
        title={editing ? (editTitle ?? EDIT_DIALOG_TITLE) : drafting ? CREATE_DRAFT_DIALOG_TITLE : CREATE_DIALOG_TITLE}
        description={
          editing
            ? "Changed fields are saved; the rest stay as they are."
            : drafting
              ? "Half-finished is fine; drafts keep whatever you have so far."
              : "Set the details now; you can refine them later."
        }
        open={open}
        onOpenChange={handleOpenChange}
        ignoreEscapeKey={busy}
        className="max-w-2xl"
        actions={
          <>
            <TabStop index={MODAL_TAB_ORDER.discard} label="Discard">
              <Button variant="ghost" onClick={requestClose} disabled={busy} tabIndex={-1}>
                Discard
              </Button>
            </TabStop>
            {editing || draftTargetId !== null || drafting ? null : (
              <Button variant="secondary" onClick={() => void saveInlineDraft()} disabled={busy}>
                {SAVE_DRAFT_BUTTON_LABEL}
              </Button>
            )}
            {moving && canPublish ? (
              <Button type="button" variant="secondary" onClick={() => void publishDraftOnce()} disabled={busy}>
                {MOVE_TO_PROJECT_BUTTON_LABEL}
              </Button>
            ) : null}
            {draftTargetId === null || moving || !canPublish ? null : (
              <Button variant="secondary" onClick={() => void publishDraftOnce()} disabled={busy}>
                {PUBLISH_BUTTON_LABEL}
              </Button>
            )}
            <TabStop index={MODAL_TAB_ORDER.save} label="Save">
              <Button
                type="submit"
                form="work-item-create-form"
                variant="primary"
                disabled={saveDisabled}
                tabIndex={-1}
              >
                {submitLabel}
              </Button>
            </TabStop>
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
              includeProjectIds={creatableIds}
              knownProjects={projectLabels}
              // A stored row keeps the project it was filed under — a draft
              // changing where it publishes does so through its own picker
              // entry, and the move entry reads the stored project (the
              // promote endpoint files under it, ISS-139).
              disabled={busy || lockedProject !== undefined || (editing && (edit?.kind !== "draft" || moving))}
            />
            {editing || drafting ? null : <DuplicatePanel matches={duplicates.matches} />}
          </div>
          {issueType === null ? null : (
            <IssueTypeSlots
              extension={issueType}
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              disabled={busy}
              typeId={typeId}
              propertyValues={propertyValues}
              onTypeChange={setTypeId}
              onPropertyValuesChange={setPropertyValues}
              onTemplateApply={(prefill) => {
                setForm((current) => applyTemplatePrefill(current, prefill));
                if (prefill.typeId !== undefined) setTypeId(prefill.typeId);
                if (prefill.propertyValues !== undefined) setPropertyValues(prefill.propertyValues);
                setEditorGeneration((current) => current + 1);
              }}
            />
          )}
          <Input
            id="name"
            inputRef={titleRef}
            label="Title"
            placeholder="Title"
            value={form.name}
            onChange={(event) => updateTitle(event.target.value)}
            {...(fieldError === null ? {} : { error: fieldError })}
            tabIndex={MODAL_TAB_ORDER.title}
            autoComplete="off"
          />
          <ModalDescription
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            value={form.descriptionHtml}
            onChange={(html) => patch({ descriptionHtml: html })}
            disabled={busy}
            title={form.name}
            assistantConfigured={writingHelp}
            assetScope={assetScope}
            onAssetUploaded={(assetId) => setAssetIds((current) => [...current, assetId])}
            generation={editorGeneration}
            tabIndex={MODAL_TAB_ORDER.description}
            onReadinessChange={setDescriptionReady}
          />
          <div className="flex flex-wrap items-start gap-(--space-4)">
            <FieldSlot label="State">
              <StatePicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                value={form.stateId}
                onChange={(stateId) => {
                  setStateError(null);
                  patch({ stateId });
                }}
                disabled={busy}
                tabIndex={MODAL_TAB_ORDER.state}
                optionGate={stateGate}
              />
              {stateError === null ? null : (
                <p role="alert" className="text-meta text-(--danger)">
                  {stateError}
                </p>
              )}
            </FieldSlot>
            <FieldSlot label="Runs on">
              <PodPicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                value={form.execution}
                onChange={(execution: ExecutionSelection) => {
                  setExecutionError(null);
                  patch({ execution });
                }}
                disabled={busy}
                saveError={executionError}
              />
            </FieldSlot>
            <FieldSlot label="Priority">
              <PriorityPicker
                value={form.priority as PriorityLevel}
                onChange={(priority) => patch({ priority })}
                disabled={busy}
                tabIndex={MODAL_TAB_ORDER.priority}
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
                tabIndex={MODAL_TAB_ORDER.assignees}
              />
            </FieldSlot>
            <FieldSlot label="Labels">
              <div role="group" aria-label="Labels" tabIndex={MODAL_TAB_ORDER.labels} className="flex items-center">
                <span className="sr-only">Labels</span>
                <LabelPicker
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  value={form.labelIds}
                  onChange={(labelIds) => patch({ labelIds })}
                  disabled={busy}
                  emptyText=""
                />
              </div>
            </FieldSlot>
            <FieldSlot label="Start date" hideLabel>
              <div
                role="group"
                aria-label="Start date"
                tabIndex={MODAL_TAB_ORDER.startDate}
                className="flex items-center"
              >
                <DatePicker
                  label="Start date"
                  placeholder="Start date"
                  value={form.startDate}
                  onChange={(startDate) => patch({ startDate })}
                  max={form.targetDate}
                  weekStart={weekStart}
                  disabled={busy}
                />
              </div>
            </FieldSlot>
            <FieldSlot label="Due date" hideLabel>
              <div role="group" aria-label="Due date" tabIndex={MODAL_TAB_ORDER.dueDate} className="flex items-center">
                <DatePicker
                  label="Due date"
                  placeholder="Due date"
                  value={form.targetDate}
                  onChange={(targetDate) => patch({ targetDate })}
                  min={form.startDate}
                  weekStart={weekStart}
                  disabled={busy}
                />
              </div>
            </FieldSlot>
            {showCycle ? (
              <FieldSlot label="Cycle" hideLabel>
                <CyclePicker
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  value={form.cycleId}
                  onChange={(cycleId) => patch({ cycleId })}
                  disabled={busy}
                  placeholder="Cycle"
                  triggerId="Cycle"
                  tabIndex={MODAL_TAB_ORDER.cycle}
                />
              </FieldSlot>
            ) : null}
            {showModules ? (
              <FieldSlot label="Modules" hideLabel>
                <ModulePicker
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  multiple
                  value={form.moduleIds}
                  onChange={(moduleIds) => patch({ moduleIds })}
                  disabled={busy}
                  placeholder="Modules"
                  triggerId="Modules"
                  tabIndex={MODAL_TAB_ORDER.modules}
                />
              </FieldSlot>
            ) : null}
            {showEstimate ? (
              <FieldSlot label="Estimate">
                <EstimatePicker
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  value={form.estimatePoint}
                  onChange={(estimatePoint) => patch({ estimatePoint })}
                  disabled={busy}
                  tabIndex={MODAL_TAB_ORDER.estimate}
                  preload={form.estimatePoint != null}
                />
              </FieldSlot>
            ) : null}
            <FieldSlot label="Parent">
              <TabStop index={MODAL_TAB_ORDER.parent} label="Parent">
                <ParentPicker
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  value={parentHit}
                  onChange={(hit) => {
                    setParentHit(hit);
                    patch({ parentId: hit?.id ?? null });
                  }}
                  disabled={busy || lockedParent}
                />
              </TabStop>
            </FieldSlot>
          </div>
          <div className="flex flex-col gap-(--space-2)">
            <Button
              variant="ghost"
              aria-expanded={gitOpen}
              aria-controls="work-item-git-section"
              onClick={() => setGitOpen((current) => !current)}
              className="self-start"
            >
              {ADVANCED_GIT_LABEL}
            </Button>
            {gitOpen ? (
              <div id="work-item-git-section">
                <Input
                  id="git_work_branch"
                  label={GIT_BRANCH_LABEL}
                  placeholder={GIT_BRANCH_PLACEHOLDER}
                  value={branchValue}
                  maxLength={GIT_BRANCH_MAX_LENGTH + 64}
                  onChange={(event) => updateBranch(event.target.value)}
                  {...(gitError === null ? {} : { error: gitError })}
                  autoComplete="off"
                />
              </div>
            ) : null}
          </div>
          {editing || draftTargetId !== null || drafting ? null : (
            <div
              role="group"
              aria-label="Create more"
              tabIndex={MODAL_TAB_ORDER.createMore}
              className="flex items-center gap-(--space-2)"
            >
              <Switch label="Create more" checked={createMore} onCheckedChange={setCreateMore} />
              <span aria-hidden="true" className="text-body text-(--text)">
                Create more
              </span>
            </div>
          )}
        </form>
      </Dialog>
      <DiscardDraftDialog
        open={discardOpen}
        saving={saveDraft.isPending}
        error={draftError}
        onCancel={() => setDiscardOpen(false)}
        onDiscard={discardAndClose}
        onSaveDraft={saveAndClose}
      />
    </>
  );
}

/** Fill a form from a template's prefill (ISS-129). */
function applyTemplatePrefill(form: CreateFormState, prefill: TemplatePrefill): CreateFormState {
  return {
    ...form,
    ...(prefill.name === undefined ? {} : { name: prefill.name }),
    ...(prefill.descriptionHtml === undefined ? {} : { descriptionHtml: prefill.descriptionHtml }),
    ...(prefill.stateId === undefined ? {} : { stateId: prefill.stateId }),
    ...(prefill.priority === undefined ? {} : { priority: prefill.priority }),
    ...(prefill.assigneeIds === undefined ? {} : { assigneeIds: [...prefill.assigneeIds] }),
    ...(prefill.labelIds === undefined ? {} : { labelIds: [...prefill.labelIds] }),
  };
}

/** The cloud trio (ISS-129): type, template and extra properties. */
function IssueTypeSlots({
  extension,
  ...props
}: IssueTypeExtensionProps & { extension: IssueTypeExtension }): React.ReactElement {
  const TypePicker = extension.typePicker;
  const TemplatePicker = extension.templatePicker;
  const AdditionalProperties = extension.additionalProperties;
  return (
    <div className="flex flex-col gap-(--space-4)">
      {TypePicker === undefined ? null : (
        <FieldSlot label="Issue type">
          <TypePicker {...props} />
        </FieldSlot>
      )}
      {TemplatePicker === undefined ? null : (
        <FieldSlot label="Template">
          <TemplatePicker {...props} />
        </FieldSlot>
      )}
      {AdditionalProperties === undefined ? null : <AdditionalProperties {...props} />}
    </div>
  );
}
