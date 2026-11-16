// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace create/edit view dialog (parity/specs/views.md §10, VIEW-034,
// VIEW-035). One dialog for every entry point (the tab strip's add button,
// the detail header, the row and tab menus): title, description, the
// spreadsheet-fixed display control and an expanded workspace filter
// builder over a temporary instance. Create starts from defaults and
// navigates to the new view; edit opens prefilled from the record and
// adopts the PATCH echo. A failed submit keeps the dialog open with the
// input intact for retry — a failed edit in particular must surface its
// error notice, never stay silent (NEWFRONT-232).

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import {
  createWorkspaceView,
  updateWorkspaceView,
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
  loadRichFilterBuilder,
  type DisplayPrefs,
  type FieldConfigRegistry,
  type IssuesEntity,
  type RichFilterBuilderModule,
} from "../../issues/index.js";
import { viewKeys } from "../api/keys.js";
import { viewAccessLabel } from "../lib/projectViews.js";
import { workspaceCustomViewHref } from "../lib/workspaceViews.js";
import { failureDetail } from "./failureDetail.js";
import { FilterSectionLoader } from "./FilterSectionLoader.js";
import { readRegistryExpression } from "./registryExpression.js";
import { ViewAccessField, SHARED_ACCESS } from "./ViewAccessField.js";
import { validateViewTitle } from "./viewTitle.js";
import { defaultWorkspaceDisplay, prefillWorkspaceDisplay, serializeWorkspaceDisplay } from "./workspaceDisplay.js";

interface WorkspaceViewDialogBase {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
}

export type WorkspaceViewDialogProps =
  | (WorkspaceViewDialogBase & { mode: "create"; view?: undefined })
  | (WorkspaceViewDialogBase & { mode: "edit"; view: SavedView });

const CREATE_FAILURE_BODY = "Nothing was created — check the details and try again.";
const EDIT_FAILURE_BODY = "Nothing was saved — check the details and try again.";

/** Submit caption for the create mode, pending-aware. */
function submitCreateLabel(submitting: boolean): string {
  return submitting ? "Creating view…" : "Create View";
}

/** Submit caption for the edit mode, pending-aware. */
function submitEditLabel(submitting: boolean): string {
  return submitting ? "Updating view…" : "Update View";
}

interface WorkspaceViewFormProps extends WorkspaceViewDialogBase {
  onSubmittingChange: (submitting: boolean) => void;
}

type WorkspaceViewFormAllProps =
  | (WorkspaceViewFormProps & { mode: "create"; view?: undefined })
  | (WorkspaceViewFormProps & { mode: "edit"; view: SavedView });

function WorkspaceViewForm(props: WorkspaceViewFormAllProps): React.ReactElement {
  const { onOpenChange, workspaceSlug, onSubmittingChange } = props;
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [title, setTitle] = React.useState(props.mode === "edit" ? props.view.name : "");
  const [titleError, setTitleError] = React.useState<string | null>(null);
  const [description, setDescription] = React.useState(props.mode === "edit" ? props.view.description : "");
  const [display, setDisplay] = React.useState<DisplayPrefs>(() =>
    props.mode === "edit"
      ? prefillWorkspaceDisplay(props.view.display_filters, props.view.display_properties)
      : defaultWorkspaceDisplay()
  );
  const [access, setAccess] = React.useState<ViewAccessValue>(SHARED_ACCESS);
  const [submitting, setSubmitting] = React.useState(false);
  const titleRef = React.useRef<HTMLInputElement | null>(null);
  const registryRef = React.useRef<FieldConfigRegistry | null>(null);
  const entity = React.useMemo<IssuesEntity>(() => ({ scope: "workspace" }), []);

  // The title takes focus on every open; the form remounts per session and
  // the input is always mounted when this runs.
  React.useEffect(() => {
    titleRef.current!.focus();
  }, []);

  function setPending(next: boolean): void {
    setSubmitting(next);
    onSubmittingChange(next);
  }

  async function submitCreate(builder: RichFilterBuilderModule, expression: Record<string, unknown>): Promise<void> {
    const { displayFilters, displayProperties } = serializeWorkspaceDisplay(display);
    const payload: SavedViewCreate = builder.buildViewCreate(
      { expression, displayFilters, displayProperties },
      { name: title.trim(), description }
    );
    const created = await createWorkspaceView(getAppClient(), workspaceSlug, payload);
    queryClient.setQueryData(builder.savedViewKeys.workspace(workspaceSlug, created.id), created);
    queryClient.setQueryData<SavedViewList>(viewKeys.workspaceList(workspaceSlug), (current) =>
      current === undefined ? current : [created, ...current]
    );
    const slot = edition.slots?.viewAccess;
    if (slot !== undefined && access !== SHARED_ACCESS) {
      try {
        await slot.storeAccess({ workspaceSlug, projectId: null, viewId: created.id, access });
      } catch (storeError) {
        // The view exists; only the access choice failed to stick. The
        // user keeps the created view and learns the choice did not land
        // (a retry here would create a second view, so the dialog closes).
        notifyError(
          "View created, but the access choice could not be saved",
          failureDetail(storeError, CREATE_FAILURE_BODY)
        );
        onOpenChange(false);
        await navigate({ to: workspaceCustomViewHref(workspaceSlug, created.id) });
        return;
      }
    }
    notifySuccess("View created successfully");
    onOpenChange(false);
    await navigate({ to: workspaceCustomViewHref(workspaceSlug, created.id) });
  }

  async function submitEdit(
    builder: RichFilterBuilderModule,
    expression: Record<string, unknown>,
    view: SavedView
  ): Promise<void> {
    const { displayFilters, displayProperties } = serializeWorkspaceDisplay(display);
    const payload: SavedViewPatch = {
      name: title.trim(),
      description,
      rich_filters: expression,
      display_filters: displayFilters,
      display_properties: displayProperties,
    };
    const updated = await updateWorkspaceView(getAppClient(), workspaceSlug, view.id, payload);
    queryClient.setQueryData(builder.savedViewKeys.workspace(workspaceSlug, updated.id), updated);
    queryClient.setQueryData<SavedViewList>(viewKeys.workspaceList(workspaceSlug), (current) =>
      current === undefined ? current : current.map((row) => (row.id === updated.id ? updated : row))
    );
    notifySuccess("View updated successfully");
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
      // The builder module is usually already loaded (its section
      // rendered), so this reuses the cached request; a failed load
      // keeps the dialog open through the same failure path as a
      // failed write. The registry may still be null on an early
      // submit; that reads as the empty expression.
      const builder = await loadRichFilterBuilder();
      const expression = readRegistryExpression(registryRef.current, builder.serializeRichExpression);
      if (props.mode === "create") {
        await submitCreate(builder, expression);
      } else {
        await submitEdit(builder, expression, props.view);
      }
    } catch (error) {
      // Keep the dialog open with the input intact for retry. A failed
      // edit must toast: the old store swallowed the failure (NEWFRONT-232).
      notifyError(
        props.mode === "create" ? "View could not be created" : "View could not be updated",
        failureDetail(error, props.mode === "create" ? CREATE_FAILURE_BODY : EDIT_FAILURE_BODY)
      );
    } finally {
      setPending(false);
    }
  }

  const submitLabel = props.mode === "create" ? submitCreateLabel(submitting) : submitEditLabel(submitting);

  return (
    <form
      data-testid="workspace-view-form"
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
          <DisplayControl
            workspaceSlug={workspaceSlug}
            entity={entity}
            offered={["spreadsheet"]}
            hideCycle={true}
            hideModule={true}
            display={display}
            onDisplayChange={setDisplay}
          />
        </div>
        {props.mode === "create" ? (
          <ViewAccessField value={access} onChange={setAccess} disabled={submitting} />
        ) : (
          edition.slots?.viewAccess !== undefined && (
            <p data-testid="workspace-view-access-readonly" className="text-body text-(--text-muted)">
              Access: {viewAccessLabel(props.view.access)}
            </p>
          )
        )}
        <section aria-label="View filters" className="flex flex-col gap-(--space-2)">
          {/*
            A workspace view spans projects, so the temporary builder has
            no project: the empty id only feeds the row's project-role
            lookup (unused for a temporary instance) and switches the
            value pickers of project-scoped dimensions to their
            workspace lists. Cycle and module fields stay hidden: the
            workspace filter set has no project features to read them
            from.
          */}
          <FilterSectionLoader
            workspaceSlug={workspaceSlug}
            projectId=""
            entity={entity}
            hideCycle={true}
            hideModule={true}
            registryRef={registryRef}
            seedExpression={props.mode === "edit" ? props.view.rich_filters : undefined}
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
          {submitLabel}
        </Button>
      </div>
    </form>
  );
}

/**
 * Controlled workspace view dialog. The host owns `open`; the form remounts
 * on every open so each visit starts from defaults (create) or the record
 * (edit). While a submit is in flight an outside press does not close it;
 * Escape and Cancel still do.
 */
export function WorkspaceViewDialog(props: WorkspaceViewDialogProps): React.ReactElement {
  const { open, onOpenChange, mode } = props;
  const [session, setSession] = React.useState(0);
  const [submitting, setSubmitting] = React.useState(false);
  const wasOpen = React.useRef(open);
  React.useEffect(() => {
    if (open && !wasOpen.current) setSession((count) => count + 1);
    wasOpen.current = open;
  }, [open]);

  return (
    <Dialog
      title={mode === "create" ? "Create View" : "Update View"}
      description={
        mode === "create" ? "Name the view and add filters." : "Change the view's details, display and filters."
      }
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
        <WorkspaceViewForm key={session} {...props} onSubmittingChange={setSubmitting} />
      </IssuesLoaderProvider>
    </Dialog>
  );
}
