// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Worklog sidebar section (parity/specs/issues.md §16, ISS-206, cloud
// leg). The logged total plus one row per entry — duration, day, note,
// and the author for entries the viewer did not log. Authors manage
// their own entries (edit, remove) on live items; everyone else reads.
// A failed read offers a retry instead of an empty section.

import { Button, Dialog, IconButton, Skeleton } from "@pidash/kit";
import type { WorklogEntry } from "@pidash/api-client";
import { Pencil, X } from "lucide-react";
import * as React from "react";

import { useMe } from "../../../../../core/session/index.js";
import { formatMinutes } from "../../../../../shared/lib/formatDuration.js";
import { SidebarRow } from "../../sidebar/SidebarRow.js";
import { useDeleteWorklogEntry, useUpdateWorklogEntry, useWorklogEntries } from "./api.js";
import { canManageWorklogEntry } from "./eligibility.js";
import {
  WORKLOG_EMPTY,
  WORKLOG_LOAD_FAILED,
  WORKLOG_SECTION_TITLE,
  formatLoggedOn,
  totalWorklogMinutes,
  worklogActorLabel,
} from "./format.js";
import { WorklogDialog } from "./WorklogDialog.js";

export interface WorklogSidebarSectionProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Signed-in user, for authorship only; defaults to the session. */
  viewerId?: string | null;
  /** Archived items keep the totals and rows but drop every manage control. */
  archived: boolean;
}

function WorklogEntryRow({
  entry,
  viewerId,
  archived,
  removing,
  onEdit,
  onDelete,
}: {
  entry: WorklogEntry;
  viewerId: string | null;
  archived: boolean;
  removing: boolean;
  onEdit: () => void;
  onDelete: () => void;
}): React.ReactElement {
  const manageable = canManageWorklogEntry({ viewerId, entry, archived });
  const when = `${formatMinutes(entry.minutes)} on ${formatLoggedOn(entry.logged_on)}`;
  return (
    <div data-worklog-row="true" className="group/row flex items-start gap-(--space-2) py-(--space-1)">
      <div className="min-w-0 flex-1">
        <p className="text-body text-(--text)">
          {worklogActorLabel(entry, viewerId)}
          <span className="font-medium">{formatMinutes(entry.minutes)}</span>
          <span className="text-(--text-muted)"> · {formatLoggedOn(entry.logged_on)}</span>
        </p>
        {entry.note === "" ? null : <p className="text-meta truncate text-(--text-muted)">{entry.note}</p>}
      </div>
      {manageable ? (
        <span className="flex shrink-0 opacity-0 group-hover/row:opacity-100 focus-within:opacity-100">
          <IconButton label={`Edit ${when}`} size="small" onClick={onEdit}>
            <Pencil size={14} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
          <IconButton label={`Remove ${when}`} size="small" onClick={onDelete} disabled={removing}>
            <X size={14} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </span>
      ) : null}
    </div>
  );
}

export function WorklogSidebarSection({
  workspaceSlug,
  projectId,
  issueId,
  viewerId: viewerOverride,
  archived,
}: WorklogSidebarSectionProps): React.ReactElement {
  const me = useMe();
  const viewerId = viewerOverride !== undefined ? viewerOverride : (me.data?.id ?? null);
  const entries = useWorklogEntries(workspaceSlug, projectId, issueId);
  const update = useUpdateWorklogEntry(workspaceSlug, projectId, issueId);
  const remove = useDeleteWorklogEntry(workspaceSlug, projectId, issueId);
  const [editing, setEditing] = React.useState<WorklogEntry | null>(null);
  const [confirmingDelete, setConfirmingDelete] = React.useState<WorklogEntry | null>(null);

  if (entries.data === undefined) {
    if (entries.isError) {
      return (
        <div data-worklog-section="true" className="flex flex-col items-start gap-(--space-2)" role="alert">
          <p className="text-meta text-(--text-muted)">{WORKLOG_LOAD_FAILED}</p>
          <Button variant="secondary" size="small" onClick={() => void entries.refetch()}>
            Try again
          </Button>
        </div>
      );
    }
    return (
      <div data-worklog-section="true" role="status" aria-label="Loading worklog">
        <Skeleton height="20px" />
      </div>
    );
  }

  return (
    <div data-worklog-section="true" className="flex flex-col">
      <SidebarRow label={WORKLOG_SECTION_TITLE}>
        {entries.data.length === 0 ? (
          <span className="text-meta text-(--text-muted)">{WORKLOG_EMPTY}</span>
        ) : (
          <span data-worklog-total="true" className="text-body font-medium text-(--text)">
            {formatMinutes(totalWorklogMinutes(entries.data))}
          </span>
        )}
      </SidebarRow>
      {entries.data.map((entry) => (
        <WorklogEntryRow
          key={entry.id}
          entry={entry}
          viewerId={viewerId}
          archived={archived}
          removing={remove.isPending}
          onEdit={() => {
            setEditing(entry);
          }}
          onDelete={() => {
            setConfirmingDelete(entry);
          }}
        />
      ))}
      {editing === null ? null : (
        <WorklogDialog
          key={editing.id}
          initial={{ minutes: editing.minutes, logged_on: editing.logged_on, note: editing.note }}
          saving={update.isPending}
          onClose={() => {
            setEditing(null);
          }}
          onSubmit={(input) => {
            update.mutate(
              { entryId: editing.id, patch: input },
              {
                onSuccess: () => {
                  setEditing(null);
                },
              }
            );
          }}
        />
      )}
      {confirmingDelete === null ? null : (
        <WorklogDeleteDialog
          entry={confirmingDelete}
          busy={remove.isPending}
          onCancel={() => {
            setConfirmingDelete(null);
          }}
          onConfirm={() => {
            remove.mutate(confirmingDelete.id, {
              onSuccess: () => {
                setConfirmingDelete(null);
              },
            });
          }}
        />
      )}
    </div>
  );
}

function WorklogDeleteDialog({
  entry,
  busy,
  onCancel,
  onConfirm,
}: {
  entry: WorklogEntry;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}): React.ReactElement {
  return (
    <Dialog
      title={`Remove ${formatMinutes(entry.minutes)} on ${formatLoggedOn(entry.logged_on)}?`}
      open
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
      actions={
        <>
          <Button onClick={onCancel}>Cancel</Button>
          <Button variant="danger" loading={busy} onClick={onConfirm}>
            Remove
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text)">This time entry will be permanently removed from this work item.</p>
    </Dialog>
  );
}
