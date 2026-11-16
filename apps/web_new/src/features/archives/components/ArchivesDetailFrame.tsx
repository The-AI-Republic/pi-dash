// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-issue detail frame (parity/specs/archives.md §1 and §3.5).
// The detail address renders inside the archives chrome but without the
// tab strip: the breadcrumb trail (project, Archives, and the detail
// crumb once NEWFRONT-700 supplies it) plus the tab-content slot the
// detail content mounts into. No count badge on this screen.

import { useNavigate, useRouter } from "@tanstack/react-router";
import * as React from "react";

import { useArchivesProject } from "../api/project.js";
import { archivesTabHref } from "../tabs.js";
import { ArchivesHeader } from "./ArchivesHeader.js";
import { ArchivesTabSlot } from "./ArchivesTabSlot.js";

export interface ArchivesDetailFrameProps {
  workspaceSlug: string;
  projectId: string;
  /** Terminal crumb; absent until the detail slice names the item. */
  detailCrumb?: string | undefined;
  children?: React.ReactNode | undefined;
}

export function ArchivesDetailFrame({
  workspaceSlug,
  projectId,
  detailCrumb,
  children,
}: ArchivesDetailFrameProps): React.ReactElement | null {
  const navigate = useNavigate();
  const router = useRouter();
  const archives = useArchivesProject(workspaceSlug, projectId);
  if (archives === null) return null;

  function selectProject(nextId: string): void {
    if (nextId === projectId) return;
    void navigate({ to: archivesTabHref(workspaceSlug, nextId, "issues") });
  }

  return (
    <div data-testid="archives-detail-frame" className="flex min-w-0 flex-col gap-(--space-3)">
      <ArchivesHeader
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={archives.project.name}
        joinedProjects={archives.joined}
        terminal={detailCrumb ?? null}
        count={null}
        showBadge={false}
        onSelectProject={selectProject}
        onBack={() => router.history.back()}
      />
      <ArchivesTabSlot>{children}</ArchivesTabSlot>
    </div>
  );
}
