// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-scoped detail surface for the browse route (SHELL-106). Thin
// shell-owned wrapper: the key, the name, the archived state and a way
// back into the project. Rich detail (threads, fields, activity) belongs
// to the issues area, which reuses this mount point.

import { Link } from "@tanstack/react-router";
import * as React from "react";
import type { Issue } from "@pidash/api-client";

export interface BrowseDetailProps {
  issue: Issue;
  issueKey: string;
  workspaceSlug: string;
  projectId: string;
}

export function BrowseDetail({ issue, issueKey, workspaceSlug, projectId }: BrowseDetailProps): React.ReactElement {
  const archived = issue.archived_at !== null;
  return (
    <section aria-label="Work item detail" className="flex min-h-0 flex-1 flex-col gap-(--space-2) p-(--space-8)">
      <p className="text-body text-(--text-muted)">{issueKey}</p>
      <h1 className="text-h2 text-(--text)">{issue.name}</h1>
      {archived ? <p className="text-body text-(--text-muted)">This work item is archived.</p> : null}
      <Link
        to="/$ws/projects/$projectId/issues"
        params={{ ws: workspaceSlug, projectId }}
        search={{}}
        className="text-body text-(--accent)"
      >
        Back to the project
      </Link>
    </section>
  );
}

export function BrowseUnknownKey({ issueKey }: { issueKey: string }): React.ReactElement {
  return (
    <section
      aria-label="Work item detail"
      className="flex min-h-0 flex-1 flex-col items-center gap-(--space-2) p-(--space-12)"
    >
      <h1 className="text-h3 text-(--text)">No work item matches this address</h1>
      <p className="text-body text-(--text-muted)">
        Nothing in this workspace is filed under {issueKey}. Check the key and try again.
      </p>
    </section>
  );
}
