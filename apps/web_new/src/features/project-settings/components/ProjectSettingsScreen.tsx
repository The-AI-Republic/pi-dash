// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One settings address (PSET-001/PSET-004): the shell around the tab
// body, the page header, then the tab gate. Every leaf route composes
// this so a direct address and a sidebar click land on the same screen
// and the role matrix holds either way. A tab whose own screen renders
// the refusal (automations, PSET-053/054) asks for the body-owned gate:
// the shell and the rail stay, and the body decides once it has the
// standing it reads, instead of being shadowed by the generic refusal.

import * as React from "react";

import { ProjectSettingsPage } from "./ProjectSettingsPage.js";
import { ProjectSettingsShell, ProjectSettingsTabGate } from "./ProjectSettingsLayout.js";
import { findTabByKey, type ProjectSettingsTabKey } from "../tabs.js";

/**
 * Who renders a refused page: the shell's generic tab gate, or the tab
 * body itself. The rail is kept either way.
 */
export type ProjectSettingsGate = "tab" | "body";

export interface ProjectSettingsScreenProps {
  workspaceSlug: string;
  projectId: string;
  tabKey: ProjectSettingsTabKey;
  /** False for screens that bring their own header (labels, automations). */
  pageHeader?: boolean;
  /** Defaults to the shell's tab gate; `body` lets the screen refuse. */
  gate?: ProjectSettingsGate;
  children: React.ReactNode;
}

export function ProjectSettingsScreen({
  workspaceSlug,
  projectId,
  tabKey,
  pageHeader = true,
  gate = "tab",
  children,
}: ProjectSettingsScreenProps): React.ReactElement {
  const title = findTabByKey(tabKey)?.label ?? tabKey;
  const page = pageHeader ? <ProjectSettingsPage title={title}>{children}</ProjectSettingsPage> : children;
  return (
    <ProjectSettingsShell workspaceSlug={workspaceSlug} projectId={projectId}>
      {gate === "body" ? page : <ProjectSettingsTabGate tabKey={tabKey}>{page}</ProjectSettingsTabGate>}
    </ProjectSettingsShell>
  );
}
