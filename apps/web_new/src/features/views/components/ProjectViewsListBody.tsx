// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project views list body (parity/specs/views.md §3, VIEW-008-011). Renders
// exactly one state for the resolved body: skeleton, error, zero-views,
// no-match, or one row per visible view.

import type { SavedView, WorkspaceMember } from "@pidash/api-client";
import * as React from "react";

import { findViewOwner, type ProjectViewsBodyState } from "../lib/projectViews.js";
import { ProjectViewRow } from "./ProjectViewRow.js";
import { ProjectViewsError } from "./ProjectViewsError.js";
import { ProjectViewsNoMatch } from "./ProjectViewsNoMatch.js";
import { ProjectViewsSkeleton } from "./ProjectViewsSkeleton.js";
import { ProjectViewsZeroState } from "./ProjectViewsZeroState.js";

export interface ProjectViewsListBodyProps {
  state: ProjectViewsBodyState;
  /** Visible views in display order. */
  views: readonly SavedView[];
  /** Roster for owner chips; rows degrade to no avatar without it. */
  members: readonly WorkspaceMember[] | undefined;
  workspaceSlug: string;
  projectId: string;
  /** Opens the create dialog. Slot owner: NEWFRONT-721. */
  onCreateView?: (() => void) | undefined;
  onRetry: () => void;
}

export function ProjectViewsListBody({
  state,
  views,
  members,
  workspaceSlug,
  projectId,
  onCreateView,
  onRetry,
}: ProjectViewsListBodyProps): React.ReactElement {
  if (state === "loading") return <ProjectViewsSkeleton />;
  if (state === "error") return <ProjectViewsError onRetry={onRetry} />;
  if (state === "zero") return <ProjectViewsZeroState onCreateView={onCreateView} />;
  if (state === "no-match") return <ProjectViewsNoMatch />;
  return (
    <ul data-testid="project-views-list" className="flex min-w-0 flex-1 flex-col">
      {views.map((view) => (
        <ProjectViewRow
          key={view.id}
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          view={view}
          owner={findViewOwner(members, view.owned_by)}
        />
      ))}
    </ul>
  );
}
