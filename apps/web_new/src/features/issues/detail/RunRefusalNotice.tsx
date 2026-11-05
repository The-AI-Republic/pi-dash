// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline answer to an assigned-runner refusal of Run AI (built-in engine
// design §7, §9). The server's sentence names the machine; the notice
// adds the way out. Someone else's runner: "Run on my engine instead"
// re-pins the issue to the caller's own runner (their engine first) and
// retries the run. An offline runner: pick another runner or the pod,
// which re-points the issue; the person starts the run again.
import { Button } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import { TriangleAlert } from "lucide-react";
import * as React from "react";

import { useMe } from "../../../core/session/hooks.js";
import {
  PodPicker,
  executionTargetPatch,
  usePickerRunners,
  type ExecutionSelection,
} from "../../../shared/pickers/index.js";
import { useUpdateExecutionTarget } from "../api/executionTarget.js";
import {
  ASSIGNED_RUNNER_NOT_OWNER,
  NO_OWN_RUNNER_TEXT,
  OWN_RUNNER_UNAVAILABLE_TEXT,
  ownRunnersUnavailable,
  PICK_ANOTHER_TARGET_LABEL,
  RUN_ON_MY_ENGINE_LABEL,
  selectOwnRunner,
  type RunRefusal,
} from "./runRefusal.js";

export interface RunRefusalNoticeProps {
  workspaceSlug: string;
  workspaceId: string;
  projectId: string;
  issue: Pick<Issue, "id">;
  refusal: RunRefusal;
  /** Start the run again once the issue points elsewhere. */
  onRetry: () => void;
  /** The refusal no longer applies (the issue was re-pointed). */
  onResolved: () => void;
}

export function RunRefusalNotice({
  workspaceSlug,
  workspaceId,
  projectId,
  issue,
  refusal,
  onRetry,
  onResolved,
}: RunRefusalNoticeProps): React.ReactElement {
  const notOwner = refusal.code === ASSIGNED_RUNNER_NOT_OWNER;
  const me = useMe();
  const runners = usePickerRunners(workspaceSlug, workspaceId, projectId, issue.id, notOwner);
  const target = useUpdateExecutionTarget(workspaceSlug, projectId, issue.id);
  const [pickerOpen, setPickerOpen] = React.useState(false);
  const own = notOwner ? selectOwnRunner(runners.data, me.data?.id) : null;
  const ownMissing = notOwner && runners.data !== undefined && me.data !== undefined && own === null;
  const ownUnavailable = ownMissing && ownRunnersUnavailable(runners.data, me.data?.id);

  async function runOnMine(): Promise<void> {
    if (own === null) return;
    try {
      await target.updateExecutionTargetAsync(
        executionTargetPatch({ type: "runner", runnerId: own.id, podId: own.pod })
      );
    } catch {
      // The picker write reports its own failure.
      return;
    }
    onResolved();
    onRetry();
  }

  async function pickOther(picked: ExecutionSelection): Promise<void> {
    try {
      await target.updateExecutionTargetAsync(executionTargetPatch(picked));
    } catch {
      return;
    }
    setPickerOpen(false);
    onResolved();
  }

  return (
    <div
      role="alert"
      data-testid="run-refusal"
      className="flex basis-full flex-col gap-(--space-2) rounded-(--radius-card) border border-(--warning) bg-(--subtle) p-(--space-3)"
    >
      <p className="text-body flex items-center gap-(--space-2) text-(--text)">
        <TriangleAlert size={16} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" aria-hidden="true" />
        {refusal.message}
      </p>
      {notOwner ? (
        ownMissing ? (
          <p className="text-caption text-(--text-muted)">
            {ownUnavailable ? OWN_RUNNER_UNAVAILABLE_TEXT : NO_OWN_RUNNER_TEXT}
          </p>
        ) : (
          <div>
            <Button
              variant="secondary"
              size="small"
              disabled={own === null || target.isSaving}
              loading={target.isSaving}
              onClick={() => void runOnMine()}
            >
              {RUN_ON_MY_ENGINE_LABEL}
            </Button>
          </div>
        )
      ) : (
        <PodPicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={null}
          onChange={(picked) => void pickOther(picked)}
          disabled={target.isSaving}
          saveError={target.validationError}
          assignableFor={issue.id}
          placeholder={PICK_ANOTHER_TARGET_LABEL}
          open={pickerOpen}
          onOpenChange={setPickerOpen}
        />
      )}
    </div>
  );
}
