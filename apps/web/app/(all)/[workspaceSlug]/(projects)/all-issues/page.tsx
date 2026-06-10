/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
// components
import { PageHead } from "@/components/core/page-title";
import { AllIssueLayoutRoot } from "@/components/issues/issue-layouts/roots/all-issue-layout-root";
// hooks
import { useWorkspace } from "@/hooks/store/use-workspace";

// Workspace-wide "all work items" view. The active view id ("all-issues") is
// provided by the route layout via GlobalViewIdProvider, so this renders the
// same multi-layout root as `/workspace-views/all-issues`.
function AllIssuesPage() {
  // store hooks
  const { currentWorkspace } = useWorkspace();
  // states
  const [isLoading, setIsLoading] = useState(false);

  // derived values
  const pageTitle = currentWorkspace?.name ? `${currentWorkspace?.name} - Work Items` : undefined;

  // handlers
  const toggleLoading = (value: boolean) => setIsLoading(value);

  return (
    <>
      <PageHead title={pageTitle} />
      <AllIssueLayoutRoot isDefaultView isLoading={isLoading} toggleLoading={toggleLoading} />
    </>
  );
}

export default observer(AllIssuesPage);
