// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project tab chrome (parity/specs/shell.md §7, SHELL-072–079). Pure
// composition: the header (switcher plus quick actions) beside the tab
// bar. Data arrives as props from the strip container.

import * as React from "react";

import { selectSwitcherTarget, type ProjectTab } from "../tabs.js";
import { ProjectQuickActions } from "./ProjectQuickActions.js";
import { ProjectSwitcher, type SwitcherProject } from "./ProjectSwitcher.js";
import { TabBar } from "./TabBar.js";

export interface ProjectTabChromeProps {
  workspaceSlug: string;
  project: SwitcherProject;
  joinedProjects: SwitcherProject[];
  tabs: ProjectTab[];
  activeKey: string | null;
  activeHref: string | null;
  defaultTab: string;
  hiddenTabs: string[];
  isAdmin: boolean;
  isAuthorized: boolean;
  onNavigate: (href: string) => void;
  onToggleDefault: (tabKey: string) => void;
  onHideTab: (tabKey: string) => void;
  onShowTab: (tabKey: string) => void;
}

export function ProjectTabChrome({
  workspaceSlug,
  project,
  joinedProjects,
  tabs,
  activeKey,
  activeHref,
  defaultTab,
  hiddenTabs,
  isAdmin,
  isAuthorized,
  onNavigate,
  onToggleDefault,
  onHideTab,
  onShowTab,
}: ProjectTabChromeProps): React.ReactElement {
  function selectProject(nextId: string): void {
    if (nextId === project.id) return;
    onNavigate(
      selectSwitcherTarget({
        currentTabs: tabs,
        currentDefaultKey: defaultTab,
        workspaceSlug,
        projectId: nextId,
      })
    );
  }

  return (
    <nav
      aria-label="Project tabs"
      data-testid="project-tab-strip"
      className="flex h-12 shrink-0 items-center gap-(--space-2) border-b border-(--border) bg-(--bg) pr-(--space-2) pl-(--space-1)"
    >
      <div className="flex shrink-0 items-center gap-(--space-1)">
        <ProjectSwitcher current={project} joined={joinedProjects} onSelectProject={selectProject} />
        <ProjectQuickActions
          workspaceSlug={workspaceSlug}
          projectId={project.id}
          projectName={project.name}
          activeHref={activeHref}
          isAdmin={isAdmin}
          isAuthorized={isAuthorized}
          onNavigate={onNavigate}
        />
      </div>
      <TabBar
        tabs={tabs}
        activeKey={activeKey}
        defaultTab={defaultTab}
        hiddenTabs={hiddenTabs}
        onToggleDefault={onToggleDefault}
        onHideTab={onHideTab}
        onShowTab={onShowTab}
      />
    </nav>
  );
}
