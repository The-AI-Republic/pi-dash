// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Project issue list (first vertical slice). Thin: the single search
// param validated by zod, a loader that prefetches in parallel with the
// code split, and the read-only list. Layouts, filters and detail arrive
// with NEWFRONT-71.

import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Button } from "@pidash/kit";
import * as React from "react";

import { invalidateWorkspace } from "../../../../../core/query/optimistic.js";
import { IssueList, issuesQueryOptions, parseIssueSearch } from "../../../../../features/issues/index.js";
import { projectsQueryOptions } from "../../../../../features/projects/index.js";
import { registerCommands } from "../../../../../shared/commands/index.js";
import { notifyError } from "../../../../../shared/shell/index.js";

export const Route = createFileRoute("/$ws/projects/$projectId/issues/")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params, search }) => {
    await context.queryClient.ensureQueryData(
      issuesQueryOptions(context.apiClient, params.ws, params.projectId, search)
    );
  },
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
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

    return (
      <section aria-label="Issue list" className="flex h-full flex-col">
        <div className="flex shrink-0 items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-3)">
          <h2 className="text-h3 text-(--text)">{project.name}</h2>
          <span className="text-body text-(--text-muted)">
            {issues.total_count} issue{issues.total_count === 1 ? "" : "s"}
          </span>
        </div>
        <div className="min-h-0 flex-1">
          <IssueList issues={issues.results} projectIdentifier={project.identifier} layout={search.layout ?? "list"} />
        </div>
      </section>
    );
  },
});
