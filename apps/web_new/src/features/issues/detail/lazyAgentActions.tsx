// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy entry for the issue-surface agent controls (RUN-046, RUN-048).
// The row's runners dispatch hooks ride their own chunk: the notification
// peek mounts this entry from the issues barrel, so a static re-export of
// the controls would pull the dispatch code into every eager barrel
// importer's closure, including the initial chain (NEWFRONT-867).
import * as React from "react";

import type { IssueAgentActionsProps } from "./IssueAgentActions.js";

const LazyIssueAgentActions = React.lazy(() =>
  import("./IssueAgentActions.js").then((module) => ({ default: module.IssueAgentActions }))
);

export type IssueAgentActionsEntryProps = IssueAgentActionsProps;

/** The agent row behind its own boundary; mounts without suspending the host. */
export function IssueAgentActionsEntry(props: IssueAgentActionsEntryProps): React.ReactElement {
  return (
    <React.Suspense fallback={<p className="text-body text-(--text-muted)">Loading agent actions…</p>}>
      <LazyIssueAgentActions {...props} />
    </React.Suspense>
  );
}
