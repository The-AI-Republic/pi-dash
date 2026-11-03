// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues empty surface (parity/specs/issues.md §8, ISS-073).
// The list host (NEWFRONT-656) owns the reads and the rows; the issues
// slice owns the card the empty surface shows. Two facts decide the card:
// whether the caller can see a project at all, and whether the workspace
// read returned rows. Guests see the same copy with the entries dead,
// because creating a project or a work item is a member power.

import type { Project } from "@pidash/api-client";
import * as React from "react";

import { usePermissions } from "../../../core/session/hooks.js";
import { GlobalIssuesEmpty } from "../emptyStates/cards.js";
import { selectCreatableProjects } from "../create/createForm.js";
import type { WorkItemCreateProject } from "../index.js";

// Both dialogs carry their own forms and reads; they mount only after an
// entry opens one, so they ride separate chunks (Quality gates).
const LazyProjectCreateEntry = React.lazy(() =>
  import("../../projects/components/ProjectCreateEntry.js").then((module) => ({
    default: module.ProjectCreateEntry,
  }))
);
const LazyWorkItemCreateDialog = React.lazy(() =>
  import("../components/WorkItemCreateDialog.js").then((module) => ({ default: module.WorkItemCreateDialog }))
);

export interface GlobalIssuesEmptyStateProps {
  workspaceSlug: string;
  /** Projects the caller can see; zero picks the project card instead. */
  projects: Project[];
}

export function GlobalIssuesEmptyState({ workspaceSlug, projects }: GlobalIssuesEmptyStateProps): React.ReactElement {
  const { canEdit } = usePermissions(workspaceSlug);
  const [createProjectOpen, setCreateProjectOpen] = React.useState(false);
  const [createWorkItemOpen, setCreateWorkItemOpen] = React.useState(false);
  const creatable: WorkItemCreateProject[] = React.useMemo(
    () =>
      selectCreatableProjects(
        projects.map((entry) => ({
          id: entry.id,
          name: entry.name,
          identifier: entry.identifier,
          memberRole: entry.member_role,
          archived: entry.archived_at !== null,
        }))
      ),
    [projects]
  );
  const projectDialog = createProjectOpen ? (
    <React.Suspense fallback={null}>
      <LazyProjectCreateEntry
        workspaceSlug={workspaceSlug}
        open={createProjectOpen}
        onOpenChange={setCreateProjectOpen}
      />
    </React.Suspense>
  ) : null;
  const workItemDialog = createWorkItemOpen ? (
    <React.Suspense fallback={null}>
      <LazyWorkItemCreateDialog
        open={createWorkItemOpen}
        onOpenChange={setCreateWorkItemOpen}
        workspaceSlug={workspaceSlug}
        projects={creatable}
      />
    </React.Suspense>
  ) : null;
  return (
    <>
      <GlobalIssuesEmpty
        projectCount={projects.length}
        // Both entries need only workspace standing: with no project to
        // file into, the create dialog states that itself once opened.
        canAct={canEdit}
        onCreateProject={() => {
          setCreateProjectOpen(true);
        }}
        onCreateWorkItem={() => {
          setCreateWorkItemOpen(true);
        }}
      />
      {projectDialog}
      {workItemDialog}
    </>
  );
}
