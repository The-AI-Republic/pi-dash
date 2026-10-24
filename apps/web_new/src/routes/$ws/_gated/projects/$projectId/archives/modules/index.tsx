// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-modules tab route (parity/specs/archives.md §1,
// ARCH-001–003). Thin: the per-tab browser title plus the shared
// shell; the modules slice mounts its content into the shell slot. A
// direct address with the modules flag off lands on the work-items tab
// instead of an empty frame, the same outcome the strip offers.

import { createFileRoute, Navigate } from "@tanstack/react-router";
import * as React from "react";

import { ArchivesShell } from "../../../../../../../features/archives/components/ArchivesShell.js";
import { archivesRouteTitle, archivesTabHref } from "../../../../../../../features/archives/tabs.js";
import { useProjectRow } from "../../../../../../../features/auth/queries.js";
import { useDocumentHead } from "../../../../../../../shared/shell/document.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/archives/modules/")({
  component: ArchivedModulesTab,
});

function ArchivedModulesTab(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const project = useProjectRow(ws, projectId);
  const row = project.data;
  useDocumentHead({
    title: archivesRouteTitle(row?.name, "modules"),
    robots: "noindex, nofollow",
  });
  if (row !== undefined && !row.module_view) {
    return <Navigate to={archivesTabHref(ws, projectId, "issues")} />;
  }
  return <ArchivesShell workspaceSlug={ws} projectId={projectId} activeTab="modules" />;
}
