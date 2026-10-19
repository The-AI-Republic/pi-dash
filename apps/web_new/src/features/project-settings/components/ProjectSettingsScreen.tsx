// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One settings address (PSET-001/PSET-004): the shell around the tab
// body, the page header, then the tab gate. Every leaf route composes
// this so a direct address and a sidebar click land on the same screen
// and the role matrix holds either way.

import * as React from "react";

import { ProjectSettingsPage } from "./ProjectSettingsPage.js";
import { ProjectSettingsShell, ProjectSettingsTabGate } from "./ProjectSettingsLayout.js";
import { findTabByKey, type ProjectSettingsTabKey } from "../tabs.js";

export interface ProjectSettingsScreenProps {
  workspaceSlug: string;
  projectId: string;
  tabKey: ProjectSettingsTabKey;
  /** False for screens that bring their own header (labels, automations). */
  pageHeader?: boolean;
  children: React.ReactNode;
}

export function ProjectSettingsScreen({
  workspaceSlug,
  projectId,
  tabKey,
  pageHeader = true,
  children,
}: ProjectSettingsScreenProps): React.ReactElement {
  const title = findTabByKey(tabKey)?.label ?? tabKey;
  return (
    <ProjectSettingsShell workspaceSlug={workspaceSlug} projectId={projectId}>
      <ProjectSettingsTabGate tabKey={tabKey}>
        {pageHeader ? <ProjectSettingsPage title={title}>{children}</ProjectSettingsPage> : children}
      </ProjectSettingsTabGate>
    </ProjectSettingsShell>
  );
}
