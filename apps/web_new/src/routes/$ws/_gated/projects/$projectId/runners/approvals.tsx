// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approvals tab, project scope (NEWFRONT-547, RUN-023/RUN-024). The
// screen reads the scope from the area context, so the route only mounts it.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ApprovalsScreen } from "../../../../../../features/runners/approvals/ApprovalsScreen.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/runners/approvals")({
  component: ApprovalsMountRoute,
});

function ApprovalsMountRoute(): React.ReactElement {
  return <ApprovalsScreen />;
}
