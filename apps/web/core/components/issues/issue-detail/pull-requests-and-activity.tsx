/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// local imports
import { IssueGithubPullRequestsRoot } from "./github-pull-requests";
import { IssueActivity } from "./issue-activity";

type Props = {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Disables the PR attach/detach controls (broader rule: non-editable / hydrating). */
  disabled?: boolean;
  /** Disables the activity/comment input (typically just archived). */
  activityDisabled?: boolean;
};

/**
 * Renders the "Pull requests" section directly above the Activity section.
 * They are composed here so every issue layout (full detail + peek variants)
 * stays in sync — render this once instead of two separate components.
 */
export const IssuePullRequestsAndActivity = observer(function IssuePullRequestsAndActivity(props: Props) {
  const { workspaceSlug, projectId, issueId, disabled = false, activityDisabled = false } = props;
  return (
    <>
      <IssueGithubPullRequestsRoot
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issueId={issueId}
        disabled={disabled}
      />
      <IssueActivity
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issueId={issueId}
        disabled={activityDisabled}
      />
    </>
  );
});
