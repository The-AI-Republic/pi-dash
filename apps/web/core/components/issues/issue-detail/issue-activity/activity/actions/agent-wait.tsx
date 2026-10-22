/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { Hourglass } from "lucide-react";
// hooks
import { useIssueDetail } from "@/hooks/store/use-issue-detail";
// components
import { IssueActivityBlockComponent } from "./";

type TIssueAgentWaitActivity = { activityId: string; ends: "top" | "bottom" | undefined };

/**
 * The agent read its open blockers, decided it could not safely proceed, and
 * called `pidash issue wait` — buying back the tick the ending run spent.
 * `new_value` is this wait's number, `old_value` the per-issue allowance.
 * Repeated entries are the signal that an issue is stuck rather than busy.
 */
export const IssueAgentWaitActivity = observer(function IssueAgentWaitActivity(props: TIssueAgentWaitActivity) {
  const { activityId, ends } = props;
  // hooks
  const {
    activity: { getActivityById },
  } = useIssueDetail();

  const activity = getActivityById(activityId);

  if (!activity) return <></>;

  return (
    <IssueActivityBlockComponent
      icon={<Hourglass className="h-3.5 w-3.5 text-secondary" aria-hidden="true" />}
      activityId={activityId}
      ends={ends}
      customUserName="Pi Dash"
    >
      chose to wait on a blocker{" "}
      <span className="font-medium text-primary">
        ({activity.new_value} of {activity.old_value})
      </span>
      .
    </IssueActivityBlockComponent>
  );
});
