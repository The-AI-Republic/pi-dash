// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud issues providers and the edition bootstrap (parity/specs/issues.md
// §20/§23, NEWFRONT-984). Flags decide whether a cloud surface may render;
// these providers decide what it knows. The app calls
// `bootstrapIssuesEdition` once at startup; the community build (no issues
// flag on) registers nothing, so every seam stays empty exactly as the
// OSS oracle pins. A flag turned on without its provider still renders
// nothing, so each area below lands independently: team projects, the
// transfer-hop note, work-item types, the modal type picker, the detail
// type badge and the provisional dev-only workflow rules now; templates
// and extra properties with their own issues. Registration stays
// synchronous at startup while every heavy body loads lazily (lazy
// components, dynamic contract imports), so this module adds only its
// shells to the initial chunk (NEWFRONT-999).
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import type { Edition } from "../../../core/edition/index.js";
import { BULK_OPERATIONS_FLAG } from "../bulk/edition.js";
import { registerIssueTypeExtension, type IssueTypeExtensionProps } from "../create/issueTypeSeam.js";
import { WORKLOG_FLAG } from "../detail/widgets/worklog/worklog.js";
import {
  DETAIL_BADGES_FLAG,
  EPICS_FLAG,
  GANTT_DEPENDENCIES_FLAG,
  ISSUE_DEDUPE_FLAG,
  ISSUE_TYPE_FILTER_FLAG,
  ISSUE_TYPES_FLAG,
  TEAM_PROJECT_FILTER_FLAG,
  WORKFLOW_FLAG,
} from "./flags.js";
import type { DetailBadgeInput, TeamProjectOption, WorkItemTypeOption } from "./providers.js";
import { registerEditionIssuesProviders } from "./providers.js";
import { parityWorkflowRulesProvider } from "./workflowParityRules.js";

/** Every issues flag the cloud edition may set (§23). */
export const ISSUES_EDITION_FLAGS = [
  BULK_OPERATIONS_FLAG,
  ISSUE_DEDUPE_FLAG,
  WORKLOG_FLAG,
  EPICS_FLAG,
  WORKFLOW_FLAG,
  TEAM_PROJECT_FILTER_FLAG,
  ISSUE_TYPE_FILTER_FLAG,
  ISSUE_TYPES_FLAG,
  DETAIL_BADGES_FLAG,
  GANTT_DEPENDENCIES_FLAG,
] as const;

/** True when the edition turns on any issues cloud surface. */
export function hasIssuesEditionFlags(flags: Record<string, boolean>): boolean {
  return ISSUES_EDITION_FLAGS.some((flag) => flags[flag] === true);
}

/**
 * Team projects the sub-issue filter offers (ISS-234): the workspace's
 * projects as the OSS list answers them. Rows from the current project
 * match as usual; nothing here filters the server answer. The contract
 * loads on first call so its schemas stay out of the initial chunk
 * (NEWFRONT-999).
 */
export async function listTeamProjectOptions(input: {
  workspaceSlug: string;
  projectId: string;
}): Promise<TeamProjectOption[]> {
  const { listProjects } = await import("@pidash/api-client");
  const projects = await listProjects(getAppClient(), input.workspaceSlug);
  return projects.map((project) => ({ id: project.id, name: project.name, identifier: project.identifier }));
}

/**
 * Project work-item types for the type filter, modal picker and detail
 * badge. The contract loads on first call so its schemas stay out of the
 * initial chunk (NEWFRONT-999).
 */
export async function listWorkItemTypeOptions(input: {
  workspaceSlug: string;
  projectId: string;
}): Promise<WorkItemTypeOption[]> {
  const { listIssueTypes } = await import("@pidash/api-client");
  const rows = await listIssueTypes(getAppClient(), input.workspaceSlug, input.projectId);
  return rows.map((row) => ({ id: row.id, name: row.name }));
}

const TransferHopNoteBody = React.lazy(() =>
  import("./TransferHopNoteContent.js").then((module) => ({ default: module.TransferHopNoteContent }))
);

/**
 * The cloud transfer-hop note (ISS-236). The body travels in its own
 * chunk: the sidebar suspends to nothing while it loads, and the
 * community bundle never downloads it.
 */
export function CloudTransferNote(input: DetailBadgeInput): React.ReactElement {
  return (
    <React.Suspense fallback={null}>
      <TransferHopNoteBody
        workspaceSlug={input.workspaceSlug}
        projectId={input.projectId}
        issueId={input.issueId}
        cycleId={input.cycleId}
      />
    </React.Suspense>
  );
}

const DetailTypeBadgeBody = React.lazy(() =>
  import("./DetailTypeBadgeContent.js").then((module) => ({ default: module.DetailTypeBadgeContent }))
);

/**
 * The cloud type badge (ISS-235). The body travels in its own chunk:
 * the identifier suspends to nothing while it loads, and the community
 * bundle never downloads it.
 */
export function CloudTypeBadge(input: {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  editable: boolean;
}): React.ReactElement {
  return (
    <React.Suspense fallback={null}>
      <DetailTypeBadgeBody
        workspaceSlug={input.workspaceSlug}
        projectId={input.projectId}
        issueId={input.issueId}
        editable={input.editable}
      />
    </React.Suspense>
  );
}

const TypePickerBody = React.lazy(() =>
  import("./IssueTypePicker.js").then((module) => ({ default: module.IssueTypePicker }))
);

/**
 * The cloud modal type picker (ISS-129 type half). The picker travels in
 * its own chunk: the modal suspends to nothing while it loads, and the
 * initial bundle never downloads it (NEWFRONT-999).
 */
export function CloudTypePicker(props: IssueTypeExtensionProps): React.ReactElement {
  return (
    <React.Suspense fallback={null}>
      <TypePickerBody
        workspaceSlug={props.workspaceSlug}
        projectId={props.projectId}
        disabled={props.disabled}
        typeId={props.typeId}
        onTypeChange={props.onTypeChange}
        onTemplateApply={props.onTemplateApply}
        propertyValues={props.propertyValues}
        onPropertyValuesChange={props.onPropertyValuesChange}
      />
    </React.Suspense>
  );
}

let bootstrapped = false;

/**
 * Register the cloud issues providers once. Called at startup with the
 * active edition; a second call (or a community edition) changes nothing.
 */
export function bootstrapIssuesEdition(edition: Edition): void {
  if (bootstrapped) return;
  bootstrapped = true;
  if (!hasIssuesEditionFlags(edition.flags)) return;
  registerEditionIssuesProviders({
    teamProjects: listTeamProjectOptions,
    detailBadges: { transferNote: CloudTransferNote },
    workItemTypes: listWorkItemTypeOptions,
    issueTypeDetail: { badge: CloudTypeBadge },
    // Provisional (ISS-233): dev-only injected rules proving the gate
    // mechanism; NEWFRONT-994 replaces this provider with the real source.
    workflowRules: parityWorkflowRulesProvider(),
  });
  // The modal type picker only (ISS-129 type half): templates and extra
  // properties register with the Part 5 model. The lazy shell keeps the
  // picker chunk out of the initial bundle (NEWFRONT-999).
  registerIssueTypeExtension({ typePicker: CloudTypePicker });
}

/** For tests: allow the bootstrap to run again. */
export function resetIssuesEditionBootstrap(): void {
  bootstrapped = false;
}
