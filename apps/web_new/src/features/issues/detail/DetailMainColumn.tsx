// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail main column (parity/specs/issues.md §13, ISS-144–147). Owns the
// shared save pipeline the title and description report into, the save
// indicator beside it, and the restore handshake between the version
// history and the description field: a restore shows the version at
// once and clears once the server confirms it (or immediately, back to
// the server value, when the write is refused). The description-meta row
// carries the issue reaction cluster (ISS-204) beside the history line,
// with the activity block (ISS-194–204) in its own slot below; the header
// row (NEWFRONT-644) plugs into its own slot. The cloud duplicate popover
// (ISS-231) sits beside the save indicator; the community build renders
// nothing there.

import type { Issue } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { descriptionAssetSrc } from "../../../shared/editor/index.js";
import { DESCRIPTION_SAVE_FAILED_TITLE, TITLE_SAVE_FAILED_TITLE, useUpdateIssue } from "../api/mutations.js";
import { DetailDuplicatePopover } from "../edition/DetailDuplicatePopover.js";
import { IssueReactions } from "../reactions/IssueReactions.js";
import { DescriptionHistory } from "./DescriptionHistory.js";
import { IssueDescriptionField, type RestoredDescription } from "./IssueDescriptionField.js";
import { IssueTitleField } from "./IssueTitleField.js";
import { SaveIndicator } from "./SaveIndicator.js";
import { useSavePipeline, type SavePipeline } from "./savePipeline.js";
import { WaitingBanner } from "./waiting/WaitingBanner.js";

/**
 * Save overrides for surfaces whose fields persist somewhere other than
 * the work-item PATCH (intake routes both fields through its own
 * update). Each settles when its write settles.
 */
export interface DetailFieldWrites {
  saveTitle: (name: string) => Promise<unknown>;
  saveDescription: (html: string) => Promise<unknown>;
}

export interface DetailMainColumnProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  /** Project member/admin standing on a live project; guests read. Archived and synced rows still render read-only. */
  editable: boolean;
  /** Header row, left of the save indicator (identifier/type). */
  topSlot?: React.ReactNode;
  /** Agent controls on surfaces without the widget action row (notification peek). */
  agentActions?: React.ReactNode;
  /** Block under the description-meta row, above the sidebar break (the activity feed). */
  activity?: React.ReactNode;
  /** Title/description saves; the work-item PATCH when absent. */
  fieldWrites?: DetailFieldWrites | undefined;
  /** Hide the title field when the host renders it (the peek panel owns its title input). */
  showTitle?: boolean | undefined;
  /** Shared save pipeline; the column owns one when absent (the peek shares its title pipeline). */
  pipeline?: SavePipeline | undefined;
}

export function DetailMainColumn({
  workspaceSlug,
  projectId,
  issue,
  editable,
  topSlot,
  agentActions,
  activity,
  fieldWrites,
  showTitle = true,
  pipeline: pipelineProp,
}: DetailMainColumnProps): React.ReactElement {
  const ownedPipeline = useSavePipeline();
  const pipeline = pipelineProp ?? ownedPipeline;
  const titleUpdate = useUpdateIssue({
    workspaceSlug,
    projectId,
    issueId: issue.id,
    failureTitle: TITLE_SAVE_FAILED_TITLE,
  });
  const descriptionUpdate = useUpdateIssue({
    workspaceSlug,
    projectId,
    issueId: issue.id,
    failureTitle: DESCRIPTION_SAVE_FAILED_TITLE,
  });
  const saveTitle = (name: string): Promise<unknown> =>
    fieldWrites === undefined ? titleUpdate.mutateAsync({ name }) : fieldWrites.saveTitle(name);
  const saveDescription = (html: string): Promise<unknown> =>
    fieldWrites === undefined
      ? descriptionUpdate.mutateAsync({ description_html: html })
      : fieldWrites.saveDescription(html);
  const [restored, setRestored] = React.useState<RestoredDescription | null>(null);
  const tokenRef = React.useRef(0);

  React.useEffect(() => {
    if (restored !== null && issue.description_html === restored.html) setRestored(null);
  }, [issue.description_html, restored]);

  const archived = issue.archived_at !== null;
  const readOnly = !editable || archived || issue.is_synced;
  // Legacy bodies keep the asset id in the image source; the stored id
  // resolves through this issue's project asset path (ISS-127).
  const resolveAsset = React.useCallback(
    (src: string) => descriptionAssetSrc(src, { workspaceSlug, projectId, apiBaseUrl: getAppClient().baseUrl }),
    [workspaceSlug, projectId]
  );

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-(--space-6)">
      <div className="flex items-start justify-between gap-(--space-4)">
        <div className="min-w-0 flex-1">{topSlot}</div>
        <div className="flex shrink-0 items-center gap-(--space-2) pt-(--space-2)">
          <DetailDuplicatePopover
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            title={issue.name}
            issueId={issue.id}
          />
          <SaveIndicator status={pipeline.status} />
        </div>
      </div>
      <WaitingBanner workspaceSlug={workspaceSlug} projectId={projectId} issue={issue} disabled={readOnly} />
      {agentActions === undefined ? null : <div className="min-w-0">{agentActions}</div>}
      {showTitle ? (
        <IssueTitleField value={issue.name} disabled={readOnly} pipeline={pipeline} onSave={saveTitle} />
      ) : null}
      <IssueDescriptionField
        value={issue.description_html}
        disabled={readOnly}
        pipeline={pipeline}
        onSave={saveDescription}
        restored={restored}
        resolveAsset={resolveAsset}
      />
      <div data-slot="description-meta" className="flex flex-wrap items-center justify-between gap-(--space-4)">
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-(--space-4)">
          <IssueReactions workspaceSlug={workspaceSlug} projectId={projectId} issueId={issue.id} disabled={archived} />
        </div>
        {editable ? (
          <DescriptionHistory
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            issueId={issue.id}
            createdAt={issue.created_at}
            createdBy={issue.created_by}
            synced={issue.is_synced}
            restoreDisabled={!editable || archived || issue.is_synced}
            onRestore={(html: string) => {
              tokenRef.current += 1;
              setRestored({ html, token: tokenRef.current });
              pipeline.markSaving();
              void saveDescription(html).then(
                () => pipeline.markSaved(),
                () => {
                  setRestored(null);
                  pipeline.markSaved();
                }
              );
            }}
          />
        ) : null}
      </div>
      {activity === undefined ? null : <div className="min-w-0">{activity}</div>}
    </div>
  );
}
