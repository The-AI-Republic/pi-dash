// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work-item detail (parity/specs/archives.md §3.4–3.5,
// ARCH-011–012). The archived address renders inside the archives frame
// with the item's short key as the terminal crumb: a banner naming the
// archive with a way back to the list, the identifier plus the archived
// quick-action menu, and the shared detail composition with every write
// surface locked. The body reads the archive-scoped detail once; a
// missing record shows the shared not-found state with a way back to
// the live list.

import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import type { ArchivedIssue } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useDocumentHead } from "../../../shared/shell/document.js";
import { ArchivesDetailFrame } from "../../archives/index.js";
import { useProjectRow } from "../../auth/index.js";
import { CommentThread } from "../comments/CommentThread.js";
import { DetailMainColumn } from "../detail/DetailMainColumn.js";
import { DetailMissingState } from "../detail/DetailMissingState.js";
import { DetailSkeleton } from "../detail/DetailSkeleton.js";
import { IssueIdentifier, issueKeyOf } from "../detail/IssueIdentifier.js";
import { IssuePropertiesSidebar } from "../detail/sidebar/index.js";
import { DetailQuickActions } from "../menus/DetailQuickActions.js";
import { useQuickActions } from "../menus/useQuickActions.js";
import { archivedIssueForDetail } from "../peek/ArchivedIssuePeek.js";
import { archivedIssueDetailPath, archivedIssueQueryOptions } from "./archivedIssues.js";

export interface ArchivedIssueDetailProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

export interface ArchivedDetailMenuProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  record: ArchivedIssue;
  projectIdentifier: string;
}

/**
 * The archived detail's quick actions: the archived menu configuration
 * (restore and delete for editors, copy-link for every viewer). A landed
 * restore lands on the live short-link the shell resolves to the browse
 * address; a landed delete returns to the archived list.
 */
export function ArchivedDetailMenu({
  workspaceSlug,
  projectId,
  issueId,
  record,
  projectIdentifier,
}: ArchivedDetailMenuProps): React.ReactElement {
  const navigate = useNavigate();
  const menu = useQuickActions({
    workspaceSlug,
    target: {
      id: record.id,
      name: record.name,
      sequenceId: record.sequence_id,
      projectId,
      projectIdentifier,
      stateId: record.state_id,
      snapshot: record,
      createdBy: record.created_by,
      archived: true,
    },
    context: "archived",
    linkHref: archivedIssueDetailPath(workspaceSlug, projectId, issueId),
    onRestored: () => {
      void navigate({
        to: "/$ws/projects/$projectId/issues/$issueId",
        params: { ws: workspaceSlug, projectId, issueId },
      });
    },
    onDeleted: () => {
      void navigate({ to: "/$ws/projects/$projectId/archives/issues/", params: { ws: workspaceSlug, projectId } });
    },
  });
  return <DetailQuickActions state={menu} />;
}

/** The archived detail screen with its frame, banner and locked body. */
export function ArchivedIssueDetail({
  workspaceSlug,
  projectId,
  issueId,
}: ArchivedIssueDetailProps): React.ReactElement {
  const detail = useQuery(archivedIssueQueryOptions(getAppClient(), workspaceSlug, projectId, issueId));
  const project = useProjectRow(workspaceSlug, projectId);
  const record = detail.data ?? null;
  const identifier = project.data?.identifier ?? null;
  useDocumentHead({ title: record === null ? "Archived work item" : record.name, robots: "noindex, nofollow" });
  // The short key needs the loaded record; until then the trail ends at
  // the archives section (the held-fetch probe reads the empty crumb).
  const crumb = record === null || identifier === null ? undefined : issueKeyOf(identifier, record.sequence_id);

  return (
    <ArchivesDetailFrame
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      {...(crumb === undefined ? {} : { detailCrumb: crumb })}
    >
      {detail.isPending ? (
        <DetailSkeleton />
      ) : record === null ? (
        <DetailMissingState issueKey={issueId} body="It may have been deleted, or the address is wrong.">
          <Link
            to="/$ws/projects/$projectId/issues"
            params={{ ws: workspaceSlug, projectId }}
            className="text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--accent) hover:bg-(--subtle)"
          >
            View live work items
          </Link>
        </DetailMissingState>
      ) : (
        <div className="flex min-w-0 flex-col gap-(--space-4)">
          <section
            data-testid="archived-detail-banner"
            aria-label="Archived notice"
            className="flex flex-wrap items-center gap-(--space-3) rounded-(--radius-control) bg-(--subtle) px-(--space-4) py-(--space-3)"
          >
            <p className="text-body min-w-0 flex-1 text-(--text)">
              This work item is archived and read-only. Restore it to bring it back to the project&apos;s active lists.
            </p>
            <Link
              to="/$ws/projects/$projectId/archives/issues/"
              params={{ ws: workspaceSlug, projectId }}
              className="text-body shrink-0 rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--accent) hover:bg-(--surface)"
            >
              Back to archived work items
            </Link>
          </section>
          <div className="flex items-center justify-between gap-(--space-2)">
            <IssueIdentifier
              projectIdentifier={identifier ?? ""}
              sequenceId={record.sequence_id}
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              issueId={record.id}
              editable={false}
            />
            <ArchivedDetailMenu
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              issueId={issueId}
              record={record}
              projectIdentifier={identifier ?? ""}
            />
          </div>
          <div className="flex min-w-0 flex-col gap-(--space-8) md:flex-row">
            <DetailMainColumn
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              issue={archivedIssueForDetail(record, null)}
              editable={false}
              activity={
                <CommentThread
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  issueId={record.id}
                  archived
                  published={false}
                  assigneeIds={[]}
                  memberRole={project.data?.member_role ?? null}
                />
              }
            />
            <section aria-label="Properties" className="flex min-w-0 flex-col gap-(--space-4) md:w-80 md:shrink-0">
              <p className="text-meta font-medium text-(--text-muted)">Properties</p>
              <IssuePropertiesSidebar
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                issue={archivedIssueForDetail(record, null)}
                editable={false}
              />
            </section>
          </div>
        </div>
      )}
    </ArchivesDetailFrame>
  );
}
