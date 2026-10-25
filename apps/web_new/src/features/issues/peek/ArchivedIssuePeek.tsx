// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work-item peek (parity/specs/archives.md §2.5, ARCH-007).
// The shared peek chrome and detail composition with every write surface
// locked: the title is a heading, the description is a static view, the
// property sidebar is disabled, and the activity timeline drops its
// composer and card menus. The body reads the archive-scoped detail, not
// the live detail route.

import { useQuery } from "@tanstack/react-query";
import type { ArchivedIssue, Issue, IssueListItem } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useProjectRow } from "../../auth/index.js";
import { CommentThread } from "../comments/CommentThread.js";
import { DetailMainColumn } from "../detail/DetailMainColumn.js";
import { IssuePropertiesSidebar } from "../detail/sidebar/index.js";
import { PeekChrome } from "./PeekChrome.js";
import type { PeekMode } from "./PeekModeToggle.js";
import { PeekErrorPanel, PeekLoadingPanel } from "./peekStates.js";
import { archivedIssueQueryOptions } from "../contexts/archivedIssues.js";

/**
 * The detail composition reads the live `Issue` shape. The archive-scoped
 * single-item read omits the annotation-backed memberships and counts the
 * archived list row carries, so the peek merges the two: the row supplies
 * cycle/module/label/assignee memberships and the sub-work-item,
 * attachment and link counts (spec §2.5). A peek opened before its row is
 * in hand falls back to the empty values the readers already handle.
 * Nothing here is editable.
 */
export function archivedIssueForDetail(issue: ArchivedIssue, row?: IssueListItem | null): Issue {
  return {
    ...issue,
    cycle_id: row?.cycle_id ?? null,
    module_ids: row?.module_ids ?? [],
    label_ids: row?.label_ids ?? [],
    assignee_ids: row?.assignee_ids ?? [],
    sub_issues_count: row?.sub_issues_count ?? null,
    attachment_count: row?.attachment_count ?? null,
    link_count: row?.link_count ?? null,
    is_epic: row?.is_epic ?? false,
  };
}

export interface ArchivedIssuePeekProps {
  workspaceSlug: string;
  projectId: string;
  projectIdentifier: string;
  issueId: string;
  /** The archived list row for this item, when it is in hand. */
  row?: IssueListItem | null | undefined;
  initialMode?: PeekMode | undefined;
  onClose: () => void;
}

export function ArchivedIssuePeek({
  workspaceSlug,
  projectId,
  projectIdentifier,
  issueId,
  row = null,
  initialMode = "side",
  onClose,
}: ArchivedIssuePeekProps): React.ReactElement {
  const [mode, setMode] = React.useState<PeekMode>(initialMode);
  const detail = useQuery(archivedIssueQueryOptions(getAppClient(), workspaceSlug, projectId, issueId));
  const project = useProjectRow(workspaceSlug, projectId);
  void projectIdentifier;

  if (detail.isPending) return <PeekLoadingPanel mode={mode} onClose={onClose} />;
  if (detail.isError || detail.data === undefined) return <PeekErrorPanel mode={mode} onClose={onClose} />;

  const issue = archivedIssueForDetail(detail.data, row);
  return (
    <PeekChrome mode={mode} onModeChange={setMode} title={issue.name} titleAsHeading onClose={onClose}>
      <div className="flex min-w-0 flex-1 flex-col gap-(--space-6)">
        <DetailMainColumn
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          issue={issue}
          editable={false}
          showTitle={false}
          activity={
            <CommentThread
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              issueId={issue.id}
              archived
              published={false}
              assigneeIds={issue.assignee_ids}
              memberRole={project.data?.member_role ?? null}
            />
          }
        />
        <section aria-label="Properties" className="flex min-w-0 flex-col gap-(--space-4)">
          <p className="text-meta font-medium text-(--text-muted)">Properties</p>
          <IssuePropertiesSidebar workspaceSlug={workspaceSlug} projectId={projectId} issue={issue} editable={false} />
        </section>
      </div>
    </PeekChrome>
  );
}
