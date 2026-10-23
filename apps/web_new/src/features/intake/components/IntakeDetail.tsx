// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request detail pane (parity/specs/intake.md INT-019–028). The
// one-shot detail read composed through the shared detail root: the
// intake header (chevrons, status chip, triage buttons and menu,
// copy/open controls) above, the request properties beside, the work
// item's attachments below. Title and description save through the
// intake update; the permission matrix decides what edits. Arrow keys
// step between requests unless typing or an overlay owns them; a stale
// or forbidden id reports missing so the screen falls back to the tab.
// Triage moves advance through the screen's neighbour callback.

import { INTAKE_STATUS, type IntakeIssueDetail } from "@pidash/api-client";
import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { workItemPath } from "../../../shared/shell/shareLink.js";
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
import { useTriageControls } from "../api/triage.js";
import { INTAKE_NAV_OVERLAY_SELECTOR, suppressIntakeArrowKey } from "../lib/navigation.js";
import { isResolvedIntakeStatus, selectIntakeEditability } from "../lib/permissions.js";
import { IntakeDetailHeader } from "./IntakeDetailHeader.js";
import { IntakeProperties } from "./IntakeProperties.js";
import { IntakeTriageActions } from "./IntakeTriageActions.js";

export interface IntakeDetailProps {
  workspaceSlug: string;
  projectId: string;
  /** Selected work-item id from the address. */
  issueId: string;
  /** Route key shown to people, e.g. "APO-7". */
  projectIdentifier: string;
  /** Project name for the accept form's project row. */
  projectName: string;
  /** The viewer's project role, or null outside the project. */
  role: number | null;
  onPrev: () => void;
  onNext: () => void;
  /** The id is stale or forbidden; the screen drops it from the address. */
  onMissing: () => void;
  /**
   * A triage move took the request out of the tab; the screen advances
   * the address to the neighbour. True when the tab emptied.
   */
  onAdvance: (removedId: string) => boolean;
}

export function IntakeDetail({
  workspaceSlug,
  projectId,
  issueId,
  projectIdentifier,
  projectName,
  role,
  onPrev,
  onNext,
  onMissing,
  onAdvance,
}: IntakeDetailProps): React.ReactElement | null {
  const detail = useIntakeDetail(workspaceSlug, projectId, issueId);
  const me = useMe();

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

  return (
    <IntakeDetailLoaded
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      issueId={issueId}
      projectIdentifier={projectIdentifier}
      projectName={projectName}
      role={role}
      detail={detail.data}
      viewerId={me.data?.id ?? null}
      onPrev={onPrev}
      onNext={onNext}
      onAdvance={onAdvance}
    />
  );
}

interface IntakeDetailLoadedProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  projectIdentifier: string;
  projectName: string;
  role: number | null;
  detail: IntakeIssueDetail;
  viewerId: string | null;
  onPrev: () => void;
  onNext: () => void;
  onAdvance: (removedId: string) => boolean;
}

function IntakeDetailLoaded({
  workspaceSlug,
  projectId,
  issueId,
  projectIdentifier,
  projectName,
  role,
  detail,
  viewerId,
  onPrev,
  onNext,
  onAdvance,
}: IntakeDetailLoadedProps): React.ReactElement {
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

  const creatorId = detail.issue.created_by;
  const isAdmin = isProjectAdmin(role);
  const isCreator = viewerId !== null && creatorId !== null && viewerId === creatorId;
  const editability = selectIntakeEditability({
    isAdmin,
    isCreator,
    isGuest: role !== null && !isProjectAuthorized(role),
    status: detail.status,
  });
  const issue = intakeDetailWorkItem(detail.issue);
  const issueKey = `${projectIdentifier}-${detail.issue.sequence_id}`;
  const triage = useTriageControls({
    workspaceSlug,
    projectId,
    projectName,
    projectIdentifier,
    issueId,
    issueKey,
    issue,
    status: detail.status,
    snoozedTill: detail.snoozed_till,
    isAdmin,
    isAuthorized: isProjectAuthorized(role),
    isCreator,
    onAdvance,
  });
  const duplicate = detail.duplicate_issue_detail;
  const duplicateKey = duplicate === null ? null : `${projectIdentifier}-${duplicate.sequence_id}`;
  const duplicateOf =
    detail.status === INTAKE_STATUS.duplicate && duplicateKey !== null
      ? { issueKey: duplicateKey, href: workItemPath(workspaceSlug, duplicateKey) }
      : null;

  return (
    <div data-testid="intake-detail" tabIndex={-1} className="flex h-full min-h-0 flex-col overflow-y-auto">
      <IntakeDetailHeader
        workspaceSlug={workspaceSlug}
        issueKey={issueKey}
        resolved={isResolvedIntakeStatus(detail.status)}
        canCopy={isProjectAuthorized(role)}
        onPrev={onPrev}
        onNext={onNext}
        status={detail.status}
        snoozedTill={detail.snoozed_till}
        menuLeadEntries={triage.leadEntries}
        menuTrailEntries={triage.trailEntries}
        actions={
          triage.visibility.showTriage ? (
            <IntakeTriageActions
              controls={triage}
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              projectName={projectName}
              projectIdentifier={projectIdentifier}
              issueId={issueId}
            />
          ) : undefined
        }
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
            accepted={detail.status === INTAKE_STATUS.accepted}
            duplicateOf={duplicateOf}
          />
        }
      />
    </div>
  );
}
