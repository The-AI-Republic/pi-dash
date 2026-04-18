/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// apple pi dash imports
import { WorkItemsIcon } from "@apple-pi-dash/propel/icons";
import { EInboxIssueSource } from "@apple-pi-dash/types";
// hooks
import { capitalizeFirstLetter } from "@apple-pi-dash/utils";
import { useIssueDetail } from "@/hooks/store/use-issue-detail";
// local imports
import { IssueActivityBlockComponent } from "./";

type TIssueDefaultActivity = { activityId: string; ends: "top" | "bottom" | undefined };

export const IssueDefaultActivity = observer(function IssueDefaultActivity(props: TIssueDefaultActivity) {
  const { activityId, ends } = props;
  // hooks
  const {
    activity: { getActivityById },
  } = useIssueDetail();

  const activity = getActivityById(activityId);

  if (!activity) return <></>;
  const source = activity.source_data?.source;

  return (
    <IssueActivityBlockComponent
      activityId={activityId}
      icon={<WorkItemsIcon width={14} height={14} className="text-secondary" aria-hidden="true" />}
      ends={ends}
    >
      <>
        {activity.verb === "created" ? (
          source && source !== EInboxIssueSource.IN_APP ? (
            <span>
              created the work item via{" "}
              <span className="font-medium">{capitalizeFirstLetter(source.toLowerCase() || "")}</span>.
            </span>
          ) : (
            <span> created the work item.</span>
          )
        ) : (
          <span> deleted a work item.</span>
        )}
      </>
    </IssueActivityBlockComponent>
  );
});
