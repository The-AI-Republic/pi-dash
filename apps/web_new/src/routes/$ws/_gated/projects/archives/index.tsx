// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived projects view (SHELL-027, SHELL-043). Thin sibling of the
// projects list: the same fetched collection filtered to archived rows,
// with the Archived crumb and no creation. Restore, delete and muted
// cards arrive with NEWFRONT-284.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ProjectsListView } from "../../../../../features/projects/components/ProjectsListView.js";

export const Route = createFileRoute("/$ws/_gated/projects/archives/")({
  component: ArchivedProjectsRoute,
});

function ArchivedProjectsRoute(): React.ReactElement {
  const { ws } = Route.useParams();
  return <ProjectsListView workspaceSlug={ws} archived={true} />;
}
