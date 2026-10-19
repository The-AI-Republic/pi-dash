// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail page's properties sidebar (parity/specs/issues.md §13,
// ISS-148–152). The rows need the project row: it names the allowed
// execution targets and decides the runs-on label. Until it resolves a
// skeleton stands in; a failed read offers a retry instead of an empty
// panel, so a transient error never reads as "no properties".

import { Button } from "@pidash/kit";
import type { ApiClient, Issue, ProjectDetail } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { pickerProjectQueryOptions } from "../../../../shared/pickers/index.js";
import { SidebarProperties, SidebarPropertiesSkeleton } from "./SidebarProperties.js";

export const PROPERTIES_LOAD_FAILED = "Couldn't load the properties.";

/** The fields the sidebar reads; both the detail and the list project rows answer them. */
export type SidebarProject = Pick<ProjectDetail, "agent_executor_options">;

export interface IssuePropertiesSidebarProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  /** Project member/admin on a live project. */
  editable: boolean;
  /** Test seam; the app client by default. */
  client?: ApiClient;
}

export function IssuePropertiesSidebar({
  workspaceSlug,
  projectId,
  issue,
  editable,
  client = getAppClient(),
}: IssuePropertiesSidebarProps): React.ReactElement {
  const project = useQuery(pickerProjectQueryOptions(client, workspaceSlug, projectId));
  if (project.data === undefined) {
    if (project.isError) {
      return (
        <div className="flex flex-col items-start gap-(--space-2)" role="alert">
          <p className="text-meta text-(--text-muted)">{PROPERTIES_LOAD_FAILED}</p>
          <Button variant="secondary" size="small" onClick={() => void project.refetch()}>
            Try again
          </Button>
        </div>
      );
    }
    return <SidebarPropertiesSkeleton />;
  }
  return (
    <SidebarProperties
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      issue={issue}
      project={project.data}
      editable={editable}
    />
  );
}
