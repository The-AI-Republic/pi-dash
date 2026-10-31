// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Right side of the rich-filter row (parity/specs/issues.md §9, ISS-104):
// clear all, save the current expression as a new view, save a copy from
// a dirty saved view, and update the view in place. The create dialog
// asks for a name only and carries the current expression with the
// display filters and properties; a delegated host opens its own dialog
// with the same snapshot instead (parity/specs/views.md §8, VIEW-048).
// The update is offered to the owner of an unlocked, changed, non-default
// view and patches the same three keys.

import {
  createProjectView,
  createWorkspaceView,
  updateProjectView,
  updateWorkspaceView,
  type SavedView,
} from "@pidash/api-client";
import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { useMe } from "../../../../core/session/hooks.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import {
  buildViewCreate,
  buildViewPatch,
  entityViewContext,
  isExpressionDirty,
  offersSaveAs,
  offersSaveView,
  offersUpdateView,
  type ViewContext,
  type ViewSnapshot,
} from "./views.js";

const SAVE_FAILED_TITLE = "Couldn't save the view";
const SAVE_FAILED_BODY = "Nothing was saved — check your connection and try again.";
const UPDATE_FAILED_TITLE = "Couldn't update the view";
const UPDATE_FAILED_BODY = "The view is unchanged — check your connection and try again.";

export interface ViewActionsProps {
  workspaceSlug: string;
  projectId: string;
  /** Live expression and display settings the actions would save. */
  snapshot: ViewSnapshot;
  /** Which record the row edits; a bare entity list passes no context. */
  context?: ViewContext | undefined;
  /** Member or admin: only they may create or update views. */
  canEdit: boolean;
  /** Project view saving waits for the project's views feature. */
  viewsEnabled: boolean;
  /** Signed-in user, for the ownership check. */
  meId: string | null;
  /** Drop every condition; the host persists the empty expression. */
  onClearAll: () => void;
  /** Whether any condition is present (Clear all only offers then). */
  hasConditions: boolean;
  /** Called after a create with the stored record, so the host can open it. */
  onCreated?: ((view: SavedView) => void) | undefined;
  /** Called after a successful update, so the host can adopt stored values. */
  onUpdated?: ((view: SavedView) => void) | undefined;
  /**
   * Views-owned save dialog (parity/specs/views.md §8, VIEW-048). When the
   * host provides it, the Save view/Save as entry hands it the live
   * snapshot instead of opening the minimal form; the host's dialog posts
   * the same payload contract (buildViewCreate's keys). Absent on issues
   * surfaces, which keep the minimal form.
   */
  onSavePress?: ((snapshot: ViewSnapshot) => void) | undefined;
}

/** Server detail when the body carries one; the fixed copy otherwise. */
function failureBody(error: unknown, fallback: string): string {
  const body = (error as { body?: unknown }).body ?? null;
  if (typeof body === "object" && body !== null && !Array.isArray(body)) {
    const detail = (body as Record<string, unknown>)["detail"];
    if (typeof detail === "string" && detail.length > 0) return detail;
  }
  return fallback;
}

export function ViewActions({
  workspaceSlug,
  projectId,
  snapshot,
  context = entityViewContext(),
  canEdit,
  viewsEnabled,
  meId,
  onClearAll,
  hasConditions,
  onCreated,
  onUpdated,
  onSavePress,
}: ViewActionsProps): React.ReactElement | null {
  // The builder passes the id it already read; a surface mounting the row
  // without one falls back to the session read.
  const session = useMe();
  const viewerId = meId ?? session.data?.id ?? null;
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [name, setName] = React.useState("");
  const [saving, setSaving] = React.useState(false);
  const [updating, setUpdating] = React.useState(false);

  // Saving needs something to save. A bare list needs live conditions;
  // Save as additionally needs a dirty view, and forkable content: live
  // conditions, or the carried expression when only the display changed.
  // An emptied saved view offers only Update view, which is how the
  // cleared expression reaches its record.
  const saveOffered =
    canEdit &&
    viewsEnabled &&
    ((offersSaveView(context) && hasConditions) ||
      (offersSaveAs(context, snapshot) && (hasConditions || !isExpressionDirty(context, snapshot))));
  const updateOffered = canEdit && viewsEnabled && offersUpdateView(context, snapshot, viewerId);
  // offersUpdateView only offers a view record, so the id is present whenever
  // the control renders.
  const updateViewId = updateOffered ? context.viewId : null;
  if (!hasConditions && !saveOffered && !updateOffered) return null;

  function openDialog(): void {
    setName("");
    setDialogOpen(true);
  }

  function pressSave(): void {
    // A delegated host opens its own dialog with the live snapshot; the
    // minimal form stays for issues surfaces without a delegate.
    if (onSavePress !== undefined) {
      onSavePress(snapshot);
      return;
    }
    openDialog();
  }

  async function create(): Promise<void> {
    // The dialog keeps Create View disabled until a name exists and while a
    // save is in flight, so only a named, single submit reaches the write.
    const trimmed = name.trim();
    setSaving(true);
    setDialogOpen(true);
    try {
      const payload = buildViewCreate(snapshot, { name: trimmed });
      const created =
        context.isView && context.viewScope === "workspace"
          ? await createWorkspaceView(getAppClient(), workspaceSlug, payload)
          : await createProjectView(getAppClient(), workspaceSlug, projectId, payload);
      notifySuccess("View created successfully");
      setDialogOpen(false);
      onCreated?.(created);
    } catch (error) {
      notifyError(SAVE_FAILED_TITLE, failureBody(error, SAVE_FAILED_BODY));
    } finally {
      setSaving(false);
    }
  }

  async function update(viewId: string): Promise<void> {
    setUpdating(true);
    try {
      const patch = buildViewPatch(snapshot);
      const stored =
        context.viewScope === "workspace"
          ? await updateWorkspaceView(getAppClient(), workspaceSlug, viewId, patch)
          : await updateProjectView(getAppClient(), workspaceSlug, projectId, viewId, patch);
      notifySuccess("View updated successfully");
      onUpdated?.(stored);
    } catch (error) {
      notifyError(UPDATE_FAILED_TITLE, failureBody(error, UPDATE_FAILED_BODY));
    } finally {
      setUpdating(false);
    }
  }

  return (
    <span className="ml-auto inline-flex items-center gap-(--space-2)" data-testid="rich-view-actions">
      {hasConditions ? (
        <Button variant="ghost" size="small" onClick={onClearAll} data-testid="rich-clear-all">
          Clear all
        </Button>
      ) : null}
      {saveOffered ? (
        <Button variant="ghost" size="small" onClick={pressSave} data-testid="rich-save-view">
          {context.isView ? "Save as" : "Save view"}
        </Button>
      ) : null}
      {updateOffered && updateViewId !== null ? (
        <Button variant="secondary" size="small" onClick={() => void update(updateViewId)} disabled={updating}>
          Update view
        </Button>
      ) : null}
      <Dialog
        open={dialogOpen}
        onOpenChange={(open) => {
          setDialogOpen(open);
        }}
        title="Create view"
        description="The current filters and display options become a saved view."
        actions={
          <>
            <Button
              variant="ghost"
              size="small"
              onClick={() => {
                setDialogOpen(false);
              }}
            >
              Cancel
            </Button>
            <Button
              variant="primary"
              size="small"
              onClick={() => void create()}
              disabled={saving || name.trim() === ""}
            >
              Create View
            </Button>
          </>
        }
      >
        <label className="flex flex-col gap-(--space-2)">
          <span className="text-body text-(--text-muted)">Name</span>
          <Input
            label="View name"
            placeholder="Title"
            value={name}
            onChange={(event) => {
              setName(event.target.value);
            }}
          />
        </label>
      </Dialog>
    </span>
  );
}
