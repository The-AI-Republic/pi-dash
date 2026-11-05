// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod members (built-in engine design §7, §11). Shown under the pod filter
// bar while a pod is selected: every live runner in the pod, whether it
// takes the pod's work or only issues assigned to it, and — for the
// viewer's own runners — add, remove and move. "Add runner to pod" lists
// the viewer's runners in the project, engines included. Writes go through
// the owner-only runner PATCH; refusals toast their reason.

import { Badge, Button, Menu, type MenuEntry } from "@pidash/kit";
import type { Pod, Runner, RunnerPatch } from "@pidash/api-client";
import { Plus } from "lucide-react";
import * as React from "react";

import { BUILT_IN_BADGE, isEngineRunner, runnerDisplayName } from "../../../shared/lib/runnerIdentity.js";
import { notifyError } from "../../../shared/shell/index.js";
import {
  POD_MEMBERSHIP_LABELS,
  REMOVE_FROM_POD_PATCH,
  RUNNER_UPDATE_FAILED_TITLE,
  addToPodPatch,
  addablePodRunners,
  canManageRunner,
  moveTargets,
  podRunners,
  runnerPodDispatch,
  runnerUpdateErrorMessage,
} from "../podMembers.js";

export const POD_MEMBERS_TITLE = "Runners in this pod";
export const POD_MEMBERS_EMPTY = "No runners in this pod yet.";
export const POD_ADD_RUNNER_LABEL = "Add runner to pod";
export const POD_ADD_RUNNER_NONE = "None of your runners in this project can be added.";
export const POD_REMOVE_LABEL = "Remove from pod";
export const POD_ADD_LABEL = "Add to pod";
export const POD_MOVE_LABEL = "Move to pod";

export interface PodMembersPanelProps {
  pod: Pod;
  /** Every runner of the scope, unfiltered; the panel narrows them. */
  runners: readonly Runner[];
  /** Pods of the scope, for the move targets. */
  pods: readonly Pod[];
  /** The signed-in user's id; null hides every action. */
  userId: string | null;
  onUpdate: (runnerId: string, patch: RunnerPatch) => Promise<unknown>;
}

function MemberRow({
  runner,
  pods,
  manageable,
  busy,
  onChange,
}: {
  runner: Runner;
  pods: readonly Pod[];
  manageable: boolean;
  busy: boolean;
  onChange: (patch: RunnerPatch) => void;
}): React.ReactElement {
  const dispatch = runnerPodDispatch(runner);
  const targets = moveTargets(pods, runner);
  return (
    <li
      data-testid="pod-member"
      data-runner-id={runner.id}
      className="flex flex-wrap items-center justify-between gap-(--space-2) border-t border-(--border) py-(--space-2) first:border-t-0"
    >
      <span className="flex flex-wrap items-center gap-(--space-2)">
        <span className="text-body text-(--text)">{runnerDisplayName(runner)}</span>
        {isEngineRunner(runner) ? <Badge tone="accent">{BUILT_IN_BADGE}</Badge> : null}
        <span data-testid="pod-member-dispatch" className="text-meta text-(--text-muted)">
          {POD_MEMBERSHIP_LABELS[dispatch]}
        </span>
      </span>
      {manageable ? (
        <span className="flex gap-(--space-2)">
          <Button
            size="small"
            variant="secondary"
            disabled={busy}
            onClick={() =>
              onChange(dispatch === "pod_member" ? REMOVE_FROM_POD_PATCH : addToPodPatch(runner, runner.pod))
            }
          >
            {dispatch === "pod_member" ? POD_REMOVE_LABEL : POD_ADD_LABEL}
          </Button>
          {targets.length === 0 ? null : (
            <Menu
              trigger={
                <Button
                  size="small"
                  variant="ghost"
                  disabled={busy}
                  aria-label={`${POD_MOVE_LABEL}: ${runnerDisplayName(runner)}`}
                >
                  {POD_MOVE_LABEL}
                </Button>
              }
              items={targets.map((target) => ({
                label: target.name,
                itemKey: target.id,
                onSelect: () => onChange({ pod: target.id }),
              }))}
            />
          )}
        </span>
      ) : null}
    </li>
  );
}

export function PodMembersPanel({ pod, runners, pods, userId, onUpdate }: PodMembersPanelProps): React.ReactElement {
  const [busyId, setBusyId] = React.useState<string | null>(null);
  const members = podRunners(runners, pod.id);
  const addable = addablePodRunners(runners, pod, userId);

  async function change(runner: Runner, patch: RunnerPatch): Promise<void> {
    setBusyId(runner.id);
    try {
      await onUpdate(runner.id, patch);
    } catch (error) {
      notifyError(RUNNER_UPDATE_FAILED_TITLE, runnerUpdateErrorMessage(error, pod.project_identifier));
    } finally {
      setBusyId(null);
    }
  }

  const addItems: MenuEntry[] =
    addable.length === 0
      ? [{ label: POD_ADD_RUNNER_NONE, disabled: true, onSelect: () => undefined }]
      : addable.map((runner) => ({
          label: runnerDisplayName(runner),
          itemKey: runner.id,
          description: runner.pod === pod.id ? POD_MEMBERSHIP_LABELS.pinned_only : runner.pod_detail.name,
          onSelect: () => void change(runner, addToPodPatch(runner, pod.id)),
        }));

  return (
    <section
      aria-label={POD_MEMBERS_TITLE}
      data-testid="pod-members"
      className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-3)"
    >
      <div className="flex items-center justify-between gap-(--space-2)">
        <h3 className="text-emphasis font-medium text-(--text)">{POD_MEMBERS_TITLE}</h3>
        {userId === null ? null : (
          <Menu
            trigger={
              <Button size="small" variant="secondary" disabled={busyId !== null}>
                <Plus size={14} />
                {POD_ADD_RUNNER_LABEL}
              </Button>
            }
            items={addItems}
          />
        )}
      </div>
      {members.length === 0 ? (
        <p className="text-meta text-(--text-muted)">{POD_MEMBERS_EMPTY}</p>
      ) : (
        <ul className="flex flex-col">
          {members.map((runner) => (
            <MemberRow
              key={runner.id}
              runner={runner}
              pods={pods}
              manageable={canManageRunner(runner, userId)}
              busy={busyId !== null}
              onChange={(patch) => void change(runner, patch)}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
