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

import { useMe, useWorkspace } from "../../../../core/session/index.js";
// Deep imports: this route module is eager in the router tree, so the
// issues barrel here would put every export it re-exports on the initial
// chain (NEWFRONT-732; NEWFRONT-871, size gate).
import { issueKeys } from "../../../../features/issues/api/keys.js";
import { liteIssueQueryOptions } from "../../../../features/issues/api/browse.js";
import { issueDetailQueryOptions } from "../../../../features/issues/api/queries.js";
import { CommentThread } from "../../../../features/issues/comments/CommentThread.js";
import { DetailMissingState } from "../../../../features/issues/detail/DetailMissingState.js";
import { DetailSkeleton } from "../../../../features/issues/detail/DetailSkeleton.js";
import { IssueDetailRoot } from "../../../../features/issues/detail/IssueDetailRoot.js";
import { IssuePropertiesSidebar } from "../../../../features/issues/detail/sidebar/index.js";
import { SubIssuesWidget } from "../../../../features/issues/subIssues/SubIssuesWidget.js";
import { IssueRunAIAction } from "../../../../features/issues/detail/IssueAgentActions.js";
import { AgentStatusPanel } from "../../../../features/issues/detail/agent/AgentStatusPanel.js";
import { DetailHeaderActions } from "../../../../features/issues/detail/DetailHeaderActions.js";
import {
  AttachmentsWidget,
  type AttachmentsWidgetHandle,
} from "../../../../features/issues/detail/widgets/attachments/AttachmentsWidget.js";
import { CodeReviewsWidget } from "../../../../features/issues/detail/widgets/codeReviews/CodeReviewsWidget.js";
import { mayDeleteWorkItem } from "../../../../features/issues/lifecycle/rules.js";
import { useQuickActions } from "../../../../features/issues/menus/useQuickActions.js";
import {
  LinkActionButton,
  LinksWidget,
  RelationActionMenu,
  RelationsWidget,
  WidgetActionsProvider,
} from "../../../../features/issues/widgets/index.js";
import { projectDetailsQueryOptions, ProjectTabStrip } from "../../../../features/projects/index.js";
import { canEditInProject, useWorkItemPaletteContext } from "../../../../shared/shell/contextual/index.js";
import { useDocumentHead } from "../../../../shared/shell/index.js";

/** The project's code; the browse key's own prefix until the row joins. */
export function projectIdentifier(project: { identifier: string } | undefined, issueKey: string): string {
  return project?.identifier ?? issueKey.slice(0, Math.max(0, issueKey.lastIndexOf("-")));
}

export const Route = createFileRoute("/$ws/_gated/browse/$issueKey")({
  // NOTE (NEWFRONT-852, size gate): the lite-identifier read stays behind
  // a dynamic import so the browse api modules never join the initial
  // bundle — this loader only runs on browse navigation, after first
  // paint. The loader still resolves before render, as before.
  loader: async ({ context, params }) => {
    const { liteIssueQueryOptions: liteOptions } = await import("../../../../features/issues/api/browse.js");
    const lite = await context.queryClient.ensureQueryData(liteOptions(context.apiClient, params.ws, params.issueKey));
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
  const { workspace } = useWorkspace(workspaceSlug);
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
  // One detail re-read for the palette, the header, and the agent panel.
  const refreshDetail = React.useCallback(
    () => context.queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) }),
    [context.queryClient, issueId, workspaceSlug]
  );
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
    mayDelete:
      project?.archived_at == null &&
      mayDeleteWorkItem({
        viewerId: me.data?.id ?? null,
        creatorId: issue.created_by,
        projectRole: project?.member_role ?? null,
      }),
    refresh: refreshDetail,
    onDeleted: () => void navigate({ to: "/$ws/projects/$projectId/issues", params: { ws: workspaceSlug, projectId } }),
  });
  const writable = editable && issue.archived_at === null && !issue.is_synced;
  const identifier = projectIdentifier(project, issueKey);
  const menu = useQuickActions(
    issue.is_synced
      ? null
      : {
          workspaceSlug,
          target: {
            id: issue.id,
            name: issue.name,
            sequenceId: issue.sequence_id,
            projectId,
            projectIdentifier: identifier,
            stateId: issue.state_id,
            snapshot: issue,
            createdBy: issue.created_by ?? null,
            archived: issue.archived_at !== null,
            isEpic: issue.is_epic ?? false,
          },
          context: "detail",
          canWrite: editable,
          onRestored: () => void navigate({ to: "/$ws/browse/$issueKey", params: { ws: workspaceSlug, issueKey } }),
          onArchived: () =>
            void navigate({
              to: "/$ws/projects/$projectId/issues",
              params: { ws: workspaceSlug, projectId },
            }),
          onDeleted: () => {
            if (issue.archived_at !== null) {
              void navigate({
                to: "/$ws/projects/$projectId/archives/issues",
                params: { ws: workspaceSlug, projectId },
              });
            } else {
              void navigate({
                to: "/$ws/projects/$projectId/issues",
                params: { ws: workspaceSlug, projectId },
              });
            }
          },
        }
  );
  const attachmentsRef = React.useRef<AttachmentsWidgetHandle>(null);
  const widgetSections = [
    {
      label: "Relations" as const,
      action: <RelationActionMenu canEdit={writable} />,
      node: (
        <RelationsWidget
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          issueId={issue.id}
          projectIdentifier={projectIdentifier(project, issueKey)}
          canEdit={writable}
        />
      ),
    },
    {
      label: "Links" as const,
      action: <LinkActionButton canEdit={writable} />,
      node: <LinksWidget workspaceSlug={workspaceSlug} projectId={projectId} issueId={issue.id} canEdit={writable} />,
    },
    {
      label: "Attachments" as const,
      node: (
        <AttachmentsWidget
          ref={attachmentsRef}
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          issueId={issue.id}
          editable={editable}
        />
      ),
    },
  ];
  const workbench = (
    <WidgetActionsProvider>
      <SubIssuesWidget
        workspaceSlug={workspaceSlug}
        issue={issue}
        project={project}
        editable={editable}
        projectIdentifier={projectIdentifier(project, issueKey)}
        canWrite={editable}
        onAttach={() => attachmentsRef.current?.openPicker()}
        runAIAction={
          <IssueRunAIAction
            workspaceSlug={workspaceSlug}
            workspaceId={workspace?.id ?? null}
            projectId={projectId}
            issue={issue}
            disabled={!writable}
            label="Manually Run AI"
            size="medium"
          />
        }
        onOpen={(row, identifier) =>
          void navigate({
            to: "/$ws/browse/$issueKey",
            params: { ws: workspaceSlug, issueKey: identifier },
          })
        }
        extraSections={widgetSections}
        createProjects={
          project === undefined
            ? []
            : [
                {
                  id: project.id,
                  name: project.name,
                  identifier: project.identifier,
                  memberRole: project.member_role,
                  archived: project.archived_at !== null,
                },
              ]
        }
      />
    </WidgetActionsProvider>
  );
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
          <DetailHeaderActions
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            issue={issue}
            issueKey={issueKey}
            projectIdentifier={identifier}
            editable={editable}
            menu={menu}
            onSubscriptionChanged={refreshDetail}
          />
        }
        activity={
          <div className="flex flex-col gap-(--space-6)">
            {workbench}
            <CodeReviewsWidget
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              issueId={issue.id}
              editable={editable}
            />
            <CommentThread
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              issueId={issue.id}
              archived={issue.archived_at !== null}
              published={project?.anchor != null}
              assigneeIds={issue.assignee_ids}
              memberRole={project?.member_role ?? null}
              commentAndRun={{ workspaceId: workspace?.id ?? null, disabled: !writable }}
            />
          </div>
        }
        sidebar={
          <>
            <AgentStatusPanel workspaceSlug={workspaceSlug} issue={issue} refresh={refreshDetail} />
            <IssuePropertiesSidebar
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              issue={issue}
              editable={editable}
            />
          </>
        }
      />
      {palette.deleteDialog}
    </>
  );
}
