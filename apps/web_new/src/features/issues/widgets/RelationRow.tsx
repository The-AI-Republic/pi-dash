// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One relation row (parity/specs/issues.md §15, ISS-179/182). Identity,
// the three inline property pickers and the row menu. A property write
// patches the related issue directly and refreshes both the row read and
// the detail header counts.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, Ellipsis, Pencil, Trash2, Unlink } from "lucide-react";
import * as React from "react";

import { IconButton, Menu } from "@pidash/kit";
import { updateEpic, updateIssue, type RelatedIssue } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { MemberPicker, PriorityPicker, StatePicker } from "../../../shared/pickers/index.js";
import { issueKeys } from "../api/keys.js";
import { issueDetailQueryOptions } from "../api/queries.js";
import { stateOptionGate } from "../edition/workflow.js";

export interface RelationRowProps {
  workspaceSlug: string;
  /** Identifier shown in the badge; the read carries only project+seq. */
  projectIdentifier: string;
  row: RelatedIssue;
  canEdit: boolean;
  onOpen: (row: RelatedIssue) => void;
  /** Opens the shared create/update modal for the related work item. */
  onEdit: (row: RelatedIssue) => void;
  onRemove: (row: RelatedIssue) => void;
  onDelete: (row: RelatedIssue) => void;
}

/** Resolve a site-relative address against the current origin. */
export function absoluteUrl(href: string): string {
  return new URL(href, window.location.origin).toString();
}

/** Copy one work item's absolute browse URL for the clipboard. */
export async function copyWorkItemUrl(href: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(absoluteUrl(href));
    notifySuccess("Link copied");
  } catch (error) {
    notifyError("Couldn't copy the link", (error as Error).message);
  }
}

export function RelationRow({
  workspaceSlug,
  projectIdentifier,
  row,
  canEdit,
  onOpen,
  onEdit,
  onRemove,
  onDelete,
}: RelationRowProps): React.ReactElement {
  const queryClient = useQueryClient();
  const identifier = `${projectIdentifier}-${row.sequence_id}`;

  // The slim row carries no kind, so the detail read decides which
  // collection the property write belongs to (ISS-179). The old app
  // swaps the epics service for an epic row.
  const update = useMutation({
    mutationFn: async (patch: Record<string, unknown>) => {
      const client = getAppClient();
      const detail = await queryClient.fetchQuery(
        issueDetailQueryOptions(client, workspaceSlug, row.project_id, row.id)
      );
      if (detail.is_epic === true) await updateEpic(client, workspaceSlug, row.project_id, row.id, patch);
      else await updateIssue(client, workspaceSlug, row.project_id, row.id, patch);
    },
    onError: (error: Error) => notifyError("Couldn't update the related work item", error.message),
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: issueKeys.widgetRelations(workspaceSlug, row.id) }),
        queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) }),
      ]).then(() => undefined),
  });

  const href = `/${workspaceSlug}/browse/${identifier}`;

  return (
    <div
      data-testid={`relation-row-${row.id}`}
      className="group flex min-h-11 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle)"
    >
      <button
        type="button"
        onClick={() => onOpen(row)}
        className="flex min-w-0 flex-1 items-center gap-(--space-3) text-left"
      >
        <span className="text-meta shrink-0 rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-px font-medium text-(--text-muted)">
          {identifier}
        </span>
        <span className="text-body min-w-0 flex-1 truncate text-(--text)">{row.name}</span>
      </button>
      <div className="flex shrink-0 items-center gap-(--space-1)" onClick={(event) => event.stopPropagation()}>
        <StatePicker
          workspaceSlug={workspaceSlug}
          projectId={row.project_id}
          value={row.state_id}
          variant="ghost"
          disabled={!canEdit}
          optionGate={stateOptionGate(row.project_id, row.state_id)}
          onChange={(stateId) => update.mutate({ state_id: stateId })}
        />
        <PriorityPicker
          value={row.priority}
          variant="ghost"
          disabled={!canEdit}
          onChange={(priority) => update.mutate({ priority })}
        />
        <MemberPicker
          workspaceSlug={workspaceSlug}
          projectId={row.project_id}
          multiple
          value={row.assignee_ids}
          variant="ghost"
          disabled={!canEdit}
          placeholder="Assignees"
          onChange={(assigneeIds) => update.mutate({ assignee_ids: assigneeIds })}
        />
      </div>
      <Menu
        trigger={
          <IconButton label={`Actions for ${row.name}`} size="small" variant="ghost">
            <Ellipsis size={14} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={[
          ...(canEdit
            ? [{ label: "Edit", icon: <Pencil size={14} aria-hidden="true" />, onSelect: () => onEdit(row) }]
            : []),
          {
            label: "Copy link",
            icon: <Copy size={14} aria-hidden="true" />,
            onSelect: () => void copyWorkItemUrl(href),
          },
          ...(canEdit
            ? [
                {
                  label: "Remove relation",
                  icon: <Unlink size={14} aria-hidden="true" />,
                  onSelect: () => onRemove(row),
                },
                {
                  label: "Delete",
                  icon: <Trash2 size={14} aria-hidden="true" />,
                  onSelect: () => onDelete(row),
                },
              ]
            : []),
        ]}
      />
    </div>
  );
}
