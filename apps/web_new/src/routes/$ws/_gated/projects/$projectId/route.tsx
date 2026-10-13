// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project layout (AUTH-024). Every project page renders through the
// project access gate: members see the page, everyone else gets the
// join, no-access, or not-found screen keyed off the details-fetch
// status plus admin standing.

import { createFileRoute, Outlet } from "@tanstack/react-router";
import * as React from "react";

import { ProjectAccessGate } from "../../../../../features/auth/index.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId")({
  component: ProjectLayoutComponent,
});

function ProjectLayoutComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectAccessGate slug={ws} projectId={projectId}>
      <Outlet />
    </ProjectAccessGate>
  );
}
