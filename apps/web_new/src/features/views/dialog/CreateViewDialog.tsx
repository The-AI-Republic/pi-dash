// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project create-view dialog (parity/specs/views.md §5, VIEW-003,
// VIEW-013, VIEW-014, VIEW-019). One dialog for every entry point (the
// list header, the zero-views action, the palette command): title,
// description, icon, layout, display options and an expanded project
// filter builder over a temporary instance. Opening resets the form to
// defaults. Success closes, toasts and navigates to the new view; a
// failed submit keeps the dialog open with the input intact for retry
// (NEWFRONT-221's reset is the old bug, not the target).

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { createProjectView, type SavedViewCreate } from "@pidash/api-client";
import { Button, Dialog, Input, Textarea } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { edition, type ViewAccessValue } from "../../../core/edition/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  buildViewCreate,
  DisplayControl,
  FieldConfigRegistry,
  IssuesLoaderProvider,
  LayoutPicker,
  loadRichFilterBuilder,
  mergeDisplayPrefs,
  savedViewKeys,
  type DisplayPrefs,
  type IssuesEntity,
} from "../../issues/index.js";
import { defaultCreateDisplay, normalizeCreateDisplay, serializeCreateDisplay } from "./createDisplay.js";
import { FilterSectionLoader } from "./FilterSectionLoader.js";
import { readRegistryExpression } from "./registryExpression.js";
import { ViewAccessField, SHARED_ACCESS } from "./ViewAccessField.js";
import { ViewIconPicker, type ViewLogoProps } from "./ViewIconPicker.js";
import { validateViewTitle } from "./viewTitle.js";

export interface CreateViewDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  projectId: string;
  /** Cycle/module entries hide while their project features are off. */
  hideCycle: boolean;
  hideModule: boolean;
}

/** Server detail when the body carries one; the fixed copy otherwise. */
function failureDetail(error: unknown): string {
  const body = (error as { body?: unknown }).body ?? null;
  if (typeof body === "object" && body !== null && !Array.isArray(body)) {
    const detail = (body as Record<string, unknown>)["detail"];
    if (typeof detail === "string" && detail.length > 0) return detail;
  }
  return "Nothing was created — check the details and try again.";
}

interface CreateViewFormProps extends CreateViewDialogProps {
  onSubmittingChange: (submitting: boolean) => void;
}

function CreateViewForm({
  onOpenChange,
  workspaceSlug,
  projectId,
  hideCycle,
  hideModule,
  onSubmittingChange,
}: CreateViewFormProps): React.ReactElement {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [title, setTitle] = React.useState("");
  const [titleError, setTitleError] = React.useState<string | null>(null);
  const [description, setDescription] = React.useState("");
  const [logo, setLogo] = React.useState<ViewLogoProps | null>(null);
  const [display, setDisplay] = React.useState<DisplayPrefs>(defaultCreateDisplay);
  const [access, setAccess] = React.useState<ViewAccessValue>(SHARED_ACCESS);
  const [submitting, setSubmitting] = React.useState(false);
  const titleRef = React.useRef<HTMLInputElement | null>(null);
  const registry = React.useMemo(() => new FieldConfigRegistry(), []);
  const entity = React.useMemo<IssuesEntity>(() => ({ scope: "project", projectId }), [projectId]);

  // The title takes focus on every open; the form remounts per session and
  // the input is always mounted when this runs.
  React.useEffect(() => {
    titleRef.current!.focus();
  }, []);

  function setPending(next: boolean): void {
    setSubmitting(next);
    onSubmittingChange(next);
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
      // The builder module is already loaded (its section rendered), so
      // this reuses the cached request; a failed load keeps the dialog
      // open through the same failure path as a failed write.
      const { serializeRichExpression } = await loadRichFilterBuilder();
      const expression = readRegistryExpression(registry, serializeRichExpression);
      const settings = normalizeCreateDisplay(display);
      const { displayFilters, displayProperties } = serializeCreateDisplay(settings);
      const payload: SavedViewCreate = {
        ...buildViewCreate({ expression, displayFilters, displayProperties }, { name: title.trim(), description }),
        ...(logo === null ? {} : { logo_props: logo }),
      };
      const created = await createProjectView(getAppClient(), workspaceSlug, projectId, payload);
      queryClient.setQueryData(savedViewKeys.project(workspaceSlug, projectId, created.id), created);
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
    } catch (error) {
      // Keep the dialog open with the input intact for retry (VIEW-014).
      notifyError("Failed to create view", failureDetail(error));
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
        <ViewAccessField value={access} onChange={setAccess} disabled={submitting} />
        <section aria-label="View filters" className="flex flex-col gap-(--space-2)">
          <FilterSectionLoader
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            entity={entity}
            hideCycle={hideCycle}
            hideModule={hideModule}
            registry={registry}
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
          {submitting ? "Creating view…" : "Create View"}
        </Button>
      </div>
    </form>
  );
}

/**
 * Controlled create dialog. The host owns `open`; the form remounts on
 * every open so each visit starts from defaults. While a submit is in
 * flight an outside press does not close it; Escape and Cancel still do.
 */
export function CreateViewDialog(props: CreateViewDialogProps): React.ReactElement {
  const { open, onOpenChange } = props;
  const [session, setSession] = React.useState(0);
  const [submitting, setSubmitting] = React.useState(false);
  const wasOpen = React.useRef(open);
  React.useEffect(() => {
    if (open && !wasOpen.current) setSession((count) => count + 1);
    wasOpen.current = open;
  }, [open]);

  return (
    <Dialog
      title="Create View"
      description="Name the view, pick its layout, and add filters."
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
        <CreateViewForm key={session} {...props} onSubmittingChange={setSubmitting} />
      </IssuesLoaderProvider>
    </Dialog>
  );
}
