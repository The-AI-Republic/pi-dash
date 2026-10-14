// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-level browse route (SHELL-106, SHELL-076). The key in the
// route resolves to the project-scoped detail through workspace search;
// unknown keys render an explicit empty state, never a workspace-wide
// browser. Resolved issues render under their project's tab strip, with
// the tracking tab highlighted.

import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Button } from "@pidash/kit";

import { BrowseDetail, BrowseUnknownKey, browseIssueQueryOptions } from "../../../../features/issues/index.js";
import { ProjectTabStrip } from "../../../../features/projects/index.js";

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
      <>
        <ProjectTabStrip
          workspaceSlug={params.ws}
          projectId={data.projectId}
          workItem={{ id: data.issue.id, projectId: data.projectId, isEpic: data.issue.is_epic ?? false }}
        />
        <BrowseDetail
          issue={data.issue}
          issueKey={params.issueKey}
          workspaceSlug={params.ws}
          projectId={data.projectId}
        />
      </>
    );
  },
});
