// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail sidebar, property rows A (parity/specs/issues.md §13,
// ISS-148–152): state, assignees, runs on, priority, created by. Each
// editable row writes one property with an optimistic PATCH; guests,
// archived items and synced items render the same rows with every
// control disabled (the archived pattern from §17, ISS-220). The
// pickers read lazily on open; the workspace directory loads eagerly
// when a creator or an assignee has to be named.

import { Skeleton, type ButtonVariant } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import {
  MemberPicker,
  PodPicker,
  PriorityPicker,
  StatePicker,
  executionTargetLabel,
  executionTargetPatch,
  type ExecutionSelection,
  type PriorityLevel,
} from "../../../../shared/pickers/index.js";
import { workspaceMembersQueryOptions } from "../../../projects/index.js";
import { useUpdateExecutionTarget } from "../../api/executionTarget.js";
import { useUpdateIssue } from "../../api/mutations.js";
import { CreatedByRow } from "./CreatedByRow.js";
import { selectCreator } from "./creator.js";
import type { SidebarProject } from "./IssuePropertiesSidebar.js";
import { SIDEBAR_ROW_CLASS, SIDEBAR_ROW_LABEL_CLASS, SidebarRow } from "./SidebarRow.js";

/** Notice title when a sidebar property write is refused. */
export const PROPERTY_SAVE_FAILED_TITLE = "Couldn't update the work item";

/** Row controls sit inline and read as text until hovered. */
const ROW_VARIANT: ButtonVariant = "ghost";

/**
 * The issue's current execution target. The per-issue override wins;
 * a null override inherits the project's default executor, exactly as
 * the server resolves it (`issue.agent_executor or
 * project.default_agent_executor`), so the row states where a run will
 * actually go. An agent target renders as that agent; a local target
 * renders the pinned pod, and no pin means the project's default pod.
 */
export function selectExecutionSelection(
  issue: Pick<Issue, "agent_executor" | "assigned_pod_id">,
  project: Pick<SidebarProject, "default_agent_executor">
): ExecutionSelection | null {
  const executor = issue.agent_executor ?? project.default_agent_executor;
  if (executor === "cloud_agent" || executor === "managed_runner") {
    return { type: "agent", executor };
  }
  const podId = issue.assigned_pod_id ?? null;
  return podId === null ? null : { type: "pod", podId };
}

export interface SidebarPropertiesProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  /** Project row carrying the agent options the runs-on row is built from. */
  project: SidebarProject;
  /** Project member/admin on a live project. Archived and synced items always read. */
  editable: boolean;
}

export function SidebarProperties({
  workspaceSlug,
  projectId,
  issue,
  project,
  editable,
}: SidebarPropertiesProps): React.ReactElement {
  const disabled = !editable || issue.archived_at !== null || issue.is_synced;
  const update = useUpdateIssue({
    workspaceSlug,
    projectId,
    issueId: issue.id,
    failureTitle: PROPERTY_SAVE_FAILED_TITLE,
  });
  const targetUpdate = useUpdateExecutionTarget(workspaceSlug, projectId, issue.id);
  const directory = useQuery({
    ...workspaceMembersQueryOptions(getAppClient(), workspaceSlug),
    enabled: issue.created_by !== null || issue.assignee_ids.length > 0,
  });
  const creator = selectCreator(directory.data, issue.created_by);
  // The directory read names the creator; until it lands, the row shows a
  // neutral placeholder rather than an "unknown" verdict.
  const creatorPending = issue.created_by !== null && directory.isPending;
  const selection = selectExecutionSelection(issue, project);

  return (
    <div className="flex flex-col gap-(--space-1)">
      <SidebarRow label="State">
        <StatePicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={issue.state_id}
          disabled={disabled}
          variant={ROW_VARIANT}
          placeholder="Set state"
          onChange={(stateId) => update.mutate({ state_id: stateId })}
        />
      </SidebarRow>
      <SidebarRow label="Assignees">
        <MemberPicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          multiple
          value={issue.assignee_ids}
          disabled={disabled}
          variant={ROW_VARIANT}
          placeholder="Add assignees"
          onChange={(ids) => update.mutate({ assignee_ids: ids })}
        />
      </SidebarRow>
      <SidebarRow label={executionTargetLabel(project.agent_executor_options)}>
        <PodPicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={selection}
          disabled={disabled}
          variant={ROW_VARIANT}
          placeholder="Select target"
          onChange={(picked) => targetUpdate.updateExecutionTarget(executionTargetPatch(picked))}
        />
      </SidebarRow>
      <SidebarRow label="Priority">
        <PriorityPicker
          value={issue.priority as PriorityLevel}
          disabled={disabled}
          variant={ROW_VARIANT}
          onChange={(level) => update.mutate({ priority: level })}
        />
      </SidebarRow>
      <CreatedByRow creator={creator} pending={creatorPending} />
    </div>
  );
}

/** Placeholder rows while the project row that shapes runs-on hydrates. */
export function SidebarPropertiesSkeleton(): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-2)" role="status" aria-label="Loading properties">
      {[0, 1, 2, 3, 4].map((index) => (
        <div key={index} className={SIDEBAR_ROW_CLASS}>
          <span className={SIDEBAR_ROW_LABEL_CLASS} />
          <Skeleton height="20px" />
        </div>
      ))}
    </div>
  );
}

export type { ExecutionSelection };
