/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import type { TIssue } from "@pi-dash/types";
import { IssueDetailRoot } from "@/components/issues/issue-detail/root";

export type TWorkItemDetailRoot = {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  issue: TIssue | undefined;
  isMetadataHydrating?: boolean;
};

export const WorkItemDetailRoot = observer(function WorkItemDetailRoot(props: TWorkItemDetailRoot) {
  const { workspaceSlug, projectId, issueId, issue, isMetadataHydrating = false } = props;

  return (
    <IssueDetailRoot
      workspaceSlug={workspaceSlug.toString()}
      projectId={projectId.toString()}
      issueId={issueId.toString()}
      is_archived={!!issue?.archived_at}
      isMetadataHydrating={isMetadataHydrating}
    />
  );
});
