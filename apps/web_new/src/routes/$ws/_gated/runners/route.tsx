// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners area layout, workspace scope (NEWFRONT-540, RUN-001/RUN-004). The
// area shell owns the role gate, the side rail and the content column; the
// tabs and every child screen render inside it. The workspace id comes from
// the session because the list reads are scoped by id, not slug.

import { createFileRoute, Outlet } from "@tanstack/react-router";
import * as React from "react";

import { selectWorkspace, useWorkspaces } from "../../../../core/session/index.js";
import { RunnersWorkspaceShell } from "../../../../features/runners/index.js";

export const Route = createFileRoute("/$ws/_gated/runners")({
  component: RunnersLayoutComponent,
});

function RunnersLayoutComponent(): React.ReactElement | null {
  const { ws } = Route.useParams();
  const workspaces = useWorkspaces();
  const workspace = selectWorkspace(workspaces.data, ws);
  // No workspace row yet means the surrounding $ws gate is still settling.
  if (workspace === undefined) return null;
  return (
    <RunnersWorkspaceShell workspaceSlug={ws} workspaceId={workspace.id}>
      <Outlet />
    </RunnersWorkspaceShell>
  );
}
