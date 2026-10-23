// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycles tab route (parity/specs/archives.md §1,
// ARCH-001–003). Thin: the per-tab browser title plus the shared
// shell; the cycles slice mounts its content into the shell slot. A
// direct address with the cycles flag off lands on the work-items tab
// instead of an empty frame, the same outcome the strip offers.

import { createFileRoute, Navigate } from "@tanstack/react-router";
import * as React from "react";

import { ArchivesShell, archivesRouteTitle, archivesTabHref } from "../../../../../../../features/archives/index.js";
import { useProjectRow } from "../../../../../../../features/auth/index.js";
import { useDocumentHead } from "../../../../../../../shared/shell/index.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/archives/cycles/")({
  component: ArchivedCyclesTab,
});

function ArchivedCyclesTab(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const project = useProjectRow(ws, projectId);
  const row = project.data;
  useDocumentHead({
    title: archivesRouteTitle(row?.name, "cycles"),
    robots: "noindex, nofollow",
  });
  if (row !== undefined && !row.cycle_view) {
    return <Navigate to={archivesTabHref(ws, projectId, "issues")} />;
  }
  return <ArchivesShell workspaceSlug={ws} projectId={projectId} activeTab="cycles" />;
}
