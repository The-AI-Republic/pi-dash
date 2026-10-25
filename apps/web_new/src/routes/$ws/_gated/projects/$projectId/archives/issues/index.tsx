// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work-items tab route (parity/specs/archives.md §1,
// ARCH-001–003). Thin: the per-tab browser title plus the shared
// shell; the list slice mounts its content into the shell slot.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ArchivesShell } from "../../../../../../../features/archives/components/ArchivesShell.js";
import { archivesRouteTitle } from "../../../../../../../features/archives/tabs.js";
import { useProjectRow } from "../../../../../../../features/auth/queries.js";
import { useDocumentHead } from "../../../../../../../shared/shell/document.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/archives/issues/")({
  component: ArchivedIssuesTab,
});

function ArchivedIssuesTab(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const project = useProjectRow(ws, projectId);
  useDocumentHead({
    title: archivesRouteTitle(project.data?.name, "issues"),
    robots: "noindex, nofollow",
  });
  return <ArchivesShell workspaceSlug={ws} projectId={projectId} activeTab="issues" />;
}
