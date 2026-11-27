// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Waiting banner for the issue detail (hybrid assignment, design
// §10.3, §14.4): while the active run is queued for its runner, the banner
// names the runner and the reason. For a locality pin it offers release
// to the pod once; for a dedicated pin the release is never offered
// (H-13, §12.2: 409 dedicated_pin_not_releasable, releasable always
// false). Change runner and Cancel run remain for every pin.
// Renders nothing while nothing waits.
import { Button, Dialog } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import { Hourglass } from "lucide-react";
import * as React from "react";

import {
  PodPicker,
  executionTargetPatch,
  formatRunnerSeenAgo,
  usePickerPods,
  type ExecutionSelection,
} from "../../../../shared/pickers/index.js";
import { useUpdateExecutionTarget } from "../../api/executionTarget.js";
import { selectWaitingRun, waitingBannerText } from "./waiting.js";
import { useCancelWaitingRun, useReleaseWaitingRun } from "./waitingRuns.js";

export interface WaitingBannerProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  /** Guests, archived and synced rows read the banner without actions. */
  disabled: boolean;
}

export function WaitingBanner({
  workspaceSlug,
  projectId,
  issue,
  disabled,
}: WaitingBannerProps): React.ReactElement | null {
  const waiting = selectWaitingRun(issue);
  const pods = usePickerPods(workspaceSlug, projectId, waiting !== null);
  const release = useReleaseWaitingRun(workspaceSlug);
  const cancel = useCancelWaitingRun(workspaceSlug);
  const change = useUpdateExecutionTarget(workspaceSlug, projectId, issue.id);
  const [changeOpen, setChangeOpen] = React.useState(false);
  const [pickerOpen, setPickerOpen] = React.useState(false);

  function handleChangeOpen(next: boolean): void {
    setChangeOpen(next);
    if (next) setPickerOpen(true);
  }

  if (waiting === null) return null;

  const podName = pods.data?.find((pod) => pod.id === issue.assigned_pod_id)?.name ?? null;
  const releaseLabel = podName === null ? "Run on any runner in the pod once" : `Run on any runner in ${podName} once`;
  const isDedicated = waiting.wait.pin_kind === "dedicated";
  const busy = (isDedicated ? false : release.isReleasing) || cancel.isCancelling || change.isSaving;

  async function handleChange(picked: ExecutionSelection): Promise<void> {
    try {
      await change.updateExecutionTargetAsync(executionTargetPatch(picked));
      setChangeOpen(false);
    } catch {
      // The rejection stays in the dialog next to the picker.
    }
  }

  // Machine-level capacity wait (NEWPIDASH-241 T7), distinct from the
  // cloud dispatch_wait above. Typed via the shared `CapacityWait`
  // contract — no inline cast. Stale reports (>90s, the server
  // freshness window) render as last-known, never as live.
  const capacity = waiting.run.capacity_wait ?? null;
  let capacityText: string | null = null;
  if (capacity !== null && capacity.phase === "waiting_for_capacity") {
    const parts = ["Waiting for machine capacity"];
    if (capacity.queue_position != null) parts.push(`position ${capacity.queue_position}`);
    if (capacity.active != null && capacity.limit != null) parts.push(`(${capacity.active}/${capacity.limit})`);
    if (capacity.reported_at != null) {
      const ago = formatRunnerSeenAgo(capacity.reported_at);
      const elapsedMs = Date.now() - Date.parse(capacity.reported_at);
      if (ago !== null) {
        parts.push(elapsedMs > 90_000 ? `last known ${ago}` : `last updated ${ago}`);
      }
    }
    capacityText = parts.join(" — ");
  }
  return (
    <section
      aria-label="Waiting for runner"
      className="flex flex-col gap-(--space-3) rounded-(--radius-card) border border-(--warning) bg-(--subtle) p-(--space-4)"
    >
      <p className="text-body flex items-center gap-(--space-2) text-(--text)">
        <Hourglass size={16} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" aria-hidden="true" />
        {waitingBannerText(waiting.runnerName, waiting.wait)}
      </p>
      {capacityText && (
        <p data-testid="capacity-waiting" className="text-body text-(--text-muted)">
          {capacityText}
        </p>
      )}
      <div className="flex flex-wrap items-center gap-(--space-2)">
        {!isDedicated && (
          <Button
            variant="secondary"
            size="small"
            disabled={disabled || busy || !waiting.wait.releasable}
            loading={release.isReleasing}
            onClick={() => release.releaseWaitingRun(waiting.run.id)}
          >
            {releaseLabel}
          </Button>
        )}
        <Dialog
          trigger={
            <Button variant="secondary" size="small" disabled={disabled || busy}>
              Change runner
            </Button>
          }
          title="Change runner"
          description="Pick the pod or the runner this issue's runs go to."
          open={changeOpen}
          onOpenChange={(next) => handleChangeOpen(next)}
        >
          <PodPicker
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            value={null}
            onChange={(picked) => void handleChange(picked)}
            disabled={change.isSaving}
            saveError={change.validationError}
            assignableFor={issue.id}
            preload
            open={pickerOpen}
            onOpenChange={setPickerOpen}
          />
        </Dialog>
        <Button
          variant="ghost"
          size="small"
          disabled={disabled || busy}
          loading={cancel.isCancelling}
          onClick={() => cancel.cancelWaitingRun(waiting.run.id)}
        >
          Cancel run
        </Button>
      </div>
    </section>
  );
}
