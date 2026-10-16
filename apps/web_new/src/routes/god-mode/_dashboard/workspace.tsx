// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace settings route (ADM-037–042, from parity/specs/admin.md
// §5). Thin: the dashboard layout already gates the session and mounts
// the shell, so this resolves the page and mounts the connected view.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminWorkspacePage } from "../../../features/admin/index.js";

export const Route = createFileRoute("/god-mode/_dashboard/workspace")({
  component: WorkspaceRoute,
});

function WorkspaceRoute(): React.ReactElement {
  return <AdminWorkspacePage />;
}
