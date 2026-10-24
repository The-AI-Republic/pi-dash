// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Resolving peek mount (parity/specs/issues.md §14, ISS-165/167). The
// layouts mount this with the raw peeked value: a list key resolves
// against the rows the opener holds, a raw id reads under the peeked
// (or route) project, and an unmatched key resolves through the lite
// read first — so deep links open whatever the list has loaded. The
// lite wait and every failure reuse the shared status panels.

import { useQuery } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { liteIssueQueryOptions } from "../api/browse.js";
import { IssuePeek } from "./IssuePeek.js";
import { PeekErrorPanel, PeekLoadingPanel } from "./peekStates.js";
import { resolvePeekTarget } from "./peekTarget.js";

export interface ResolvingIssuePeekProps {
  workspaceSlug: string;
  /** Project of the mounting route. */
  projectId: string;
  /** Code the opener keyed its rows with. */
  projectIdentifier: string;
  /** The raw peeked value: list key or issue id. */
  peeked: string;
  /** Peeked project from the URL, when it differs from the route. */
  peekProjectId?: string | null | undefined;
  /** Rows the opener holds for key resolution without a fetch. */
  rows?: readonly Pick<IssueListItem, "id" | "project_id" | "sequence_id">[] | undefined;
  onClose: () => void;
}

export function ResolvingIssuePeek({
  workspaceSlug,
  projectId,
  projectIdentifier,
  peeked,
  peekProjectId,
  rows = [],
  onClose,
}: ResolvingIssuePeekProps): React.ReactElement {
  const client = getAppClient();
  const target = resolvePeekTarget(peeked, { rows, projectIdentifier, routeProjectId: projectId, peekProjectId });
  const lite = useQuery({ ...liteIssueQueryOptions(client, workspaceSlug, peeked.trim()), enabled: target === null });
  if (target !== null) {
    return (
      <IssuePeek
        workspaceSlug={workspaceSlug}
        projectId={target.projectId}
        projectIdentifier={projectIdentifier}
        issueId={target.issueId}
        issueKey={target.issueKey}
        onClose={onClose}
      />
    );
  }
  if (lite.isPending) return <PeekLoadingPanel mode="side" onClose={onClose} />;
  const row = lite.data?.lite ?? null;
  if (lite.isError || row === null) return <PeekErrorPanel mode="side" onClose={onClose} />;
  return (
    <IssuePeek
      workspaceSlug={workspaceSlug}
      projectId={row.project_id}
      projectIdentifier={projectIdentifier}
      issueId={row.id}
      issueKey={peeked.trim()}
      onClose={onClose}
    />
  );
}
