// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers, calendar tab (parity/specs/agents.md §5,
// AGT-015–017, 062). Thin composition: the feature owns the grids, the
// visibility rail and the occurrence panel; this route reads the caller's
// project role so only admins get the panel's edit entry point.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { useWorkspace } from "../../../../../../core/session/index.js";
import { useProjectRoles } from "../../../../../../features/auth/index.js";
import { isProjectAdmin } from "../../../../../../features/projects/index.js";
import { useDocumentHead } from "../../../../../../shared/shell/index.js";

// Deferred so the calendar's code stays in its own chunk (Quality gates).
const FiringCalendar = React.lazy(() =>
  import("../../../../../../features/schedulers/FiringCalendar.js").then((module) => ({
    default: module.FiringCalendar,
  }))
);

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/schedulers/calendar")({
  component: ProjectSchedulersCalendarComponent,
});

function ProjectSchedulersCalendarComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const { workspace } = useWorkspace(ws);
  const roles = useProjectRoles(ws);
  // Management needs the role read; a failed read refuses rather than
  // enabling the edit entry point.
  const canEdit = roles.isSuccess && isProjectAdmin(roles.data[projectId] ?? null);

  useDocumentHead({
    title: `${workspace?.name ?? ws} · Scheduler calendar`,
    robots: "noindex, nofollow",
  });

  return (
    <React.Suspense fallback={<div role="status" aria-label="Loading calendar" className="h-full w-full" />}>
      <FiringCalendar workspaceSlug={ws} projectId={projectId} canEdit={canEdit} />
    </React.Suspense>
  );
}
