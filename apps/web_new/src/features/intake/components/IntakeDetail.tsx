// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request detail pane (parity/specs/intake.md INT-019/024/026–029). The
// one-shot detail read composed through the shared detail root: the
// intake header (chevrons, copy/open controls) above, the triage-state
// properties beside, the work item's attachments below. Title and
// description save through the intake update; the permission matrix
// decides what edits. Arrow keys step between requests unless typing
// or an overlay owns them; a stale or forbidden id reports missing so
// the screen falls back to the tab.

import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { isProjectAdmin, isProjectAuthorized } from "../../projects/index.js";
import {
  AttachmentsWidget,
  DESCRIPTION_SAVE_FAILED_TITLE,
  DetailSkeleton,
  IssueDetailRoot,
  TITLE_SAVE_FAILED_TITLE,
} from "../../issues/index.js";
import { intakeDetailWorkItem, isMissingIntakeDetail, useIntakeDetail } from "../api/detail.js";
import { useUpdateIntakeIssue } from "../api/mutations.js";
import { INTAKE_NAV_OVERLAY_SELECTOR, suppressIntakeArrowKey } from "../lib/navigation.js";
import { isResolvedIntakeStatus, selectIntakeEditability } from "../lib/permissions.js";
import { IntakeDetailHeader } from "./IntakeDetailHeader.js";
import { IntakeProperties } from "./IntakeProperties.js";

export interface IntakeDetailProps {
  workspaceSlug: string;
  projectId: string;
  /** Selected work-item id from the address. */
  issueId: string;
  /** Project code prefixing the route key, e.g. "APO" in "APO-7". */
  projectIdentifier: string;
  /** The viewer's project role, or null outside the project. */
  role: number | null;
  onPrev: () => void;
  onNext: () => void;
  /** The id is stale or forbidden; the screen drops it from the address. */
  onMissing: () => void;
}

export function IntakeDetail({
  workspaceSlug,
  projectId,
  issueId,
  projectIdentifier,
  role,
  onPrev,
  onNext,
  onMissing,
}: IntakeDetailProps): React.ReactElement | null {
  const detail = useIntakeDetail(workspaceSlug, projectId, issueId);
  const me = useMe();
  const titleUpdate = useUpdateIntakeIssue({
    workspaceSlug,
    projectId,
    issueId,
    failureTitle: TITLE_SAVE_FAILED_TITLE,
  });
  const descriptionUpdate = useUpdateIntakeIssue({
    workspaceSlug,
    projectId,
    issueId,
    failureTitle: DESCRIPTION_SAVE_FAILED_TITLE,
  });

  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
      if (event.repeat || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return;
      const overlayOpen = document.querySelector(INTAKE_NAV_OVERLAY_SELECTOR) !== null;
      if (suppressIntakeArrowKey(event.target, overlayOpen)) return;
      event.preventDefault();
      if (event.key === "ArrowUp") onPrev();
      else onNext();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onPrev, onNext]);

  const missing = detail.isError && isMissingIntakeDetail(detail.error);
  React.useEffect(() => {
    if (missing) onMissing();
  }, [missing, onMissing]);

  if (detail.isPending) {
    return (
      <div data-testid="intake-detail" className="min-h-0 flex-1">
        <DetailSkeleton />
      </div>
    );
  }
  if (missing || detail.data === undefined) {
    if (!missing) {
      return (
        <div data-testid="intake-detail-error" className="flex flex-col gap-(--space-2) p-(--space-6)">
          <h3 className="text-emphasis text-(--text)">The request could not be loaded.</h3>
          <button
            type="button"
            className="text-body self-start text-(--accent) underline"
            onClick={() => void detail.refetch()}
          >
            Retry
          </button>
        </div>
      );
    }
    return null;
  }

  const viewerId = me.data?.id ?? null;
  const creatorId = detail.data.issue.created_by;
  const editability = selectIntakeEditability({
    isAdmin: isProjectAdmin(role),
    isCreator: viewerId !== null && creatorId !== null && viewerId === creatorId,
    isGuest: role !== null && !isProjectAuthorized(role),
    status: detail.data.status,
  });
  const issue = intakeDetailWorkItem(detail.data.issue);
  const issueKey = `${projectIdentifier}-${detail.data.issue.sequence_id}`;

  return (
    <div data-testid="intake-detail" className="flex h-full min-h-0 flex-col overflow-y-auto">
      <IntakeDetailHeader
        workspaceSlug={workspaceSlug}
        issueKey={issueKey}
        resolved={isResolvedIntakeStatus(detail.data.status)}
        canCopy={isProjectAuthorized(role)}
        onPrev={onPrev}
        onNext={onNext}
      />
      <IssueDetailRoot
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issue={issue}
        editable={editability.canEditContent}
        showParentBanner={false}
        fieldWrites={{
          saveTitle: (name: string) => titleUpdate.mutateAsync({ issue: { name } }),
          saveDescription: (html: string) => descriptionUpdate.mutateAsync({ issue: { description_html: html } }),
        }}
        topSlot={<span className="text-meta font-medium text-(--text-muted)">{issueKey}</span>}
        activity={
          <AttachmentsWidget
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            issueId={issue.id}
            editable={editability.canEditContent}
          />
        }
        sidebar={
          <IntakeProperties
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            issue={issue}
            canEdit={editability.canEditProperties}
          />
        }
      />
    </div>
  );
}
