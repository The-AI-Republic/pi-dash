// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Full work-item peek (parity/specs/issues.md §14, ISS-165–172). The
// shared chrome plus the whole detail composition: the header actions
// (H2/C1), the title and description column (H1) with its own save
// pipeline, the widget roots, and the properties sidebar (I1/I2). The
// panel fetches the full issue with a skeleton while loading and one
// generic empty state for every failure; display modes reshape it,
// closing clears the selection and refocuses the originating row, and
// opening a sub-issue row nests a child peek above with the nesting
// level written back to the URL.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { ArrowUpRight } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useWorkspace } from "../../../core/session/index.js";
import { canEditInProject } from "../../../shared/shell/contextual/index.js";
import { projectDetailsQueryOptions } from "../../projects/index.js";
import { issueKeys } from "../api/keys.js";
import { TITLE_SAVE_FAILED_TITLE, useUpdateIssue } from "../api/mutations.js";
import { issueDetailQueryOptions } from "../api/queries.js";
import type { SubIssueItem } from "@pidash/api-client";
import { CommentThread } from "../comments/CommentThread.js";
import { AgentStatusPanel } from "../detail/agent/AgentStatusPanel.js";
import { DetailHeaderActions } from "../detail/DetailHeaderActions.js";
import { DetailMainColumn } from "../detail/DetailMainColumn.js";
import { IssueRunAIAction } from "../detail/IssueAgentActions.js";
import { ParentBanner } from "../detail/ParentBanner.js";
import { useSavePipeline } from "../detail/savePipeline.js";
import { IssuePropertiesSidebar } from "../detail/sidebar/index.js";
import { AttachmentsWidget, type AttachmentsWidgetHandle } from "../detail/widgets/attachments/AttachmentsWidget.js";
import { CodeReviewsWidget } from "../detail/widgets/codeReviews/CodeReviewsWidget.js";
import { useQuickActions } from "../menus/useQuickActions.js";
import { SubIssuesWidget } from "../subIssues/SubIssuesWidget.js";
import {
  LinkActionButton,
  LinksWidget,
  RelationActionMenu,
  RelationsWidget,
  WidgetActionsProvider,
} from "../widgets/index.js";
import { PeekChrome } from "./PeekChrome.js";
import type { PeekMode } from "./PeekModeToggle.js";
import { PeekErrorPanel, PeekLoadingPanel } from "./peekStates.js";
import { nestingSearchFor } from "./peekUrl.js";

export { PEEK_LOADING_LABEL, PEEK_MISSING_BODY, PEEK_MISSING_TITLE } from "./peekStates.js";

/** Nested panels carry their own test handle; the top panel keeps the default. */
export const NESTED_PEEK_TEST_ID = "issue-preview-nested";

export interface IssuePeekProps {
  workspaceSlug: string;
  /** Project the detail reads from (the peeked or route project). */
  projectId: string;
  /** Route project code; the key display falls back to it. */
  projectIdentifier: string;
  issueId: string;
  /** Opener-known list key; derived after load when null (deep links). */
  issueKey: string | null;
  /** Nesting depth; 0 at the top. */
  level?: number | undefined;
  /** Presentation the panel opens in; nested peeks inherit their parent. */
  initialMode?: PeekMode | undefined;
  /** Embedded hosts disable Escape and outside-click close. */
  embedded?: boolean | undefined;
  onClose: () => void;
}

/** List key plus project code behind the header links and menus. */
export interface PeekKey {
  key: string;
  identifier: string;
}

/**
 * Resolve the project code behind the header: the loaded project wins,
 * then the opener key's prefix, then the route fallback.
 */
export function peekIdentifierOf(
  issueKey: string | null,
  projectCode: string | undefined,
  fallbackCode: string
): string {
  if (projectCode !== undefined) return projectCode;
  if (issueKey !== null) return issueKey.slice(0, Math.max(0, issueKey.lastIndexOf("-")));
  return fallbackCode;
}

/**
 * Resolve the display key: the opener's key wins, otherwise the loaded
 * project code (or the route fallback) plus the sequence.
 */
export function peekKeyOf(
  issueKey: string | null,
  projectCode: string | undefined,
  fallbackCode: string,
  sequenceId: number
): PeekKey {
  const identifier = peekIdentifierOf(issueKey, projectCode, fallbackCode);
  return { key: issueKey ?? `${identifier}-${sequenceId}`, identifier };
}

/**
 * The key the open-full-page control may link to, or null while it is
 * still unresolved (ISS-169): the opener's key, or the sequence under a
 * loaded project code. The route fallback is a guess that can point at
 * another project, so it never enables navigation by itself.
 */
export function peekLinkKeyOf(
  issueKey: string | null,
  projectCode: string | undefined,
  sequenceId: number
): string | null {
  if (issueKey !== null) return issueKey;
  return projectCode === undefined ? null : `${projectCode}-${sequenceId}`;
}

/** Clicks that keep the peek up: modified clicks and non-primary buttons. */
export function keepPeekOnClick(event: {
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  button: number;
}): boolean {
  return event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0;
}

/**
 * Returns focus to the opener on unmount: the row focused when the peek
 * mounted, still connected (ISS-168). Deep-link opens capture the body,
 * which focusing leaves alone.
 */
function useRefocusOrigin(): void {
  const originRef = React.useRef<HTMLElement | null | undefined>(undefined);
  if (originRef.current === undefined) {
    const active = document.activeElement;
    originRef.current = active instanceof HTMLElement ? active : null;
  }
  React.useEffect(
    () => () => {
      const origin = originRef.current;
      if (origin !== null && origin !== undefined && origin.isConnected) origin.focus({ preventScroll: true });
    },
    []
  );
}

interface NestedTarget {
  issueId: string;
  projectId: string;
  issueKey: string;
}

export function IssuePeek({
  workspaceSlug,
  projectId,
  projectIdentifier,
  issueId,
  issueKey,
  level = 0,
  initialMode = "side",
  embedded = false,
  onClose,
}: IssuePeekProps): React.ReactElement {
  const client = getAppClient();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const search = useSearch({ strict: false });
  const [mode, setMode] = React.useState<PeekMode>(initialMode);
  const [nested, setNested] = React.useState<NestedTarget | null>(null);
  const pipeline = useSavePipeline();
  const detail = useQuery(issueDetailQueryOptions(client, workspaceSlug, projectId, issueId));
  const { workspace } = useWorkspace(workspaceSlug);
  const projectQuery = useQuery(projectDetailsQueryOptions(client, workspaceSlug));
  const project = projectQuery.data?.find((row) => row.id === projectId);
  const titleUpdate = useUpdateIssue({ workspaceSlug, projectId, issueId, failureTitle: TITLE_SAVE_FAILED_TITLE });
  const attachmentsRef = React.useRef<AttachmentsWidgetHandle>(null);
  useRefocusOrigin();

  const refreshDetail = React.useCallback(
    () => queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) }),
    [queryClient, issueId, workspaceSlug]
  );

  const writeNesting = React.useCallback(
    (next: number): void => {
      void navigate({ to: ".", search: nestingSearchFor(search, next), replace: true } as never);
    },
    [navigate, search]
  );

  const issue = detail.data ?? null;
  const menu = useQuickActions(
    issue === null || issue.is_synced
      ? null
      : {
          workspaceSlug,
          target: {
            id: issue.id,
            name: issue.name,
            sequenceId: issue.sequence_id,
            projectId,
            projectIdentifier: peekIdentifierOf(issueKey, project?.identifier, projectIdentifier),
            stateId: issue.state_id,
            snapshot: issue,
            createdBy: issue.created_by ?? null,
            archived: issue.archived_at !== null,
            isEpic: issue.is_epic ?? false,
          },
          context: "detail",
          canWrite:
            project !== undefined
              ? canEditInProject({ memberRole: project.member_role, archived: project.archived_at != null })
              : false,
          // Delete, archive, and restore all close the peek (ISS-169);
          // the hook already re-reads the lists behind it.
          onRestored: onClose,
          onArchived: onClose,
          onDeleted: onClose,
        }
  );

  const testId = level === 0 ? undefined : NESTED_PEEK_TEST_ID;
  const stacked = level > 0;

  if (detail.isPending) {
    return (
      <PeekLoadingPanel
        mode={mode}
        onClose={onClose}
        suppressEscape={nested !== null}
        embedded={embedded}
        stacked={stacked}
        {...(testId === undefined ? {} : { testId })}
      />
    );
  }

  if (detail.isError || issue === null) {
    return (
      <PeekErrorPanel
        mode={mode}
        onClose={onClose}
        suppressEscape={nested !== null}
        embedded={embedded}
        stacked={stacked}
        {...(testId === undefined ? {} : { testId })}
      />
    );
  }

  const editable = canEditInProject({
    memberRole: project?.member_role ?? null,
    archived: project?.archived_at != null,
  });
  const archived = issue.archived_at !== null;
  const readOnly = !editable || archived || issue.is_synced;
  const writable = editable && !archived && !issue.is_synced;
  const resolved = peekKeyOf(issueKey, project?.identifier, projectIdentifier, issue.sequence_id);
  const linkKey = peekLinkKeyOf(issueKey, project?.identifier, issue.sequence_id);

  const commitTitle = (next: string): void => {
    pipeline.markSaving();
    void titleUpdate.mutateAsync({ name: next }).then(
      () => pipeline.markSaved(),
      () => pipeline.markSaved()
    );
  };

  const widgetSections = [
    {
      label: "Relations" as const,
      action: <RelationActionMenu canEdit={writable} />,
      node: (
        <RelationsWidget
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          issueId={issue.id}
          projectIdentifier={resolved.identifier}
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

  const main = (
    <>
      <ParentBanner workspaceSlug={workspaceSlug} projectId={projectId} issue={issue} editable={editable} />
      <DetailMainColumn
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issue={issue}
        editable={editable}
        showTitle={false}
        pipeline={pipeline}
        activity={
          <div className="flex flex-col gap-(--space-6)">
            <WidgetActionsProvider>
              <SubIssuesWidget
                workspaceSlug={workspaceSlug}
                issue={issue}
                project={project}
                editable={editable}
                projectIdentifier={resolved.identifier}
                canWrite={editable}
                onOpen={(row: SubIssueItem, identifier: string) => {
                  setNested({ issueId: row.id, projectId: row.project_id, issueKey: identifier });
                  writeNesting(level + 1);
                }}
                extraSections={widgetSections}
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
              archived={archived}
              published={project?.anchor != null}
              assigneeIds={issue.assignee_ids}
              memberRole={project?.member_role ?? null}
              commentAndRun={{ workspaceId: workspace?.id ?? null, disabled: !writable }}
            />
          </div>
        }
      />
    </>
  );

  const sidebar = (
    <>
      <AgentStatusPanel workspaceSlug={workspaceSlug} issue={issue} refresh={refreshDetail} />
      <IssuePropertiesSidebar workspaceSlug={workspaceSlug} projectId={projectId} issue={issue} editable={editable} />
    </>
  );

  return (
    <>
      <PeekChrome
        mode={mode}
        onModeChange={setMode}
        title={issue.name}
        titleAsHeading={readOnly}
        {...(readOnly ? {} : { onCommitTitle: commitTitle })}
        onClose={onClose}
        suppressEscape={nested !== null}
        suppressOutsideClick={nested !== null}
        embedded={embedded}
        {...(testId === undefined ? {} : { testId })}
        stacked={stacked}
        actions={
          <>
            <DetailHeaderActions
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              issue={issue}
              issueKey={resolved.key}
              projectIdentifier={resolved.identifier}
              editable={editable}
              menu={menu}
              onSubscriptionChanged={refreshDetail}
            />
            {linkKey === null ? (
              <button
                type="button"
                disabled
                aria-label="Open full page"
                data-testid="peek-fullscreen-link"
                className="text-body inline-flex rounded-(--radius-control) p-(--space-2) text-(--text-muted) opacity-50"
              >
                <ArrowUpRight size={16} strokeWidth={1.5} aria-hidden="true" />
              </button>
            ) : (
              <Link
                to="/$ws/browse/$issueKey"
                params={{ ws: workspaceSlug, issueKey: linkKey }}
                data-testid="peek-fullscreen-link"
                aria-label="Open full page"
                className="text-body inline-flex rounded-(--radius-control) p-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
                onClick={(event) => {
                  // Modified clicks open a new tab and leave the peek up;
                  // a plain click clears the peek before navigating on.
                  if (keepPeekOnClick(event)) return;
                  event.preventDefault();
                  onClose();
                  void navigate({
                    to: "/$ws/browse/$issueKey",
                    params: { ws: workspaceSlug, issueKey: linkKey },
                  });
                }}
              >
                <ArrowUpRight size={16} strokeWidth={1.5} aria-hidden="true" />
              </Link>
            )}
          </>
        }
      >
        {mode === "full" ? (
          <div className="flex min-w-0 flex-1 flex-col gap-(--space-8) md:flex-row">
            <div className="flex min-w-0 flex-1 flex-col gap-(--space-6)">{main}</div>
            <aside aria-label="Properties" className="flex min-w-0 flex-col gap-(--space-4) md:w-80 md:shrink-0">
              <p className="text-meta font-medium text-(--text-muted)">Properties</p>
              {sidebar}
            </aside>
          </div>
        ) : (
          <div className="flex min-w-0 flex-1 flex-col gap-(--space-6)">
            {main}
            <section aria-label="Properties" className="flex min-w-0 flex-col gap-(--space-4)">
              <p className="text-meta font-medium text-(--text-muted)">Properties</p>
              {sidebar}
            </section>
          </div>
        )}
      </PeekChrome>
      {nested === null ? null : (
        <IssuePeek
          workspaceSlug={workspaceSlug}
          projectId={nested.projectId}
          projectIdentifier={projectIdentifier}
          issueId={nested.issueId}
          issueKey={nested.issueKey}
          level={level + 1}
          initialMode={mode}
          embedded={embedded}
          onClose={() => {
            setNested(null);
            writeNesting(level);
          }}
        />
      )}
    </>
  );
}
