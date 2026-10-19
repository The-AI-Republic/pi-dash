// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail widget stack (parity/specs/issues.md §15/§16). The browse detail
// mounts the widgets this slice owns in their fixed order: Attachments
// then Code reviews, above the activity area that a later slice fills.
// The worklog seam renders nothing in the community build, so no worklog
// control reaches the DOM.

import * as React from "react";

import { AttachmentsWidget } from "./attachments/AttachmentsWidget.js";
import { CodeReviewsWidget } from "./codeReviews/CodeReviewsWidget.js";

export interface IssueDetailWidgetsProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Guests, archived and synced items read; writes stay hidden. */
  editable: boolean;
}

export function IssueDetailWidgets({
  workspaceSlug,
  projectId,
  issueId,
  editable,
}: IssueDetailWidgetsProps): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-4)" data-testid="issue-detail-widgets">
      <AttachmentsWidget workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} editable={editable} />
      <CodeReviewsWidget workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} editable={editable} />
    </div>
  );
}
