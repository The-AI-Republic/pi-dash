// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI dev machines layout (NEWFRONT-540, RUN-001). Same workspace role gate as
// the runners area: admins and members reach the screen, everyone else gets
// the area's not-authorized view inside normal workspace chrome. The role
// record is awaited before anything renders, so the denial never flashes.

import { createFileRoute, Outlet } from "@tanstack/react-router";
import * as React from "react";

import { usePermissions } from "../../../../core/session/index.js";
import { decideWorkspaceRunnersAccess } from "../../../../features/runners/index.js";
import { RunnersNotAuthorized } from "../../../../features/runners/components/NotAuthorizedView.js";

export const Route = createFileRoute("/$ws/_gated/ai-dev-machines")({
  component: AiDevMachinesLayout,
});

function AiDevMachinesLayout(): React.ReactElement | null {
  const { ws } = Route.useParams();
  const permissions = usePermissions(ws);
  const access = decideWorkspaceRunnersAccess({ role: permissions.role, isLoading: permissions.isLoading });
  if (access === "loading") return null;
  if (access === "denied") return <RunnersNotAuthorized scope="workspace" />;
  return <Outlet />;
}
