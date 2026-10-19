// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-level browse route (SHELL-106, SHELL-076, SHELL-090;
// parity/specs/issues.md §13, ISS-142/144). The key in the route
// resolves in two phases: the lite identifier fetch answers for live,
// archived, draft, and intake items alike, then the full detail
// hydrates behind it. Intake items leave for the project intake tab;
// unknown keys render an explicit empty state, never a workspace-wide
// browser. Resolved issues render under their project's tab strip,
// with the tracking tab highlighted, and register the work-item
// contextual palette group while mounted.

import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Link, Navigate, redirect, useNavigate } from "@tanstack/react-router";
import { Button } from "@pidash/kit";
import * as React from "react";

import { useMe } from "../../../../core/session/index.js";
import {
  CommentThread,
  DetailMissingState,
  DetailSkeleton,
  IssueDetailRoot,
  issueDetailQueryOptions,
  issueKeys,
  liteIssueQueryOptions,
} from "../../../../features/issues/index.js";
import { projectDetailsQueryOptions, ProjectTabStrip } from "../../../../features/projects/index.js";
import { canEditInProject, useWorkItemPaletteContext } from "../../../../shared/shell/contextual/index.js";
import { CopyWorkItemLinkButton } from "../../../../shared/shell/CopyWorkItemLinkButton.js";
import { useDocumentHead } from "../../../../shared/shell/index.js";

export const Route = createFileRoute("/$ws/_gated/browse/$issueKey")({
  loader: async ({ context, params }) => {
    const lite = await context.queryClient.ensureQueryData(
      liteIssueQueryOptions(context.apiClient, params.ws, params.issueKey)
    );
    if (lite.lite?.is_intake === true) {
      throw redirect({
        to: "/$ws/projects/$projectId/intake",
        params: { ws: params.ws, projectId: lite.lite.project_id },
        search: { currentTab: "open", inboxIssueId: lite.lite.id },
      });
    }
    if (lite.lite !== null) {
      await context.queryClient.ensureQueryData(
        issueDetailQueryOptions(context.apiClient, params.ws, lite.lite.project_id, lite.lite.id)
      );
    }
  },
  pendingComponent: function BrowsePending() {
    return <DetailSkeleton />;
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
    const { data } = useSuspenseQuery(liteIssueQueryOptions(context.apiClient, params.ws, params.issueKey));
    if (data.lite === null) {
      return (
        <DetailMissingState issueKey={params.issueKey}>
          <Link
            to="/$ws"
            params={{ ws: params.ws }}
            className="text-body font-medium text-(--accent) hover:underline focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-1 focus-visible:outline-none"
          >
            Back to the workspace home
          </Link>
        </DetailMissingState>
      );
    }
    if (data.lite.is_intake) {
      // The loader redirects first; this backstop covers renders that
      // resolve intake rows from cache without running it.
      return (
        <Navigate
          to="/$ws/projects/$projectId/intake"
          params={{ ws: params.ws, projectId: data.lite.project_id }}
          search={{ currentTab: "open", inboxIssueId: data.lite.id }}
        />
      );
    }
    return (
      <BrowseIssueDetail
        workspaceSlug={params.ws}
        projectId={data.lite.project_id}
        issueKey={params.issueKey}
        issueId={data.lite.id}
      />
    );
  },
});

function BrowseIssueDetail({
  workspaceSlug,
  projectId,
  issueKey,
  issueId,
}: {
  workspaceSlug: string;
  projectId: string;
  issueKey: string;
  issueId: string;
}): React.ReactElement {
  const context = Route.useRouteContext();
  const navigate = useNavigate();
  const me = useMe();
  const { data: issue } = useSuspenseQuery(
    issueDetailQueryOptions(context.apiClient, workspaceSlug, projectId, issueId)
  );
  const projectQuery = useQuery(projectDetailsQueryOptions(context.apiClient, workspaceSlug));
  const project = projectQuery.data?.find((row) => row.id === projectId);
  useDocumentHead({ title: `${issueKey} ${issue.name}`, robots: "noindex, nofollow" });
  const editable = canEditInProject({
    memberRole: project?.member_role ?? null,
    archived: project?.archived_at != null,
  });
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
    refresh: async () => context.queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) }),
    onDeleted: () => void navigate({ to: "/$ws/projects/$projectId/issues", params: { ws: workspaceSlug, projectId } }),
  });
  return (
    <>
      <ProjectTabStrip
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        workItem={{ id: issue.id, projectId, isEpic: issue.is_epic ?? false }}
      />
      <IssueDetailRoot
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issue={issue}
        editable={editable}
        topSlot={
          <div className="flex items-center gap-(--space-2)">
            <span className="text-meta font-medium text-(--text-muted)">{issueKey}</span>
            <CopyWorkItemLinkButton workspaceSlug={workspaceSlug} issueKey={issueKey} />
          </div>
        }
        belowDescription={
          <CommentThread
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            issueId={issue.id}
            archived={issue.archived_at !== null}
            published={project?.anchor != null}
          />
        }
      />
      {palette.deleteDialog}
    </>
  );
}
