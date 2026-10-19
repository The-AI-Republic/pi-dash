// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overview screen container (NEWFRONT-540, RUN-002). One component for both
// scopes: it resolves the area from context, loads the table's data and the
// title, and renders the table. Kept out of the route file so a unit test can
// mount it with a stub area.

import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";
import { useRunnersArea } from "../area.js";
import { useRunnersOverview } from "../useOverview.js";
import { RunnersOverview } from "./RunnersOverview.js";

/** Page title (RUN-005 pins "AI Agents"); one area name per surface. */
export const RUNNERS_PAGE_TITLE = "AI Agents";

export interface RunnersOverviewScreenProps {
  workspaceSlug: string;
  /** Workspace name for the tab title; omitted outside a loaded workspace. */
  workspaceName?: string | undefined;
}

export function runnersDocumentTitle(workspaceName: string | undefined): string {
  return workspaceName === undefined ? RUNNERS_PAGE_TITLE : `${workspaceName} - ${RUNNERS_PAGE_TITLE}`;
}

export function RunnersOverviewScreen({
  workspaceSlug,
  workspaceName,
}: RunnersOverviewScreenProps): React.ReactElement {
  const area = useRunnersArea();
  const data = useRunnersOverview(workspaceSlug);
  useDocumentHead({ title: runnersDocumentTitle(workspaceName), robots: "noindex, nofollow" });
  return (
    <RunnersOverview
      scope={area.scope}
      filter={data}
      pods={data.pods}
      podsLoading={data.podsLoading}
      podsFailed={data.podsFailed}
      onDelete={data.onDelete}
      onRevoke={data.onRevoke}
    />
  );
}
