// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycle quick look (parity/specs/archives.md §4.1 and §5.6,
// ARCH-014, ARCH-025). The shared peek frame over the archived list,
// opened through the ?peekCycle= address key: the name in the frame's
// title heading, and the archived record's details — the state marker,
// dates, and progress summary — read from the archived detail
// endpoint, so opening the peek never depends on the list row still
// being loaded. Every editing affordance the live panel offers stays
// out: no status picker, no add-link control, no date or description
// edit, and no rename commit. A skeleton covers the load, and a failed
// read offers a retry instead of a blank panel.

import { Badge, Button, IconButton, Skeleton } from "@pidash/kit";
import type { ArchivedCycleDetail } from "@pidash/api-client";
import { ExternalLink, Link2 } from "lucide-react";
import * as React from "react";

import { PeekPanel } from "../../../shared/shell/index.js";
// Deep imports: this panel is reached from an eager route definition, so
// the cycles barrel would pin its whole export surface into the initial
// chunk (size gate).
import { CycleDateSpan } from "../../cycles/components/CycleDateSpan.js";
import { CycleProgressRing } from "../../cycles/components/CycleProgressRing.js";
import { lifecycleLabel, lifecycleTone } from "../../cycles/lib/cycleGroups.js";
import { cycleProgressCounts, cycleProgressPercent } from "../../cycles/lib/cycleProgress.js";
import { copyCycleLink, openCycleInNewTab } from "../../cycles/lib/cycleShare.js";
import { useArchivedCycleDetail } from "../api/archivedCycles.js";

export interface ArchivedCyclePeekPanelProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  /** Name from the list row; the detail read replaces it once loaded. */
  fallbackName: string;
  /** IANA name of the project's timezone. */
  projectTimeZone: string;
  onClose: () => void;
}

function ArchivedCyclePeekSkeleton(): React.ReactElement {
  return (
    <div data-testid="archived-cycle-peek-skeleton" className="flex flex-col gap-(--space-3)">
      <Skeleton height="14px" width="45%" />
      <Skeleton height="12px" width="75%" />
      <Skeleton height="12px" width="55%" />
      <Skeleton height="12px" width="30%" />
    </div>
  );
}

function ArchivedCyclePeekBody({
  cycle,
  projectTimeZone,
}: {
  cycle: ArchivedCycleDetail;
  projectTimeZone: string;
}): React.ReactElement {
  const counts = cycleProgressCounts(cycle);
  return (
    <div data-testid="archived-cycle-peek-body" className="flex flex-col gap-(--space-4)">
      <Badge tone={lifecycleTone(cycle.status)} dot>
        {lifecycleLabel(cycle.status)}
      </Badge>
      <CycleDateSpan startDate={cycle.start_date} endDate={cycle.end_date} projectTimeZone={projectTimeZone} />
      <div data-testid="archived-cycle-peek-summary" className="flex items-center gap-(--space-3)">
        <CycleProgressRing percent={cycleProgressPercent(cycle)} />
        <span className="text-body text-(--text)">
          {counts.completed} of {counts.total} {counts.total === 1 ? "work item" : "work items"}
        </span>
      </div>
    </div>
  );
}

export function ArchivedCyclePeekPanel({
  workspaceSlug,
  projectId,
  cycleId,
  fallbackName,
  projectTimeZone,
  onClose,
}: ArchivedCyclePeekPanelProps): React.ReactElement {
  const detail = useArchivedCycleDetail(workspaceSlug, projectId, cycleId);
  const cycle = detail.data ?? null;
  return (
    <PeekPanel
      label="Archived cycle preview"
      testId="archived-cycle-preview"
      eyebrow="Archived cycle"
      title={cycle?.name ?? fallbackName}
      titleLabel="Archived cycle name"
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
        <ArchivedCyclePeekSkeleton />
      ) : cycle === null ? (
        <div data-testid="archived-cycle-peek-error" className="flex flex-col items-start gap-(--space-3)">
          <p className="text-body text-(--text)">Couldn&apos;t load this archived cycle.</p>
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
        <ArchivedCyclePeekBody cycle={cycle} projectTimeZone={projectTimeZone} />
      )}
    </PeekPanel>
  );
}
