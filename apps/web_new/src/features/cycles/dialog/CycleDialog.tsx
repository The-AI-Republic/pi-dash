// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle save dialog (parity/specs/cycles.md §6 and §7, CYC-014–021).
// One dialog for every entry point (the list header, the zero-state
// shortcut, the palette command, and later the row menu and the detail
// surfaces) in two modes: create opens on defaults with the project
// picker, update opens prefilled from the record without it (a cycle
// never moves project). Opening resets the form. Create success closes,
// toasts, remembers the all-cycles tab and refreshes the shared cycle
// reads; an overlap refusal toasts the draft workaround and keeps the
// dialog open; a one-sided create range is refused inline before any
// request; any other failure shows inline with the input intact.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { checkCycleDates, createCycle, updateCycle, type CycleRow, type ProjectDetails } from "@pidash/api-client";
import { Button, Dialog, Input, Spinner, Textarea } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useProjectRoles } from "../../auth/index.js";
import { DateRangePicker, todayIsoDay, type RangeValue } from "../../issues/index.js";
import { isProjectAuthorized, projectDetailsQueryOptions } from "../../projects/index.js";
import { ProjectPicker, toProjectOption } from "../../../shared/pickers/index.js";
import { notifyError, notifySuccess, storePreference } from "../../../shared/shell/index.js";
import { invalidateAfterCycleWrite } from "../api/invalidate.js";
import { instantToProjectDate } from "../lib/cycleDates.js";
import {
  CYCLE_OVERLAP_TITLE,
  cycleFailureMessage,
  cycleFormDays,
  cycleOverlapMessage,
  needsCycleDateCheck,
  validateCycleRange,
} from "./cycleRange.js";
import { validateCycleTitle } from "./cycleTitle.js";

/** Remembered list tab (spec §6, CYC-018): creation lands it on "all". */
export const CYCLE_LIST_TAB_KEY = "cycle_tab";

/** A project the dialog can label before the picker read resolves. */
export interface CycleProjectRef {
  id: string;
  name: string;
  identifier: string;
}

export interface CreateCycleDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  projectId: string;
  /** The project in the address: the picker's default and its pre-read label. */
  currentProject: CycleProjectRef;
}

export interface UpdateCycleDialogProps extends CreateCycleDialogProps {
  /** The record being edited; the form prefills from it. */
  cycle: CycleRow;
}

type DialogMode = "create" | "update";

type SaveCycleDialogProps =
  | (CreateCycleDialogProps & { mode: "create"; cycle?: undefined })
  | (UpdateCycleDialogProps & { mode: "update" });

const DIALOG_COPY: Record<DialogMode, { title: string; submit: string; submitting: string }> = {
  create: { title: "Create cycle", submit: "Create cycle", submitting: "Creating cycle…" },
  update: { title: "Update cycle", submit: "Update cycle", submitting: "Updating cycle…" },
};

interface SaveCycleInitial {
  title: string;
  description: string;
  range: RangeValue;
}

/** Update-mode prefill: the record's stored values as form state. */
function initialFromRecord(cycle: CycleRow, timeZone: string | undefined): SaveCycleInitial {
  return {
    title: cycle.name,
    description: cycle.description ?? "",
    range: {
      start: cycle.start_date === null ? null : instantToProjectDate(cycle.start_date, timeZone),
      end: cycle.end_date === null ? null : instantToProjectDate(cycle.end_date, timeZone),
    },
  };
}

/** Create-mode prefill: blank title and an undated range. */
function blankInitial(): SaveCycleInitial {
  return { title: "", description: "", range: { start: null, end: null } };
}

interface SaveCycleFormBase {
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  projectId: string;
  currentProject: CycleProjectRef;
  onSubmittingChange: (submitting: boolean) => void;
}

// An explicit union (Omit would collapse the discriminant): the mode
// narrows the record, so update always carries one.
type SaveCycleFormProps =
  | (SaveCycleFormBase & { mode: "create"; cycle?: undefined })
  | (SaveCycleFormBase & { mode: "update"; cycle: CycleRow });

/**
 * The shared workspace details read. The update form mounts only after
 * it resolves; create mounts at once and reads it reactively, so the
 * value starts undefined there and after a failed read, degrading to
 * system time per cycleDates.
 */
type SaveCycleFormBodyProps = SaveCycleFormProps & { projectDetails: ProjectDetails[] | undefined };

function SaveCycleForm(props: SaveCycleFormProps): React.ReactElement {
  // The update prefill reads stored instants in the project timezone,
  // so the update form waits for this read: mounting on the unresolved
  // zone would lock the browser zone into the prefill (spec §7). Create
  // starts blank and reads the zone reactively, so it mounts at once.
  const details = useQuery(projectDetailsQueryOptions(getAppClient(), props.workspaceSlug));
  if (props.mode === "update" && details.isPending) {
    return (
      <div className="flex justify-center py-(--space-6)">
        <Spinner label="Loading project details" />
      </div>
    );
  }
  return <SaveCycleFormBody {...props} projectDetails={details.data} />;
}

function SaveCycleFormBody(props: SaveCycleFormBodyProps): React.ReactElement {
  const { onOpenChange, workspaceSlug, projectId, currentProject, onSubmittingChange, projectDetails } = props;
  const queryClient = useQueryClient();
  const [targetProjectId, setTargetProjectId] = React.useState(projectId);
  // The retargeted project (create) or the cycle's own (update): its
  // feature flag and timezone, from the shared workspace details read.
  const effectiveTarget = props.mode === "create" ? targetProjectId : projectId;
  const targetDetails = projectDetails?.find((row) => row.id === effectiveTarget);
  const roles = useProjectRoles(workspaceSlug);
  const timeZone = targetDetails?.timezone;
  // The form remounts on every open (the chrome keys it by session),
  // so these initializers run once per visit, never across visits. The
  // outer form resolves the details first, so update's prefill sees the
  // project zone at mount; only a failed read falls back to system time.
  // The mode union narrows the record: update always carries one.
  const initial = React.useMemo(
    // Per-visit by design: the chrome remounts this form on every open.
    () => (props.mode === "update" ? initialFromRecord(props.cycle, timeZone) : blankInitial()),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- per-visit by design (see above).
    [props.mode]
  );
  const [title, setTitle] = React.useState(initial.title);
  const [titleError, setTitleError] = React.useState<string | null>(null);
  const [description, setDescription] = React.useState(initial.description);
  const [range, setRange] = React.useState<RangeValue>(initial.range);
  const [rangeError, setRangeError] = React.useState<string | null>(null);
  const [targetError, setTargetError] = React.useState<string | null>(null);
  const [formError, setFormError] = React.useState<string | null>(null);
  const [submitting, setSubmitting] = React.useState(false);
  const titleRef = React.useRef<HTMLInputElement | null>(null);
  const copy = DIALOG_COPY[props.mode];

  // The title takes focus on every open. The ref is null while the
  // dialog is closed (the portal unmounts), so the call is guarded. The
  // dialog itself takes no initial focus (initialFocus={false}): its
  // open-time pass would otherwise reclaim the container after this
  // gated form mounts.
  React.useEffect(() => {
    titleRef.current?.focus();
  }, []);

  /** Projects where the viewer may create cycles (spec §6). */
  const creatableProjectIds = React.useMemo(() => {
    if (roles.data === undefined) return undefined;
    return Object.entries(roles.data)
      .filter(([, role]) => isProjectAuthorized(role))
      .map(([id]) => id);
  }, [roles.data]);

  function setPending(next: boolean): void {
    setSubmitting(next);
    onSubmittingChange(next);
  }

  function retarget(next: string): void {
    setTargetProjectId(next);
    setTargetError(null);
    setFormError(null);
  }

  async function submitCreate(target: string): Promise<void> {
    const days = cycleFormDays(range);
    if (needsCycleDateCheck(null, range, timeZone)) {
      const check = await checkCycleDates(getAppClient(), workspaceSlug, target, {
        start_date: range.start,
        end_date: range.end,
      });
      if (!check.status) {
        notifyError(CYCLE_OVERLAP_TITLE, cycleOverlapMessage("create"));
        return;
      }
    }
    const created = await createCycle(getAppClient(), workspaceSlug, target, {
      name: title.trim(),
      description,
      start_date: days.start_date,
      end_date: days.end_date,
    });
    await invalidateAfterCycleWrite(queryClient, workspaceSlug, target, created.id);
    storePreference(CYCLE_LIST_TAB_KEY, "all");
    notifySuccess("Cycle created successfully.");
    onOpenChange(false);
  }

  async function submitUpdate(record: CycleRow): Promise<void> {
    const days = cycleFormDays(range);
    if (needsCycleDateCheck({ start_date: record.start_date, end_date: record.end_date }, range, timeZone)) {
      const check = await checkCycleDates(getAppClient(), workspaceSlug, projectId, {
        start_date: range.start,
        end_date: range.end,
        cycle_id: record.id,
      });
      if (!check.status) {
        notifyError(CYCLE_OVERLAP_TITLE, cycleOverlapMessage("update"));
        return;
      }
    }
    await updateCycle(getAppClient(), workspaceSlug, projectId, record.id, {
      name: title.trim(),
      description,
      start_date: days.start_date,
      end_date: days.end_date,
    });
    await invalidateAfterCycleWrite(queryClient, workspaceSlug, projectId, record.id);
    notifySuccess("Cycle updated successfully.");
    onOpenChange(false);
  }

  async function submit(event: React.FormEvent): Promise<void> {
    event.preventDefault();
    const error = validateCycleTitle(title);
    if (error !== null) {
      setTitleError(error);
      // The submitter is mounted, so the input is too.
      titleRef.current!.focus();
      return;
    }
    const rangeViolation = validateCycleRange(range, props.mode);
    if (rangeViolation !== null) {
      setRangeError(rangeViolation);
      return;
    }
    if (props.mode === "create" && targetDetails?.cycle_view === false) {
      setTargetError("Cycles are turned off for this project.");
      return;
    }
    setPending(true);
    try {
      if (props.mode === "update") {
        await submitUpdate(props.cycle);
      } else {
        await submitCreate(targetProjectId);
      }
    } catch (submitError) {
      setFormError(cycleFailureMessage(submitError, props.mode));
    } finally {
      setPending(false);
    }
  }

  return (
    <form
      data-testid="cycle-dialog-form"
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <div className="flex flex-col gap-(--space-4)">
        {props.mode === "create" ? (
          <div data-testid="cycle-project-field" className="flex flex-col gap-(--space-2)">
            <span className="text-body font-medium text-(--text)">Project</span>
            <ProjectPicker
              workspaceSlug={workspaceSlug}
              value={targetProjectId}
              onChange={retarget}
              includeProjectIds={creatableProjectIds}
              knownProjects={[toProjectOption(currentProject)]}
              disabled={submitting}
            />
            {targetError === null ? null : (
              <p role="alert" className="text-meta text-(--danger)">
                {targetError}
              </p>
            )}
          </div>
        ) : null}
        <Input
          label="Title"
          name="name"
          placeholder="Title"
          value={title}
          inputRef={titleRef}
          {...(titleError === null ? {} : { error: titleError })}
          disabled={submitting}
          onBlur={() => {
            setTitleError(validateCycleTitle(title));
          }}
          onChange={(event) => {
            const next = event.target.value;
            setTitle(next);
            if (titleError !== null) setTitleError(validateCycleTitle(next));
          }}
        />
        <Textarea
          label="Description"
          name="description"
          placeholder="Description"
          rows={3}
          value={description}
          disabled={submitting}
          onChange={(event) => {
            setDescription(event.target.value);
          }}
        />
        <div data-testid="cycle-date-field" className="flex flex-col gap-(--space-2)">
          <span className="text-body font-medium text-(--text)">Dates</span>
          <DateRangePicker
            mode="split"
            label="Start date End date"
            value={range}
            onChange={(next) => {
              setRange(next);
              if (rangeError !== null) setRangeError(validateCycleRange(next, props.mode));
            }}
            min={todayIsoDay()}
            clearable
            disabled={submitting}
            testId="cycle-date-range"
          />
          {rangeError === null ? null : (
            <p role="alert" className="text-meta text-(--danger)">
              {rangeError}
            </p>
          )}
        </div>
        {formError === null ? null : (
          <p role="alert" data-testid="cycle-dialog-error" className="text-meta text-(--danger)">
            {formError}
          </p>
        )}
      </div>
      <div className="mt-(--space-6) flex justify-end gap-(--space-3)">
        <Button
          variant="secondary"
          type="button"
          onClick={() => {
            onOpenChange(false);
          }}
        >
          Cancel
        </Button>
        <Button variant="primary" type="submit" loading={submitting} disabled={submitting}>
          {submitting ? copy.submitting : copy.submit}
        </Button>
      </div>
    </form>
  );
}

/**
 * Controlled save dialog. The host owns `open`; the form remounts on
 * every open so each visit starts from its prefill. While a submit is in
 * flight an outside press does not close it; Escape and Cancel still do.
 */
function SaveCycleDialog(props: SaveCycleDialogProps): React.ReactElement {
  const { open, onOpenChange } = props;
  const copy = DIALOG_COPY[props.mode];
  const [session, setSession] = React.useState(0);
  const [submitting, setSubmitting] = React.useState(false);
  const wasOpen = React.useRef(open);
  React.useEffect(() => {
    if (open && !wasOpen.current) setSession((count) => count + 1);
    wasOpen.current = open;
  }, [open]);
  const shared = {
    onOpenChange,
    workspaceSlug: props.workspaceSlug,
    projectId: props.projectId,
    currentProject: props.currentProject,
    onSubmittingChange: setSubmitting,
  };

  return (
    <Dialog
      title={copy.title}
      open={open}
      onOpenChange={(next, details) => {
        // A controlled dialog without a trigger only ever reports closing;
        // while a submit flies an outside press is swallowed and every
        // other dismissal (Escape, the footer) still closes.
        if (submitting && details.reason === "outside-press") return;
        onOpenChange(next);
      }}
      closeButton={false}
      initialFocus={false}
    >
      {props.mode === "update" ? (
        <SaveCycleForm key={session} {...shared} mode="update" cycle={props.cycle} />
      ) : (
        <SaveCycleForm key={session} {...shared} mode="create" />
      )}
    </Dialog>
  );
}

/** Controlled create dialog; each visit starts from defaults. */
export function CreateCycleDialog(props: CreateCycleDialogProps): React.ReactElement {
  return <SaveCycleDialog mode="create" {...props} />;
}

/** Controlled edit dialog; each visit starts prefilled from the record. */
export function UpdateCycleDialog(props: UpdateCycleDialogProps): React.ReactElement {
  return <SaveCycleDialog mode="update" {...props} />;
}
