// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project save-view dialog (parity/specs/views.md §5, VIEW-003,
// VIEW-013, VIEW-014, VIEW-019; §6, VIEW-015; §8, VIEW-048). One dialog
// for every entry point (the list header, the zero-views action, the
// palette command, the detail's Save as) in two modes: create opens on
// defaults, or on the Save-as prefill when one is provided; update opens
// prefilled from the record. Opening resets the form. Create success
// closes, toasts and navigates to the new view; a failed create keeps
// the dialog open with the input intact for retry (NEWFRONT-221's reset
// is the old bug, not the target). Update success is silent, closes,
// and adopts the PATCH echo into the cached record; a failed update
// toasts and restores the prefilled originals.

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import {
  createProjectView,
  updateProjectView,
  type SavedView,
  type SavedViewCreate,
  type SavedViewList,
  type SavedViewPatch,
} from "@pidash/api-client";
import { Button, Dialog, Input, Textarea } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { edition, type ViewAccessValue } from "../../../core/edition/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  DisplayControl,
  IssuesLoaderProvider,
  LayoutPicker,
  loadRichFilterBuilder,
  mergeDisplayPrefs,
  parseDisplayPrefs,
  type DisplayPrefs,
  type FieldConfigRegistry,
  type IssuesEntity,
} from "../../issues/index.js";
import { viewKeys } from "../api/keys.js";
import { replaceProjectViewRow } from "../lib/projectViewActions.js";
import { defaultCreateDisplay, normalizeCreateDisplay, serializeCreateDisplay } from "./createDisplay.js";
import { FilterSectionLoader } from "./FilterSectionLoader.js";
import { readRegistryExpression } from "./registryExpression.js";
import { ViewAccessField, SHARED_ACCESS } from "./ViewAccessField.js";
import { ViewAccessSummary } from "./ViewAccessSummary.js";
import { ViewIconPicker, type ViewLogoProps } from "./ViewIconPicker.js";
import { parseViewLogo } from "./viewLogo.js";
import { validateViewTitle } from "./viewTitle.js";

/**
 * Save-as prefill (parity/specs/views.md §8, VIEW-048): the duplicate opens
 * on the given title with the live expression and display snapshot. Only
 * these three ride along; description, icon and access stay at the create
 * defaults.
 */
export interface SaveViewPrefill {
  title: string;
  display: DisplayPrefs;
  expression: Record<string, unknown>;
}

export interface CreateViewDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  projectId: string;
  /** Cycle/module entries hide while their project features are off. */
  hideCycle: boolean;
  hideModule: boolean;
  /**
   * Create-mode prefill for the Save-as entry; absent on plain creates,
   * which open on defaults. Never set in update mode.
   */
  prefill?: SaveViewPrefill | undefined;
}

export interface UpdateViewDialogProps extends CreateViewDialogProps {
  /** The record being edited; the form prefills from it. */
  view: SavedView;
}

type DialogMode = "create" | "update";

type SaveViewDialogProps =
  | (CreateViewDialogProps & { mode: "create"; view?: undefined })
  | (UpdateViewDialogProps & { mode: "update" });

const DIALOG_COPY: Record<DialogMode, { title: string; description: string; submit: string; submitting: string }> = {
  create: {
    title: "Create View",
    description: "Name the view, pick its layout, and add filters.",
    submit: "Create View",
    submitting: "Creating view…",
  },
  update: {
    title: "Update View",
    description: "Change the name, layout, or filters of this view.",
    submit: "Update View",
    submitting: "Updating view…",
  },
};

/** Server detail when the body carries one; null otherwise. */
function serverDetail(error: unknown): string | null {
  const body = (error as { body?: unknown }).body ?? null;
  if (typeof body === "object" && body !== null && !Array.isArray(body)) {
    const detail = (body as Record<string, unknown>)["detail"];
    if (typeof detail === "string" && detail.length > 0) return detail;
  }
  return null;
}

/** Server detail when the body carries one; the fixed copy otherwise. */
function failureDetail(error: unknown): string {
  return serverDetail(error) ?? "Nothing was created — check the details and try again.";
}

/** Server detail when the body carries one; the fixed copy otherwise. */
function updateFailureDetail(error: unknown): string {
  return serverDetail(error) ?? "Nothing was changed — check the details and try again.";
}

interface SaveViewInitial {
  title: string;
  description: string;
  logo: ViewLogoProps | null;
  display: DisplayPrefs;
}

/** Update-mode prefill: the record's stored values as form state. */
function initialFromRecord(view: SavedView): SaveViewInitial {
  return {
    title: view.name,
    description: view.description,
    logo: parseViewLogo(view.logo_props),
    display: parseDisplayPrefs(view.display_filters, view.display_properties),
  };
}

/** Create-mode prefill: blank title, no logo, List grouped by state. */
function blankInitial(): SaveViewInitial {
  return { title: "", description: "", logo: null, display: defaultCreateDisplay() };
}

/** Save-as prefill: the duplicate's title and live display snapshot. */
function saveAsInitial(prefill: SaveViewPrefill): SaveViewInitial {
  return { title: prefill.title, description: "", logo: null, display: prefill.display };
}

interface SaveViewFormBase {
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  projectId: string;
  hideCycle: boolean;
  hideModule: boolean;
  onSubmittingChange: (submitting: boolean) => void;
}

// An explicit union (Omit would collapse the discriminant): the mode
// narrows the record, so update always carries one; create optionally
// carries the Save-as prefill.
type SaveViewFormProps =
  | (SaveViewFormBase & { mode: "create"; view?: undefined; prefill?: SaveViewPrefill | undefined })
  | (SaveViewFormBase & { mode: "update"; view: SavedView });

function SaveViewForm(props: SaveViewFormProps): React.ReactElement {
  const { onOpenChange, workspaceSlug, projectId, hideCycle, hideModule, onSubmittingChange } = props;
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  // The form remounts on every open (the chrome keys it by session),
  // so these initializers run once per visit, never across visits.
  // The mode union narrows the record: update always carries one.
  const initial = React.useMemo(
    // Per-visit by design: the chrome remounts this form on every open.
    () =>
      props.mode === "update"
        ? initialFromRecord(props.view)
        : props.prefill === undefined
          ? blankInitial()
          : saveAsInitial(props.prefill),
    [props.mode]
  );
  const [title, setTitle] = React.useState(initial.title);
  const [titleError, setTitleError] = React.useState<string | null>(null);
  const [description, setDescription] = React.useState(initial.description);
  const [logo, setLogo] = React.useState<ViewLogoProps | null>(initial.logo);
  const [display, setDisplay] = React.useState<DisplayPrefs>(initial.display);
  const [access, setAccess] = React.useState<ViewAccessValue>(SHARED_ACCESS);
  const [submitting, setSubmitting] = React.useState(false);
  // Remounts the temporary filter builder so a failed update restores
  // the prefilled expression along with the other fields.
  const [filterRun, setFilterRun] = React.useState(0);
  const titleRef = React.useRef<HTMLInputElement | null>(null);
  const registryRef = React.useRef<FieldConfigRegistry | null>(null);
  const entity = React.useMemo<IssuesEntity>(() => ({ scope: "project", projectId }), [projectId]);
  const copy = DIALOG_COPY[props.mode];

  // The title takes focus on every open; the form remounts per session and
  // the input is always mounted when this runs.
  React.useEffect(() => {
    titleRef.current!.focus();
  }, []);

  function setPending(next: boolean): void {
    setSubmitting(next);
    onSubmittingChange(next);
  }

  /** Back to what the dialog opened with (VIEW-015's failure path). */
  function restoreInitial(): void {
    setTitle(initial.title);
    setTitleError(null);
    setDescription(initial.description);
    setLogo(initial.logo);
    setDisplay(initial.display);
    setFilterRun((run) => run + 1);
  }

  async function submitCreate(): Promise<void> {
    // The builder module is usually already loaded (its section
    // rendered), so this reuses the cached request; a failed load
    // keeps the dialog open through the same failure path as a
    // failed write. The registry may still be null on an early
    // submit; that reads as the empty expression.
    const builder = await loadRichFilterBuilder();
    const expression = readRegistryExpression(registryRef.current, builder.serializeRichExpression);
    const settings = normalizeCreateDisplay(display);
    const { displayFilters, displayProperties } = serializeCreateDisplay(settings);
    const payload: SavedViewCreate = {
      ...builder.buildViewCreate(
        { expression, displayFilters, displayProperties },
        { name: title.trim(), description }
      ),
      ...(logo === null ? {} : { logo_props: logo }),
    };
    const created = await createProjectView(getAppClient(), workspaceSlug, projectId, payload);
    queryClient.setQueryData(builder.savedViewKeys.project(workspaceSlug, projectId, created.id), created);
    // The acting user's own create updates the list at once (VIEW-043):
    // a fetched list gains the row without waiting out its stale time,
    // while a never-fetched list stays unfetched and reads fresh on mount.
    queryClient.setQueryData<SavedViewList>(viewKeys.projectList(workspaceSlug, projectId), (current) =>
      current === undefined ? current : [created, ...current]
    );
    const slot = edition.slots?.viewAccess;
    if (slot !== undefined && access !== SHARED_ACCESS) {
      try {
        await slot.storeAccess({ workspaceSlug, projectId, viewId: created.id, access });
      } catch (storeError) {
        // The view exists; only the access choice failed to stick. The
        // user keeps the created view and learns the choice did not land
        // (a retry here would create a second view, so the dialog closes).
        notifyError("View created, but the access choice could not be saved", failureDetail(storeError));
        onOpenChange(false);
        await navigate({
          to: "/$ws/projects/$projectId/views/$viewId",
          params: { ws: workspaceSlug, projectId, viewId: created.id },
        });
        return;
      }
    }
    notifySuccess("View created successfully");
    onOpenChange(false);
    await navigate({
      to: "/$ws/projects/$projectId/views/$viewId",
      params: { ws: workspaceSlug, projectId, viewId: created.id },
    });
  }

  async function submitUpdate(record: SavedView): Promise<void> {
    const builder = await loadRichFilterBuilder();
    const expression = readRegistryExpression(registryRef.current, builder.serializeRichExpression);
    const settings = normalizeCreateDisplay(display);
    const { displayFilters, displayProperties } = builder.viewDisplaySnapshot(record, settings);
    const payload: SavedViewPatch = {
      ...builder.buildViewPatch({ expression, displayFilters, displayProperties }),
      name: title.trim(),
      description,
      ...(logo === null ? {} : { logo_props: logo }),
    };
    const echo = await updateProjectView(getAppClient(), workspaceSlug, projectId, record.id, payload);
    // Silent save: the list row and the detail record adopt the echo.
    queryClient.setQueryData(viewKeys.projectList(workspaceSlug, projectId), (list: SavedViewList | undefined) =>
      replaceProjectViewRow(list, echo)
    );
    queryClient.setQueryData(builder.savedViewKeys.project(workspaceSlug, projectId, echo.id), echo);
    onOpenChange(false);
  }

  async function submit(event: React.FormEvent): Promise<void> {
    event.preventDefault();
    const error = validateViewTitle(title);
    if (error !== null) {
      setTitleError(error);
      titleRef.current!.focus();
      return;
    }
    setPending(true);
    try {
      if (props.mode === "update") {
        await submitUpdate(props.view);
      } else {
        await submitCreate();
      }
    } catch (error) {
      if (props.mode === "update") {
        // Keep the dialog open on the prefilled originals (VIEW-015):
        // nothing that was saved is lost.
        notifyError("Failed to update view", updateFailureDetail(error));
        restoreInitial();
      } else {
        // Keep the dialog open with the input intact for retry (VIEW-014).
        notifyError("Failed to create view", failureDetail(error));
      }
    } finally {
      setPending(false);
    }
  }

  return (
    <form
      data-testid="create-view-form"
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <div className="flex flex-col gap-(--space-4)">
        <Input
          label="Title"
          placeholder="Title"
          value={title}
          inputRef={titleRef}
          {...(titleError === null ? {} : { error: titleError })}
          disabled={submitting}
          onChange={(event) => {
            const next = event.target.value;
            setTitle(next);
            if (titleError !== null) setTitleError(validateViewTitle(next));
          }}
        />
        <Textarea
          label="Description"
          placeholder="Description"
          rows={3}
          value={description}
          disabled={submitting}
          onChange={(event) => {
            setDescription(event.target.value);
          }}
        />
        <div className="flex flex-wrap items-center gap-(--space-3)">
          <ViewIconPicker value={logo} onChange={setLogo} disabled={submitting} />
          <LayoutPicker
            value={display.layout}
            onChange={(kind) => {
              setDisplay(mergeDisplayPrefs(display, { layout: kind }));
            }}
            disabled={submitting}
          />
          <DisplayControl
            workspaceSlug={workspaceSlug}
            entity={entity}
            offered={["list", "kanban", "calendar", "spreadsheet", "gantt_chart"]}
            hideCycle={hideCycle}
            hideModule={hideModule}
            display={display}
            onDisplayChange={setDisplay}
          />
        </div>
        {props.mode === "update" ? (
          <ViewAccessSummary access={props.view.access} />
        ) : (
          <ViewAccessField value={access} onChange={setAccess} disabled={submitting} />
        )}
        <section aria-label="View filters" className="flex flex-col gap-(--space-2)">
          <FilterSectionLoader
            key={filterRun}
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            entity={entity}
            hideCycle={hideCycle}
            hideModule={hideModule}
            registryRef={registryRef}
            {...(props.mode === "update"
              ? { expression: props.view.rich_filters }
              : props.prefill === undefined
                ? {}
                : { expression: props.prefill.expression })}
          />
        </section>
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
        <Button variant="primary" type="submit" disabled={submitting}>
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
function SaveViewDialog(props: SaveViewDialogProps): React.ReactElement {
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
    hideCycle: props.hideCycle,
    hideModule: props.hideModule,
    onSubmittingChange: setSubmitting,
  };

  return (
    <Dialog
      title={copy.title}
      description={copy.description}
      open={open}
      onOpenChange={(next, details) => {
        // A controlled dialog without a trigger only ever reports closing;
        // while a submit flies an outside press is swallowed and every
        // other dismissal (Escape, the footer) still closes.
        if (submitting && details.reason === "outside-press") return;
        onOpenChange(next);
      }}
      closeButton={false}
    >
      <IssuesLoaderProvider>
        {props.mode === "update" ? (
          <SaveViewForm key={session} {...shared} mode="update" view={props.view} />
        ) : (
          <SaveViewForm key={session} {...shared} mode="create" prefill={props.prefill} />
        )}
      </IssuesLoaderProvider>
    </Dialog>
  );
}

/**
 * Controlled create dialog; each visit starts from defaults, or from the
 * Save-as prefill when one is provided.
 */
export function CreateViewDialog(props: CreateViewDialogProps): React.ReactElement {
  return <SaveViewDialog mode="create" {...props} />;
}

/** Controlled edit dialog; each visit starts prefilled from the record. */
export function UpdateViewDialog(props: UpdateViewDialogProps): React.ReactElement {
  return <SaveViewDialog mode="update" {...props} />;
}
