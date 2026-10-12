// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-level browse route (SHELL-106). The key in the route resolves
// to the project-scoped detail through workspace search; unknown keys
// render an explicit empty state, never a workspace-wide browser.

import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Button } from "@pidash/kit";

import { BrowseDetail, BrowseUnknownKey, browseIssueQueryOptions } from "../../../features/issues/index.js";

export const Route = createFileRoute("/$ws/browse/$issueKey")({
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
      <BrowseDetail
        issue={data.issue}
        issueKey={params.issueKey}
        workspaceSlug={params.ws}
        projectId={data.projectId}
      />
    );
  },
});
