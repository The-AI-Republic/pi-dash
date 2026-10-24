// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates settings surface (parity/specs/project-settings-part-f.md
// §Estimates entry, PSET-049; inventory EST-001–007, EST-016). The screen
// is the shared component the settings entry embeds: a loader while the
// list resolves, an illustrated empty state with a create action when the
// project has no system, otherwise the enable switch, the active system
// row (delete only in this build) and the read-only superseded section.
// Row owners are project admins; the page gate around the screen refuses
// everyone else.

import { useQueryClient } from "@tanstack/react-query";
import type { Estimate } from "@pidash/api-client";
import { Button, Dialog, EmptyState, IconButton, Skeleton, Switch } from "@pidash/kit";
import { Pencil, Trash2 } from "lucide-react";
import * as React from "react";

import { authKeys, useProjectRow } from "../auth/index.js";
import { edition } from "../../core/edition/index.js";
import { useDeleteEstimate, useProjectEstimates, useToggleEstimates } from "./api.js";
import { EstimateCreateDialog } from "./EstimateCreateDialog.js";
import { EstimateUpdateDialog } from "./EstimateUpdateDialog.js";
import { estimateSavedEditEnabled, selectActiveSystem, selectSupersededSystems, systemValueLabels } from "./model.js";

export const ESTIMATES_HEADING = "Estimates";
export const ESTIMATES_DESCRIPTION = "Define how your team measures effort so work items read consistently.";
export const ESTIMATES_ENABLE_TITLE = "Enable estimates for this project";
export const ESTIMATES_ENABLE_DESCRIPTION = "Work items can carry one value from the active system.";
export const ESTIMATES_ACTIVE_HEADING = "Estimates list";
export const ESTIMATES_ARCHIVED_HEADING = "Archived estimates";
export const ESTIMATES_ARCHIVED_DESCRIPTION =
  "Estimates have gone through a change: these are the systems your older versions used. They stay readable so work items that carry their values keep their meaning.";
/** Where the archived explainer points; it must leave the app. */
export const ESTIMATES_ARCHIVED_REFERENCE_HREF = "https://github.com/The-AI-Republic/pi-dash#readme";
export const ESTIMATES_ARCHIVED_REFERENCE_LABEL = "Learn more about estimates";
export const ESTIMATES_LOAD_ERROR = "Estimates could not be loaded.";
export const ESTIMATES_RETRY_LABEL = "Try again";
export const ESTIMATES_EMPTY_TITLE = "No estimates yet";
export const ESTIMATES_EMPTY_DESCRIPTION = "Create a measurement system to start tracking effort on work items.";
export const ESTIMATES_ADD_LABEL = "Add estimate system";
export const ESTIMATE_DELETE_DIALOG_TITLE = "Delete estimate system";
export const ESTIMATE_DELETE_DIALOG_BODY =
  "This permanently removes the system and strips its value from every work item. This cannot be undone.";

export interface EstimatesScreenProps {
  workspaceSlug: string;
  projectId: string;
  /** Row and switch actions render only with project-admin standing. */
  canManage: boolean;
}

function SystemRow({
  system,
  manage,
  onDelete,
  onEdit,
}: {
  system: Estimate;
  manage: boolean;
  onDelete: ((system: Estimate) => void) | undefined;
  onEdit?: ((system: Estimate) => void) | undefined;
}): React.ReactElement {
  // Delete stays the row's first control so the one-button OSS row and
  // the parity driver's row-delete click keep working where the edition
  // adds the edit control beside it.
  const controls =
    manage && onDelete !== undefined ? (
      <div className="flex items-center gap-(--space-2)">
        <IconButton label={`Delete ${system.name}`} variant="ghost" onClick={() => onDelete(system)}>
          <Trash2 size={14} aria-hidden="true" />
        </IconButton>
        {onEdit === undefined ? null : (
          <IconButton label={`Edit ${system.name}`} variant="ghost" onClick={() => onEdit(system)}>
            <Pencil size={14} aria-hidden="true" />
          </IconButton>
        )}
      </div>
    ) : null;
  return (
    <div
      className="flex items-start justify-between gap-(--space-4) rounded-(--radius-control) border border-(--border) p-(--space-4)"
      data-testid="estimate-system-row"
      data-system-id={system.id}
    >
      <div className="flex min-w-0 flex-col gap-(--space-1)">
        <h4 className="text-emphasis truncate text-(--text)">{system.name}</h4>
        <p className="text-body text-(--text-muted)">{systemValueLabels(system).join(", ")}</p>
      </div>
      {controls}
    </div>
  );
}

function EstimatesLoader(): React.ReactElement {
  return (
    <div role="status" aria-label="Loading estimates" className="flex flex-col gap-(--space-4)">
      <Skeleton height="40px" />
      <Skeleton height="40px" />
      <Skeleton height="40px" />
      <Skeleton height="40px" />
    </div>
  );
}

/**
 * Failed systems read (EST-002): a visible error with a retry action,
 * rendered instead of the empty state or content so a failure never
 * reads as "no systems".
 */
function EstimatesLoadError({ onRetry, retrying }: { onRetry: () => void; retrying: boolean }): React.ReactElement {
  return (
    <div role="alert" className="flex flex-col items-start gap-(--space-3)">
      <p className="text-body text-(--danger)">{ESTIMATES_LOAD_ERROR}</p>
      <Button variant="secondary" loading={retrying} onClick={onRetry}>
        {ESTIMATES_RETRY_LABEL}
      </Button>
    </div>
  );
}

export function EstimatesScreen({ workspaceSlug, projectId, canManage }: EstimatesScreenProps): React.ReactElement {
  const queryClient = useQueryClient();
  const systems = useProjectEstimates(workspaceSlug, projectId);
  const project = useProjectRow(workspaceSlug, projectId);
  const remove = useDeleteEstimate(workspaceSlug, projectId);
  const toggle = useToggleEstimates(workspaceSlug, projectId);
  const [creating, setCreating] = React.useState(false);
  const [deleting, setDeleting] = React.useState<Estimate | null>(null);
  const [editing, setEditing] = React.useState<Estimate | null>(null);

  if (systems.isPending) return <EstimatesLoader />;
  if (systems.isError) {
    return <EstimatesLoadError onRetry={() => void systems.refetch()} retrying={systems.isFetching} />;
  }

  const list = systems.data ?? [];
  const active = selectActiveSystem(list);
  const superseded = selectSupersededSystems(list);
  // The switch reads the project's stored reference, not list position.
  const enabled = project.data?.estimate != null;
  // Saved-system editing is a cloud capability: without the edition flag
  // the row stays delete-only and the update dialog is never mounted.
  const savedEdit = estimateSavedEditEnabled(edition.flags);

  return (
    <section aria-label="Estimates settings" className="flex flex-col gap-(--space-8)" data-testid="estimates-screen">
      <div className="flex flex-col gap-(--space-1)">
        <h2 className="text-h3 text-(--text)">{ESTIMATES_HEADING}</h2>
        <p className="text-body text-(--text-muted)">{ESTIMATES_DESCRIPTION}</p>
      </div>

      {active === undefined ? (
        <EmptyState
          variant="detailed"
          title={ESTIMATES_EMPTY_TITLE}
          description={ESTIMATES_EMPTY_DESCRIPTION}
          art={{ category: "general", type: "empty" }}
          {...(canManage ? { primaryAction: { label: ESTIMATES_ADD_LABEL, onSelect: () => setCreating(true) } } : {})}
        />
      ) : (
        <div className="flex flex-col gap-(--space-8)">
          <div className="flex items-center justify-between gap-(--space-4) rounded-(--radius-control) border border-(--border) p-(--space-4)">
            <div className="flex min-w-0 flex-col gap-(--space-1)">
              <p className="text-emphasis text-(--text)">{ESTIMATES_ENABLE_TITLE}</p>
              <p className="text-body text-(--text-muted)">{ESTIMATES_ENABLE_DESCRIPTION}</p>
            </div>
            <Switch
              label={ESTIMATES_ENABLE_TITLE}
              checked={enabled}
              disabled={!canManage || toggle.isPending}
              onCheckedChange={(next) =>
                toggle.mutate(
                  { enabled: next, activeId: active.id },
                  {
                    onError: () => {
                      // The stored reference is unchanged on failure; keep
                      // the row read fresh so the switch snaps back.
                      void queryClient.invalidateQueries({
                        queryKey: authKeys.project(workspaceSlug, projectId),
                      });
                    },
                  }
                )
              }
            />
          </div>

          <div className="flex flex-col gap-(--space-4)" data-testid="estimates-active-section">
            <h3 className="text-h4 text-(--text)">{ESTIMATES_ACTIVE_HEADING}</h3>
            <SystemRow
              system={active}
              manage={canManage}
              onDelete={(system) => setDeleting(system)}
              {...(canManage && savedEdit ? { onEdit: (system: Estimate) => setEditing(system) } : {})}
            />
          </div>
        </div>
      )}

      {superseded.length > 0 ? (
        <div className="flex flex-col gap-(--space-4)" data-testid="estimates-archived-section">
          <div className="flex flex-col gap-(--space-1)">
            <h3 className="text-h4 text-(--text)">{ESTIMATES_ARCHIVED_HEADING}</h3>
            <p className="text-body text-(--text-muted)">
              {ESTIMATES_ARCHIVED_DESCRIPTION}{" "}
              <a
                href={ESTIMATES_ARCHIVED_REFERENCE_HREF}
                target="_blank"
                rel="noreferrer"
                className="font-medium text-(--accent) hover:underline"
                data-testid="estimates-archived-reference"
              >
                {ESTIMATES_ARCHIVED_REFERENCE_LABEL}
              </a>
            </p>
          </div>
          {superseded.map((system) => (
            <SystemRow key={system.id} system={system} manage={false} onDelete={undefined} />
          ))}
        </div>
      ) : null}

      <EstimateCreateDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        open={creating}
        onOpenChange={setCreating}
        onCreated={() => {
          void queryClient.invalidateQueries({ queryKey: authKeys.project(workspaceSlug, projectId) });
        }}
      />

      {savedEdit && editing !== null ? (
        <EstimateUpdateDialog
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          system={editing}
          onClose={() => setEditing(null)}
        />
      ) : null}

      <Dialog
        open={deleting !== null}
        onOpenChange={(next) => {
          // Dismissal runs only while the delete is idle: the dialog is
          // non-dismissable during the write.
          if (!next) setDeleting(null);
        }}
        title={ESTIMATE_DELETE_DIALOG_TITLE}
        description={ESTIMATE_DELETE_DIALOG_BODY}
        nonDismissable={remove.isPending}
        {...(deleting === null
          ? {}
          : {
              actions: (
                <>
                  <Button variant="secondary" disabled={remove.isPending} onClick={() => setDeleting(null)}>
                    Cancel
                  </Button>
                  <Button
                    variant="danger"
                    loading={remove.isPending}
                    disabled={remove.isPending}
                    onClick={() => {
                      const target = deleting;
                      // The unlink follows the stored reference, not the
                      // row: deleting the active system while the project
                      // is unlinked needs no project write (EST-016).
                      remove.mutate(
                        { estimate: target, active: target.id === active?.id && enabled },
                        { onSuccess: () => setDeleting(null) }
                      );
                    }}
                  >
                    Delete
                  </Button>
                </>
              ),
            })}
      >
        <p className="text-body text-(--text)">
          Delete <strong>{deleting?.name ?? ""}</strong>?
        </p>
      </Dialog>
    </section>
  );
}
