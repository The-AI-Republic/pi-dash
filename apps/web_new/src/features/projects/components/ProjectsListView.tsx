// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Data-connected projects list (SHELL-024, SHELL-025). Subscribes to the
// shared workspace collection and the caller's workspace role, then
// renders the screen. The routes stay thin: they resolve the workspace
// slug and mount this view.
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { usePermissions } from "../../../core/session/index.js";
import { projectDetailsQueryOptions } from "../api/queries.js";
import { ProjectsListScreen } from "./ProjectsListScreen.js";

export interface ProjectsListViewProps {
  workspaceSlug: string;
  /** Archive view: archived rows, the Archived crumb, no creation. */
  archived?: boolean;
}

export function ProjectsListView({ workspaceSlug, archived = false }: ProjectsListViewProps): React.ReactElement {
  const details = useQuery(projectDetailsQueryOptions(getAppClient(), workspaceSlug));
  const { canEdit } = usePermissions(workspaceSlug);
  return (
    <ProjectsListScreen
      workspaceSlug={workspaceSlug}
      archived={archived}
      collection={details.data}
      canCreate={canEdit}
    />
  );
}
