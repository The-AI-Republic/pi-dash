// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approvals tab mount point (NEWFRONT-540, RUN-003). The approval queue lands
// with NEWFRONT-547; the route keeps the tab deep-linkable in both scopes.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunnersAreaSectionMount } from "../../../../features/runners/components/RunnersAreaSectionMount.js";

export const Route = createFileRoute("/$ws/_gated/runners/approvals")({
  component: ApprovalsMountRoute,
});

function ApprovalsMountRoute(): React.ReactElement {
  return <RunnersAreaSectionMount title="Approvals" />;
}
