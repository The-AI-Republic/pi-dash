// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace list route (ADM-037–042, from parity/specs/admin.md §5).
// The index child of the workspace layout; the create page is its
// sibling. Thin: the dashboard layout already gates the session and
// mounts the shell, so this mounts the connected list view.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminWorkspacePage } from "../../../../features/admin/components/AdminWorkspacePage.js";

export const Route = createFileRoute("/god-mode/_dashboard/workspace/")({
  component: WorkspaceRoute,
});

function WorkspaceRoute(): React.ReactElement {
  return <AdminWorkspacePage />;
}
