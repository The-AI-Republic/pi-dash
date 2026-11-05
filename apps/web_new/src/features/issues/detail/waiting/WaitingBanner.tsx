// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Waiting banner for the issue detail (hybrid assignment, design
// §10.3): while the active run is queued for its runner, the banner
// names the runner and the reason, and offers the three ways out —
// release this run to the pod once, change the issue's runner, or
// cancel the run. A run strictly pinned to an offline computer (built-in
// engine design §9) reads "Waiting for <machine>" with the machine's
// offline sentence; its pin is never released to the pod, so it offers
// only changing the issue's runner and cancelling. Renders nothing while
// nothing waits.
import { Button, Dialog } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import { Hourglass } from "lucide-react";
import * as React from "react";

import { useWorkspace } from "../../../../core/session/hooks.js";
import {
  PodPicker,
  executionTargetPatch,
  usePickerPods,
  usePickerRunners,
  type ExecutionSelection,
} from "../../../../shared/pickers/index.js";
import { useUpdateExecutionTarget } from "../../api/executionTarget.js";
import {
  machineWaitBannerText,
  machineWaitLabel,
  machineWaitName,
  machineWaitPause,
  selectMachineWait,
  selectWaitingRun,
  waitingBannerText,
} from "./waiting.js";
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
  const machineWait = waiting === null ? selectMachineWait(issue) : null;
  const { workspace } = useWorkspace(workspaceSlug);
  const runners = usePickerRunners(workspaceSlug, workspace?.id ?? "", projectId, issue.id, machineWait !== null);
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

  if (waiting === null && machineWait === null) return null;

  const podName = pods.data?.find((pod) => pod.id === issue.assigned_pod_id)?.name ?? null;
  const releaseLabel = podName === null ? "Run on any runner in the pod once" : `Run on any runner in ${podName} once`;
  const busy = release.isReleasing || cancel.isCancelling || change.isSaving;

  async function handleChange(picked: ExecutionSelection): Promise<void> {
    try {
      await change.updateExecutionTargetAsync(executionTargetPatch(picked));
      setChangeOpen(false);
    } catch {
      // The rejection stays in the dialog next to the picker.
    }
  }

  const machineName = machineWait === null ? null : machineWaitName(machineWait, runners.data);
  const machinePause = machineWait === null ? null : machineWaitPause(machineWait, runners.data);
  const sentence =
    waiting !== null
      ? waitingBannerText(waiting.runnerName, waiting.wait)
      : machineWaitBannerText(machineName ?? "", machinePause);
  const activeRunId = waiting?.run.id ?? machineWait?.run.id ?? "";

  return (
    <section
      aria-label={machineName === null ? "Waiting for runner" : machineWaitLabel(machineName)}
      className="flex flex-col gap-(--space-3) rounded-(--radius-card) border border-(--warning) bg-(--subtle) p-(--space-4)"
    >
      <p className="text-body flex items-center gap-(--space-2) text-(--text)">
        <Hourglass size={16} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" aria-hidden="true" />
        {sentence}
      </p>
      <div className="flex flex-wrap items-center gap-(--space-2)">
        {waiting === null ? null : (
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
          onClick={() => cancel.cancelWaitingRun(activeRunId)}
        >
          Cancel run
        </Button>
      </div>
    </section>
  );
}
