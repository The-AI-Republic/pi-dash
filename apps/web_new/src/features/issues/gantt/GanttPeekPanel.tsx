// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline peek panel (parity/specs/issues.md §14, ISS-049/165). Reuses
// the shell's shared panel: the eyebrow carries the short key, the title
// stays editable through the issue PATCH, and the body names the issue's
// state plus its dates while the full peek composition lands with its own
// slice (ISS-165). The detail query hydrates behind a skeleton so the panel
// never flashes; the state name resolves through the project's state list
// the timeline already reads for bar tints.

import { useSuspenseQuery } from "@tanstack/react-query";
import * as React from "react";

import { Skeleton } from "@pidash/kit";

import { getAppClient } from "../../../core/api/client.js";
import { CopyWorkItemLinkButton } from "../../../shared/shell/CopyWorkItemLinkButton.js";
import { PeekPanel } from "../../../shared/shell/index.js";
import { useUpdateIssue } from "../api/mutations.js";
import { issueDetailQueryOptions } from "../api/queries.js";
import type { GanttRow } from "./rows.js";
import { useGanttStates } from "./useGanttData.js";

export interface GanttPeekPanelProps {
  workspaceSlug: string;
  projectId: string;
  row: GanttRow;
  onClose: () => void;
}

/** Name of the state a row belongs to; "No state" when unset or unknown. */
export function stateNameOf(states: readonly { id: string; name: string }[], stateId: string | null): string {
  if (stateId === null) return "No state";
  return states.find((state) => state.id === stateId)?.name ?? "No state";
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
  const states = useGanttStates(workspaceSlug, projectId);
  return (
    <dl className="text-body grid grid-cols-[max-content_1fr] gap-(--space-2) text-(--text)">
      <dt className="text-(--text-muted)">State</dt>
      <dd>{stateNameOf(states, data.state_id)}</dd>
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
      actions={<CopyWorkItemLinkButton workspaceSlug={workspaceSlug} issueKey={row.identifier} />}
    >
      <React.Suspense fallback={<PeekFallback />}>
        <PeekBody workspaceSlug={workspaceSlug} projectId={projectId} issueId={row.id} />
      </React.Suspense>
    </PeekPanel>
  );
}
