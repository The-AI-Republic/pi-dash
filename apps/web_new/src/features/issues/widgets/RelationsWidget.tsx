// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Relations widget (parity/specs/issues.md §15, ISS-179–183). Four kinds
// in fixed order; only non-empty kinds render, each group opens by
// default and carries its tone. Rows expose the row menu. Delete asks
// first, then detaches and destroys the related item.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight } from "lucide-react";
import * as React from "react";

import { Link } from "@tanstack/react-router";
import { Menu } from "@pidash/kit";
import { deleteEpic, deleteIssue, type RelatedIssue } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess, PeekPanel } from "../../../shared/shell/index.js";
import { DeleteWorkItemDialog } from "../../../shared/shell/contextual/index.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { issueKeys } from "../api/keys.js";
import { issueDetailQueryOptions } from "../api/queries.js";
import { AddRelationDialog } from "./AddRelationDialog.js";
import { RelationRow } from "./RelationRow.js";
import { useAddRelations, useIssueRelations, useRemoveRelation } from "./api.js";
import {
  RELATION_KIND_SPECS,
  RELATION_KINDS,
  relationCount,
  visibleRelationGroups,
  type RelationKind,
} from "./relationsModel.js";
import { WidgetAddButton, WidgetSection } from "./WidgetSection.js";

/** Group header tint per kind tone. */
const HEADER_TONE: Record<"neutral" | "warning" | "danger", string> = {
  neutral: "bg-(--subtle) text-(--text-muted)",
  warning: "bg-(--subtle) text-(--warning)",
  danger: "bg-(--subtle) text-(--danger)",
};

export interface RelationsWidgetProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Identifier of the current issue's project (row badges). */
  projectIdentifier: string;
  canEdit: boolean;
}

export function RelationsWidget({
  workspaceSlug,
  projectId,
  issueId,
  projectIdentifier,
  canEdit,
}: RelationsWidgetProps): React.ReactElement | null {
  const queryClient = useQueryClient();
  const relations = useIssueRelations(workspaceSlug, projectId, issueId);
  const projects = useQuery(projectsQueryOptions(getAppClient(), workspaceSlug));
  const add = useAddRelations({ workspaceSlug, projectId, issueId });
  const detach = useRemoveRelation({ workspaceSlug, projectId, issueId });
  const [addKind, setAddKind] = React.useState<RelationKind | null>(null);
  const [pendingDelete, setPendingDelete] = React.useState<RelatedIssue | null>(null);
  const [collapsed, setCollapsed] = React.useState<ReadonlySet<RelationKind>>(() => new Set());
  const [peek, setPeek] = React.useState<{ row: RelatedIssue; identifier: string } | null>(null);

  // The slim relation row carries no epic flag; the detail read decides
  // which collection the destroy belongs to (ISS-182).
  const destroy = useMutation({
    mutationFn: async (row: RelatedIssue) => {
      const client = getAppClient();
      const detail = await queryClient.fetchQuery(
        issueDetailQueryOptions(client, workspaceSlug, row.project_id, row.id)
      );
      if (detail.is_epic === true) await deleteEpic(client, workspaceSlug, row.project_id, row.id);
      else await deleteIssue(client, workspaceSlug, row.project_id, row.id);
    },
    onSuccess: () => notifySuccess("Work item deleted"),
    onError: (error: Error) => notifyError("Couldn't delete the work item", error.message),
    onSettled: () =>
      Promise.all([queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) })]).then(() => undefined),
  });

  const groups = visibleRelationGroups(relations.data);
  if (groups.length === 0 && !canEdit) return null;

  /** Identifier for a related row: its own project when the list resolved it. */
  function identifierOf(row: RelatedIssue): string {
    const known = projects.data?.find((project) => project.id === row.project_id)?.identifier;
    return known ?? projectIdentifier;
  }

  /**
   * Row activation (ISS-179): epics open in a new tab, other work items
   * open the peek panel. The slim relation row has no epic flag, so the
   * detail read decides; the panel prefetches it for the same reason.
   */
  function open(row: RelatedIssue): void {
    const identifier = identifierOf(row);
    const routeKey = `${identifier}-${row.sequence_id}`;
    void queryClient
      .fetchQuery(issueDetailQueryOptions(getAppClient(), workspaceSlug, row.project_id, row.id))
      .then((detail) => {
        if (detail.is_epic === true) {
          window.open(`/${workspaceSlug}/browse/${routeKey}`, "_blank", "noopener,noreferrer");
          return;
        }
        setPeek({ row, identifier });
      })
      .catch(() => {
        setPeek({ row, identifier });
      });
  }

  function toggleGroup(kind: RelationKind): void {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });
  }

  function removeRelation(row: RelatedIssue, kind: RelationKind): void {
    detach.mutate({ kind, relatedIssue: row.id });
  }

  function confirmDelete(): void {
    if (pendingDelete === null) return;
    destroy.mutate(pendingDelete);
    setPendingDelete(null);
  }

  return (
    <>
      <WidgetSection
        title="Relations"
        count={relationCount(relations.data)}
        add={
          canEdit ? (
            <Menu
              trigger={<WidgetAddButton label="Add relation" onClick={() => undefined} />}
              items={RELATION_KINDS.map((kind) => {
                const spec = RELATION_KIND_SPECS[kind];
                const Icon = spec.icon;
                return {
                  label: spec.label,
                  icon: <Icon size={14} strokeWidth={1.5} aria-hidden="true" />,
                  onSelect: () => setAddKind(kind),
                };
              })}
            />
          ) : undefined
        }
      >
        <div className="flex flex-col gap-(--space-2) p-(--space-1)">
          {groups.map(({ spec, rows }) => {
            const groupOpen = !collapsed.has(spec.kind);
            const GroupIcon = spec.icon;
            return (
              <div key={spec.kind} data-testid={`relation-group-${spec.kind}`}>
                <button
                  type="button"
                  aria-expanded={groupOpen}
                  onClick={() => toggleGroup(spec.kind)}
                  className={`flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left ${HEADER_TONE[spec.tone]}`}
                >
                  {groupOpen ? (
                    <ChevronDown size={12} strokeWidth={2} aria-hidden="true" />
                  ) : (
                    <ChevronRight size={12} strokeWidth={2} aria-hidden="true" />
                  )}
                  <GroupIcon size={14} strokeWidth={1.5} aria-hidden="true" />
                  <span className="text-caption font-medium">{spec.label}</span>
                  <span className="text-meta">{rows.length}</span>
                </button>
                {groupOpen ? (
                  <div className="flex flex-col">
                    {rows.map((row) => (
                      <RelationRow
                        key={`${spec.kind}-${row.id}`}
                        workspaceSlug={workspaceSlug}
                        projectIdentifier={identifierOf(row)}
                        row={row}
                        canEdit={canEdit}
                        onOpen={open}
                        onRemove={(target) => removeRelation(target, spec.kind)}
                        onDelete={setPendingDelete}
                      />
                    ))}
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      </WidgetSection>
      {canEdit && addKind !== null ? (
        <AddRelationDialog
          open
          onOpenChange={(open) => {
            if (!open) setAddKind(null);
          }}
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          issueId={issueId}
          kind={addKind}
          pending={add.isPending}
          onEmptySelection={() => notifyError("No work items selected", "Pick at least one work item to relate.")}
          onSubmit={(issueIds) => {
            add.mutate(
              { kind: addKind, issueIds },
              {
                onSuccess: () => {
                  setAddKind(null);
                },
              }
            );
          }}
        />
      ) : null}
      {peek === null ? null : (
        <RelationPeek
          workspaceSlug={workspaceSlug}
          projectId={peek.row.project_id}
          issueId={peek.row.id}
          identifier={`${peek.identifier}-${peek.row.sequence_id}`}
          title={peek.row.name}
          onClose={() => setPeek(null)}
        />
      )}
      <DeleteWorkItemDialog
        open={pendingDelete !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        noun="Work item"
        title={pendingDelete?.name ?? ""}
        deleting={destroy.isPending}
        onConfirm={confirmDelete}
      />
    </>
  );
}

/** Peek body: the related work item's detail summary and a full-page exit. */
function RelationPeek({
  workspaceSlug,
  projectId,
  issueId,
  identifier,
  title,
  onClose,
}: {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  identifier: string;
  title: string;
  onClose: () => void;
}): React.ReactElement {
  const detail = useQuery(issueDetailQueryOptions(getAppClient(), workspaceSlug, projectId, issueId));
  return (
    <PeekPanel eyebrow={identifier} title={detail.data?.name ?? title} titleLabel="Work item title" onClose={onClose}>
      {detail.isError ? (
        <p role="alert" className="text-body text-(--danger)">
          Couldn't load this work item.
        </p>
      ) : (
        <Link
          to="/$ws/browse/$issueKey"
          params={{ ws: workspaceSlug, issueKey: identifier }}
          className="text-body font-medium text-(--accent) hover:underline"
          onClick={() => {
            onClose();
          }}
        >
          Open the full work item
        </Link>
      )}
    </PeekPanel>
  );
}
