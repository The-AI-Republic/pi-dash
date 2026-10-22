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
// row (NEWFRONT-644) plugs into its own slot.

import type { Issue } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { descriptionAssetSrc } from "../../../shared/editor/index.js";
import { DESCRIPTION_SAVE_FAILED_TITLE, TITLE_SAVE_FAILED_TITLE, useUpdateIssue } from "../api/mutations.js";
import { IssueReactions } from "../reactions/IssueReactions.js";
import { DescriptionHistory } from "./DescriptionHistory.js";
import { IssueDescriptionField, type RestoredDescription } from "./IssueDescriptionField.js";
import { IssueTitleField } from "./IssueTitleField.js";
import { SaveIndicator } from "./SaveIndicator.js";
import { useSavePipeline } from "./savePipeline.js";
import { WaitingBanner } from "./waiting/WaitingBanner.js";

export interface DetailMainColumnProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  /** Project member/admin standing on a live project; guests read. Archived and synced rows still render read-only. */
  editable: boolean;
  /** Header row, left of the save indicator (identifier/type). */
  topSlot?: React.ReactNode;
  /** Block under the description-meta row, above the sidebar break (the activity feed). */
  activity?: React.ReactNode;
}

export function DetailMainColumn({
  workspaceSlug,
  projectId,
  issue,
  editable,
  topSlot,
  activity,
}: DetailMainColumnProps): React.ReactElement {
  const pipeline = useSavePipeline();
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
        <div className="shrink-0 pt-(--space-2)">
          <SaveIndicator status={pipeline.status} />
        </div>
      </div>
      <WaitingBanner workspaceSlug={workspaceSlug} projectId={projectId} issue={issue} disabled={readOnly} />
      <IssueTitleField
        value={issue.name}
        disabled={readOnly}
        pipeline={pipeline}
        onSave={(name: string) => titleUpdate.mutateAsync({ name })}
      />
      <IssueDescriptionField
        value={issue.description_html}
        disabled={readOnly}
        pipeline={pipeline}
        onSave={(html: string) => descriptionUpdate.mutateAsync({ description_html: html })}
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
              void descriptionUpdate.mutateAsync({ description_html: html }).then(
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
