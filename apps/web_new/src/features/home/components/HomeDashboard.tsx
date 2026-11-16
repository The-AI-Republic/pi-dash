// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home dashboard composition (SHELL-003; written from parity/specs/shell.md
// §2). The greeting, assistant card and quickstart checklist render inside
// a centered column once workspace context exists, with the workspace name
// in the tab title. The widget stack below the top half is a slot the
// sibling home slices (NEWFRONT-279, NEWFRONT-280) fill; the dashboard
// never forks their composition, it only hosts it.

import type { ProjectList } from "@pidash/api-client";
import * as React from "react";

import { useWorkspace } from "../../../core/session/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { HomeAssistantCard } from "./HomeAssistantCard.js";
import { HomeGreeting } from "./HomeGreeting.js";
import { QuickstartGuide } from "./QuickstartGuide.js";

export interface HomeDashboardProps {
  workspaceSlug: string;
  /** Workspace projects for the quickstart create card; the route reads them. */
  projects: ProjectList | undefined;
  /**
   * Opens the create-project dialog from the quickstart create card.
   * Owned by the projects area (NEWFRONT-284); the card links to the
   * projects list until it lands.
   */
  onCreateProject?: (() => void) | undefined;
  /**
   * The widget stack below the home top half. Owned by the sibling home
   * slices (NEWFRONT-279, NEWFRONT-280); empty until they land.
   */
  widgets?: React.ReactNode;
}

export function HomeDashboard({
  workspaceSlug,
  projects,
  onCreateProject,
  widgets,
}: HomeDashboardProps): React.ReactElement | null {
  const { workspace } = useWorkspace(workspaceSlug);
  useDocumentHead({
    title: workspace === undefined ? `${workspaceSlug} · Home` : `${workspace.name} · Home`,
    robots: "noindex, nofollow",
  });
  if (workspace === undefined) return null;
  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-(--space-8) px-(--space-6) py-(--space-8)">
      <HomeGreeting />
      <HomeAssistantCard workspaceSlug={workspaceSlug} />
      {/* Remount per workspace so the checklist re-reads that workspace's
          dismissal and visit flags instead of keeping the previous one's. */}
      <QuickstartGuide
        key={workspaceSlug}
        workspaceSlug={workspaceSlug}
        projects={projects}
        {...(onCreateProject === undefined ? {} : { onCreateProject })}
      />
      {widgets}
    </div>
  );
}
