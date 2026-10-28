// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud issues providers and the edition bootstrap (parity/specs/issues.md
// §20/§23, NEWFRONT-984). Flags decide whether a cloud surface may render;
// these providers decide what it knows. The app calls
// `bootstrapIssuesEdition` once at startup; the community build (no issues
// flag on) registers nothing, so every seam stays empty exactly as the
// OSS oracle pins. A flag turned on without its provider still renders
// nothing, so each area below lands independently: team projects and the
// transfer-hop note now, workflow rules, work-item types and the type
// detail/modal extensions with their own issues.
import * as React from "react";

import { listProjects } from "@pidash/api-client";
import { getAppClient } from "../../../core/api/client.js";
import type { Edition } from "../../../core/edition/index.js";
import { BULK_OPERATIONS_FLAG } from "../bulk/edition.js";
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
import type { DetailBadgeInput, TeamProjectOption } from "./providers.js";
import { registerEditionIssuesProviders } from "./providers.js";

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
 * match as usual; nothing here filters the server answer.
 */
export async function listTeamProjectOptions(input: {
  workspaceSlug: string;
  projectId: string;
}): Promise<TeamProjectOption[]> {
  const projects = await listProjects(getAppClient(), input.workspaceSlug);
  return projects.map((project) => ({ id: project.id, name: project.name, identifier: project.identifier }));
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
  });
}

/** For tests: allow the bootstrap to run again. */
export function resetIssuesEditionBootstrap(): void {
  bootstrapped = false;
}
