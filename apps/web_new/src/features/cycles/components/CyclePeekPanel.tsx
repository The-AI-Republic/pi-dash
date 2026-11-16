// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle quick look (parity/specs/cycles.md §2, CYC-004). The shared
// peek frame over the list, opened through the ?peekCycle= address key:
// the name in the frame's title field, and the cycle summary — the
// state marker, dates, progress ring and work-item counts — with a
// link to the full detail. The body reads the same detail query as
// the detail page, so opening the peek never depends on the list row
// still being loaded; a skeleton covers the load, and a failed read
// offers a retry instead of a blank panel. Renaming stays in the
// detail sidebar (slice G), so the frame's title field does not commit.

import { Link } from "@tanstack/react-router";
import { Badge, Button, IconButton } from "@pidash/kit";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import { ExternalLink, Link2 } from "lucide-react";
import * as React from "react";

import { PeekPanel } from "../../../shared/shell/index.js";
import { useCycleDetail } from "../api/cycleDetail.js";
import { lifecycleLabel, lifecycleTone } from "../lib/cycleGroups.js";
import { cycleDetailPath } from "../lib/cycleScreen.js";
import { cycleProgressCounts, cycleProgressPercent } from "../lib/cycleProgress.js";
import { copyCycleLink, openCycleInNewTab } from "../lib/cycleShare.js";
import { CycleDateSpan } from "./CycleDateSpan.js";
import { CycleProgressRing } from "./CycleProgressRing.js";
import { CycleSidebarSkeleton } from "./CycleSidebarSkeleton.js";

export interface CyclePeekPanelProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  /** Name from the list row; the detail read replaces it once loaded. */
  fallbackName: string;
  /** IANA name of the project's timezone. */
  projectTimeZone: string;
  onClose: () => void;
}

interface CyclePeekBodyProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  cycle: CycleRowData;
  projectTimeZone: string;
}

function CyclePeekBody({
  workspaceSlug,
  projectId,
  cycleId,
  cycle,
  projectTimeZone,
}: CyclePeekBodyProps): React.ReactElement {
  const counts = cycleProgressCounts(cycle);
  return (
    <div data-testid="cycle-peek-body" className="flex flex-col gap-(--space-4)">
      <Badge tone={lifecycleTone(cycle.status)} dot>
        {lifecycleLabel(cycle.status)}
      </Badge>
      <CycleDateSpan startDate={cycle.start_date} endDate={cycle.end_date} projectTimeZone={projectTimeZone} />
      <div className="flex items-center gap-(--space-3)">
        <CycleProgressRing percent={cycleProgressPercent(cycle)} />
        <span className="text-body text-(--text)">
          {counts.completed} of {counts.total} {counts.total === 1 ? "work item" : "work items"}
        </span>
      </div>
      <Link
        to={cycleDetailPath(workspaceSlug, projectId, cycleId)}
        className="text-body font-medium text-(--accent) hover:underline"
      >
        Open full detail
      </Link>
    </div>
  );
}

export function CyclePeekPanel({
  workspaceSlug,
  projectId,
  cycleId,
  fallbackName,
  projectTimeZone,
  onClose,
}: CyclePeekPanelProps): React.ReactElement {
  const detail = useCycleDetail(workspaceSlug, projectId, cycleId);
  const cycle = detail.data ?? null;
  return (
    <PeekPanel
      label="Cycle preview"
      testId="cycle-preview"
      eyebrow="Cycle"
      title={cycle?.name ?? fallbackName}
      titleLabel="Cycle name"
      titleAsHeading
      onClose={onClose}
      actions={
        <>
          <IconButton
            label="Copy cycle link"
            size="small"
            onClick={() => {
              void copyCycleLink(workspaceSlug, projectId, cycleId);
            }}
          >
            <Link2 size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
          <IconButton
            label="Open cycle in new tab"
            size="small"
            onClick={() => {
              void openCycleInNewTab(workspaceSlug, projectId, cycleId);
            }}
          >
            <ExternalLink size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </>
      }
    >
      {detail.isPending ? (
        <CycleSidebarSkeleton testId="cycle-peek-skeleton" />
      ) : cycle === null ? (
        <div data-testid="cycle-peek-error" className="flex flex-col items-start gap-(--space-3)">
          <p className="text-body text-(--text)">Couldn&apos;t load this cycle.</p>
          <Button
            variant="secondary"
            size="small"
            onClick={() => {
              void detail.refetch();
            }}
          >
            Try again
          </Button>
        </div>
      ) : (
        <CyclePeekBody
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          cycleId={cycleId}
          cycle={cycle}
          projectTimeZone={projectTimeZone}
        />
      )}
    </PeekPanel>
  );
}
