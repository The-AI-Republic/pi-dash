/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { AlarmClock } from "lucide-react";
// hooks
import { useIssueDetail } from "@/hooks/store/use-issue-detail";
// components
import { IssueActivityBlockComponent } from "./";

type TIssueAgentWakeActivity = { activityId: string; ends: "top" | "bottom" | undefined };

/** The agent was woken because a work item this one is blocked by was completed or cancelled. */
export const IssueAgentWakeActivity = observer(function IssueAgentWakeActivity(props: TIssueAgentWakeActivity) {
  const { activityId, ends } = props;
  // hooks
  const {
    activity: { getActivityById },
  } = useIssueDetail();

  const activity = getActivityById(activityId);

  if (!activity) return <></>;

  return (
    <IssueActivityBlockComponent
      icon={<AlarmClock className="h-3.5 w-3.5 text-secondary" aria-hidden="true" />}
      activityId={activityId}
      ends={ends}
      customUserName="Pi Dash"
    >
      woke the agent: blocker <span className="font-medium text-primary">{activity.new_value}</span> was{" "}
      {activity.old_value === "cancelled" ? "cancelled" : "completed"}.
    </IssueActivityBlockComponent>
  );
});
