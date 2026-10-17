// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project issues (parity/specs/issues.md §1 and §9, ISS-001–006 and
// ISS-075–081/ISS-106). Thin: the single search param validated by zod,
// a loader that prefetches issues plus display preferences in parallel
// with the code split, and the per-entity layout root with the Display
// control in both headers. Layout slices extend the offered list with
// their renderer; context empty-state copy arrives with ISS-068.

import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Button } from "@pidash/kit";
import * as React from "react";

import { invalidateWorkspace } from "../../../../../../core/query/optimistic.js";
import { isProjectGateError } from "../../../../../../features/auth/index.js";
import {
  DisplayControl,
  IssueList,
  IssuesLayoutRoot,
  ListSkeleton,
  issuesQueryOptions,
  parseIssueSearch,
  prefsQueryOptions,
  type LayoutKind,
} from "../../../../../../features/issues/index.js";
import { projectsQueryOptions } from "../../../../../../features/projects/index.js";
import { registerCommands } from "../../../../../../shared/commands/index.js";
import { notifyError } from "../../../../../../shared/shell/index.js";

function ProjectIssuesEmptyState(): React.ReactElement {
  return (
    <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
      <h2 className="text-h3 text-(--text)">No issues yet</h2>
      <p className="text-body text-(--text-muted)">New issues in this project will appear here.</p>
    </section>
  );
}

function ProjectIssuesNoMatchState(): React.ReactElement {
  return (
    <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
      <h2 className="text-h3 text-(--text)">No matching issues</h2>
      <p className="text-body text-(--text-muted)">No issues match the current filters.</p>
    </section>
  );
}

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/issues/")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params, search }) => {
    try {
      await Promise.all([
        context.queryClient.ensureQueryData(issuesQueryOptions(context.apiClient, params.ws, params.projectId, search)),
        context.queryClient.ensureQueryData(
          prefsQueryOptions(context.apiClient, params.ws, { scope: "project", projectId: params.projectId })
        ),
      ]);
    } catch (error) {
      // Denial statuses belong to the project gate (AUTH-024): let them
      // through so the gate renders the denial screen instead of the load
      // error.
      if (!isProjectGateError(error)) throw error;
    }
  },
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
  },
  // The stored layout is unknown while the prefetch runs (it is fetched,
  // not in the URL), so the pending window always shows the list
  // placeholder; the layout renders its own skeleton once mounted.
  pendingComponent: function IssuesPending() {
    return <ListSkeleton />;
  },
  errorComponent: function IssueListError({ error, reset }: { error: unknown; reset: () => void }) {
    return (
      <section aria-label="Issues" className="flex flex-col items-center gap-(--space-4) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Could not load issues</h2>
        <p className="text-body text-(--text-muted)">
          {error instanceof Error ? error.message : "Something went wrong."}
        </p>
        <Button variant="primary" onClick={reset}>
          Try again
        </Button>
      </section>
    );
  },
  component: function IssueListScreen() {
    const params = Route.useParams();
    const search = Route.useSearch();
    const context = Route.useRouteContext();
    const queryClient = context.queryClient;

    const { data: projects } = useSuspenseQuery(projectsQueryOptions(context.apiClient, params.ws));
    const project = projects.find((entry) => entry.id === params.projectId) ?? null;

    const { data: issues } = useSuspenseQuery(
      issuesQueryOptions(context.apiClient, params.ws, params.projectId, search)
    );

    React.useEffect(
      () =>
        registerCommands([
          {
            id: "issues.refresh-list",
            title: "Refresh issue list",
            hint: project?.name ?? params.projectId,
            section: "Issues",
            run: () => {
              void invalidateWorkspace(queryClient, params.ws).catch(() => {
                notifyError("Could not refresh issues", "Check your connection and try again.");
              });
            },
          },
        ]),
      [queryClient, params.ws, params.projectId, project?.name]
    );

    if (!project) {
      return (
        <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
          <h2 className="text-h3 text-(--text)">Project not found</h2>
          <p className="text-body text-(--text-muted)">Pick another project from the sidebar.</p>
        </section>
      );
    }

    // Layout slices extend the offered list with their renderer; the
    // Display control resolves the same active layout from it.
    const offered: readonly LayoutKind[] = ["list"];
    const display = (
      <DisplayControl
        workspaceSlug={params.ws}
        entity={{ scope: "project", projectId: params.projectId }}
        offered={offered}
        hideCycle={!project.cycle_view}
        hideModule={!project.module_view}
      />
    );
    return (
      <IssuesLayoutRoot
        workspaceSlug={params.ws}
        entity={{ scope: "project", projectId: params.projectId }}
        title={project.name}
        totalCount={issues.total_count}
        resultCount={issues.results.length}
        offered={offered}
        renderLayout={() => (
          <IssueList issues={issues.results} projectIdentifier={project.identifier} layout={search.layout ?? "list"} />
        )}
        emptyState={<ProjectIssuesEmptyState />}
        noMatchState={<ProjectIssuesNoMatchState />}
        headerActions={display}
        displayControl={display}
      />
    );
  },
});
