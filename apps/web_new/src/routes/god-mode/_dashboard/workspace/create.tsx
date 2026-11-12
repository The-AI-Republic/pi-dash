// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-workspace route (ADM-043–045, from parity/specs/admin.md
// §5.7–§5.9): the name/slug/size form behind the slug-availability
// gate. The index child of the workspace layout carries the list;
// this sibling carries the create page.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminWorkspaceCreatePage } from "../../../../features/admin/components/AdminWorkspaceCreatePage.js";

export const Route = createFileRoute("/god-mode/_dashboard/workspace/create")({
  component: WorkspaceCreateRoute,
});

function WorkspaceCreateRoute(): React.ReactElement {
  return <AdminWorkspaceCreatePage />;
}
