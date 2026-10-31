// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archives shell (parity/specs/archives.md §1, ARCH-001–003). The shared
// frame every archives tab renders inside: the breadcrumb header with
// the count badge, the gated tab strip, and the tab-content slot the
// tab slices mount into. Tab routes own their titles; the shell owns
// the chrome. The archived-issue detail uses the detail frame instead
// (no strip, own breadcrumb).

import { useNavigate, useRouter } from "@tanstack/react-router";
import * as React from "react";

import { useArchivedIssuesCount } from "../api/count.js";
import { useArchivesProject } from "../api/project.js";
import { archivesTabDef, archivesTabHref, selectArchivesTabs, type ArchivesTabKey } from "../tabs.js";
import { ArchivesHeader } from "./ArchivesHeader.js";
import { ArchivesTabSlot } from "./ArchivesTabSlot.js";
import { ArchivesTabStrip } from "./ArchivesTabStrip.js";

export interface ArchivesShellProps {
  workspaceSlug: string;
  projectId: string;
  activeTab: ArchivesTabKey;
  children?: React.ReactNode | undefined;
}

export function ArchivesShell({
  workspaceSlug,
  projectId,
  activeTab,
  children,
}: ArchivesShellProps): React.ReactElement | null {
  const navigate = useNavigate();
  const router = useRouter();
  const archives = useArchivesProject(workspaceSlug, projectId);
  const count = useArchivedIssuesCount(workspaceSlug, projectId);
  if (archives === null) return null;
  const tabs = selectArchivesTabs({ workspaceSlug, projectId, flags: archives.project });

  function selectProject(nextId: string): void {
    if (nextId === projectId) return;
    void navigate({ to: archivesTabHref(workspaceSlug, nextId, activeTab) });
  }

  return (
    <div data-testid="archives-shell" className="flex min-w-0 flex-col gap-(--space-3)">
      <ArchivesHeader
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={archives.project.name}
        joinedProjects={archives.joined}
        terminal={archivesTabDef(activeTab).label}
        count={count.data}
        showBadge={activeTab === "issues"}
        onSelectProject={selectProject}
        onBack={() => router.history.back()}
      />
      <ArchivesTabStrip tabs={tabs} activeTab={activeTab} />
      <ArchivesTabSlot>{children}</ArchivesTabSlot>
    </div>
  );
}
