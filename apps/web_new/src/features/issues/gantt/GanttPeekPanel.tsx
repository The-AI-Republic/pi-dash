// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline peek panel (parity/specs/issues.md §14, ISS-049/165). Reuses
// the shell's shared panel: the eyebrow carries the short key, the title
// stays editable through the issue PATCH, and the body shows the parent
// context while the full peek composition lands with its own slice. The
// detail query hydrates behind a skeleton so the panel never flashes.

import { useSuspenseQuery } from "@tanstack/react-query";
import * as React from "react";

import { Skeleton } from "@pidash/kit";

import { getAppClient } from "../../../core/api/client.js";
import { PeekPanel } from "../../../shared/shell/index.js";
import { useUpdateIssue } from "../api/mutations.js";
import { issueDetailQueryOptions } from "../api/queries.js";
import type { GanttRow } from "./rows.js";

export interface GanttPeekPanelProps {
  workspaceSlug: string;
  projectId: string;
  row: GanttRow;
  onClose: () => void;
}

function PeekBody({
  workspaceSlug,
  projectId,
  issueId,
}: {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}) {
  const { data } = useSuspenseQuery(issueDetailQueryOptions(getAppClient(), workspaceSlug, projectId, issueId));
  return (
    <dl className="text-body grid grid-cols-[max-content_1fr] gap-(--space-2) text-(--text)">
      <dt className="text-(--text-muted)">State</dt>
      <dd>{data.state_id === null ? "No state" : "Assigned"}</dd>
      <dt className="text-(--text-muted)">Start</dt>
      <dd>{data.start_date ?? "Not set"}</dd>
      <dt className="text-(--text-muted)">Target</dt>
      <dd>{data.target_date ?? "Not set"}</dd>
    </dl>
  );
}

function PeekFallback(): React.ReactElement {
  return (
    <div data-testid="gantt-peek-skeleton" className="flex flex-col gap-(--space-2)">
      <Skeleton height="12px" width="60%" />
      <Skeleton height="12px" width="40%" />
      <Skeleton height="12px" width="80%" />
    </div>
  );
}

export function GanttPeekPanel({ workspaceSlug, projectId, row, onClose }: GanttPeekPanelProps): React.ReactElement {
  const rename = useUpdateIssue({
    workspaceSlug,
    projectId,
    issueId: row.id,
    failureTitle: "Couldn't rename the work item",
  });
  return (
    <PeekPanel
      eyebrow={row.identifier}
      title={row.name}
      titleLabel="Work item title"
      onClose={onClose}
      onCommitTitle={(next: string) => rename.mutate({ name: next })}
    >
      <React.Suspense fallback={<PeekFallback />}>
        <PeekBody workspaceSlug={workspaceSlug} projectId={projectId} issueId={row.id} />
      </React.Suspense>
    </PeekPanel>
  );
}
