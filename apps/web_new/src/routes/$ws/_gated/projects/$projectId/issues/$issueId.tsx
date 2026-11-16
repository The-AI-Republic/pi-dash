// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy short-link route (parity/specs/issues.md §13, ISS-143).
// Loader-only: the project and issue ids resolve through the issue
// meta to the canonical browse key and redirect there. Unknown,
// deleted, archived, and refused issues all land on one generic
// missing state with a link back to the project list.

import { createFileRoute, Link, redirect } from "@tanstack/react-router";
import { Spinner } from "@pidash/kit";
import * as React from "react";

// Deep import: the barrel is eager through the router tree and would
// pin the whole issues feature here (NEWFRONT-871, size gate).
import { DetailMissingState } from "../../../../../../features/issues/detail/DetailMissingState.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/issues/$issueId")({
  // NOTE (NEWFRONT-852, size gate): the short-link resolve stays behind a
  // dynamic import so the browse api modules never join the initial
  // bundle — this loader only runs on short-link navigation, after first
  // paint. The loader still resolves before render, as before.
  loader: async ({ context, params }) => {
    const { resolveShortLink } = await import("../../../../../../features/issues/api/browse.js");
    const target = await resolveShortLink(context.apiClient, params.ws, params.projectId, params.issueId);
    if (target === null) return { missing: true };
    throw redirect({
      to: "/$ws/browse/$issueKey",
      params: { ws: params.ws, issueKey: target.issueKey },
    });
  },
  pendingComponent: function ShortLinkPending() {
    return (
      <div className="flex items-center gap-(--space-4) p-(--space-12)">
        <Spinner size="medium" label="Resolving work item link" />
        <p className="text-body text-(--text-muted)">Resolving the work item link…</p>
      </div>
    );
  },
  component: function ShortLinkMissing() {
    const params = Route.useParams();
    return (
      <DetailMissingState
        issueKey={params.issueId}
        body="It may have been archived or deleted, or the address is wrong."
      >
        <Link
          to="/$ws/projects/$projectId/issues"
          params={{ ws: params.ws, projectId: params.projectId }}
          className="text-body font-medium text-(--accent) hover:underline focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-1 focus-visible:outline-none"
        >
          Back to the project issues
        </Link>
      </DetailMissingState>
    );
  },
});
