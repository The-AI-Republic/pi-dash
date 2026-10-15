// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-level browse route (SHELL-106, SHELL-076, SHELL-090). The key
// in the route resolves to the project-scoped detail through workspace
// search; unknown keys render an explicit empty state, never a
// workspace-wide browser. Resolved issues render under their project's
// tab strip, with the tracking tab highlighted, and register the
// work-item contextual palette group while mounted.

import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Button } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import * as React from "react";

import { useMe } from "../../../../core/session/index.js";
import {
  BrowseDetail,
  BrowseUnknownKey,
  browseIssueQueryOptions,
  issueKeys,
} from "../../../../features/issues/index.js";
import { projectDetailsQueryOptions, ProjectTabStrip } from "../../../../features/projects/index.js";
import { useWorkItemPaletteContext } from "../../../../shared/shell/contextual/index.js";

export const Route = createFileRoute("/$ws/_gated/browse/$issueKey")({
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(browseIssueQueryOptions(context.apiClient, params.ws, params.issueKey));
  },
  errorComponent: function BrowseError({ error, reset }: { error: unknown; reset: () => void }) {
    return (
      <section aria-label="Work item detail" className="flex flex-col items-center gap-(--space-4) p-(--space-12)">
        <h1 className="text-h3 text-(--text)">Could not load this work item</h1>
        <p className="text-body text-(--text-muted)">
          {error instanceof Error ? error.message : "Something went wrong."}
        </p>
        <Button variant="primary" onClick={reset}>
          Try again
        </Button>
      </section>
    );
  },
  component: function BrowseIssueScreen() {
    const params = Route.useParams();
    const context = Route.useRouteContext();
    const { data } = useSuspenseQuery(browseIssueQueryOptions(context.apiClient, params.ws, params.issueKey));
    if (data.issue === null || data.projectId === null) {
      return <BrowseUnknownKey issueKey={params.issueKey} />;
    }
    return (
      <BrowseIssueDetail
        workspaceSlug={params.ws}
        projectId={data.projectId}
        issueKey={params.issueKey}
        issue={data.issue}
      />
    );
  },
});

function BrowseIssueDetail({
  workspaceSlug,
  projectId,
  issueKey,
  issue,
}: {
  workspaceSlug: string;
  projectId: string;
  issueKey: string;
  issue: Issue;
}): React.ReactElement {
  const context = Route.useRouteContext();
  const navigate = useNavigate();
  const me = useMe();
  const projectQuery = useQuery(projectDetailsQueryOptions(context.apiClient, workspaceSlug));
  const project = projectQuery.data?.find((row) => row.id === projectId);
  const palette = useWorkItemPaletteContext({
    workspaceSlug,
    projectId,
    issueId: issue.id,
    issueKey,
    issueName: issue.name,
    stateId: issue.state_id,
    priority: issue.priority,
    assigneeIds: issue.assignee_ids,
    labelIds: issue.label_ids,
    estimatePoint: issue.estimate_point,
    cycleId: issue.cycle_id,
    moduleIds: issue.module_ids,
    isEpic: issue.is_epic ?? false,
    parentId: issue.parent_id,
    archived: issue.archived_at !== null,
    estimatesEnabled: project?.estimate !== null && project?.estimate !== undefined,
    cycleView: project?.cycle_view ?? false,
    moduleView: project?.module_view ?? false,
    meId: me.data?.id ?? null,
    standing: { memberRole: project?.member_role ?? null, archived: project?.archived_at != null },
    refresh: async () => context.queryClient.invalidateQueries({ queryKey: issueKeys.browse(workspaceSlug, issueKey) }),
    onDeleted: () => void navigate({ to: "/$ws/projects/$projectId/issues", params: { ws: workspaceSlug, projectId } }),
  });
  return (
    <>
      <ProjectTabStrip
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        workItem={{ id: issue.id, projectId, isEpic: issue.is_epic ?? false }}
      />
      <BrowseDetail issue={issue} issueKey={issueKey} workspaceSlug={workspaceSlug} projectId={projectId} />
      {palette.deleteDialog}
    </>
  );
}
