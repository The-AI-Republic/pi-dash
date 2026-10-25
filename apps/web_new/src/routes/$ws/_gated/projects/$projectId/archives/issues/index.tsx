// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work-items tab route (parity/specs/archives.md §1–2,
// ARCH-001–007). Thin: the per-tab browser title, the shared shell and
// the read-only archived list with its ?peekIssueId= selection. Closing
// the peek strips every peek param; opening writes the item without
// pushing history.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { ArchivesShell } from "../../../../../../../features/archives/components/ArchivesShell.js";
import { archivesRouteTitle } from "../../../../../../../features/archives/tabs.js";
import { useProjectRow } from "../../../../../../../features/auth/queries.js";
import { ArchivedIssuesScreen } from "../../../../../../../features/issues/contexts/ArchivedIssuesScreen.js";
import { usePeekResetOnProjectChange } from "../../../../../../../features/issues/contexts/useProjectScopeReset.js";
import { parseIssueSearch } from "../../../../../../../features/issues/filters/search.js";
import { useDocumentHead } from "../../../../../../../shared/shell/document.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/archives/issues/")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  component: ArchivedIssuesTab,
});

export function ArchivedIssuesTab(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const project = useProjectRow(ws, projectId);
  useDocumentHead({
    title: archivesRouteTitle(project.data?.name, "issues"),
    robots: "noindex, nofollow",
  });
  const setPeek = (issueId: string | null): void => {
    const next = { ...search };
    if (issueId === null) {
      delete next.peekIssueId;
      delete next.peekProjectId;
      delete next.peekNestingLevel;
    } else {
      next.peekIssueId = issueId;
      delete next.peekProjectId;
      delete next.peekNestingLevel;
    }
    void navigate({ to: ".", search: next, replace: true } as never);
  };
  // The tab body and its stored expression belong to one project. A
  // project switch re-mounts the body and drops any selection carried
  // over from the previous project's address (spec §2.2/§2.5).
  const scope = `${ws}/${projectId}`;
  usePeekResetOnProjectChange(scope, search, (next) => {
    void navigate({ to: ".", search: next, replace: true } as never);
  });
  return (
    <ArchivesShell workspaceSlug={ws} projectId={projectId} activeTab="issues">
      <React.Suspense fallback={null}>
        <ArchivedIssuesScreen
          key={scope}
          workspaceSlug={ws}
          projectId={projectId}
          peekIssueId={search.peekIssueId ?? null}
          onPeekChange={setPeek}
        />
      </React.Suspense>
    </ArchivesShell>
  );
}
