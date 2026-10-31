// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project quick actions (parity/specs/shell.md §7, SHELL-073/074). The
// three-dot menu beside the header: publish, copy-location, archives,
// settings and leave, each gated by project standing.

import { IconButton, Menu } from "@pidash/kit";
import { Ellipsis } from "lucide-react";
import * as React from "react";

import { copyWorkItemLink } from "../../../shared/shell/shareLink.js";
import { selectCopyPath, selectQuickActions, type QuickActionId } from "../tabs.js";
import { LeaveProjectEntry } from "./LeaveProjectEntry.js";
import { PublishProjectEntry } from "./PublishProjectEntry.js";

const COPIED_TITLE = "Copied the project link";
const COPIED_BODY = "Paste it anywhere to share this tab.";
const COPY_FAILED_TITLE = "Couldn't copy the address";
const COPY_FAILED_BODY = "The clipboard refused the write — try again.";
const COPY_LINK_MESSAGES = {
  copied: COPIED_TITLE,
  copiedBody: COPIED_BODY,
  failed: COPY_FAILED_TITLE,
  failedBody: COPY_FAILED_BODY,
} as const;

export interface ProjectQuickActionsProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  activeHref: string | null;
  isAdmin: boolean;
  isAuthorized: boolean;
  onNavigate: (href: string) => void;
}

export function ProjectQuickActions({
  workspaceSlug,
  projectId,
  projectName,
  activeHref,
  isAdmin,
  isAuthorized,
  onNavigate,
}: ProjectQuickActionsProps): React.ReactElement {
  const [publishOpen, setPublishOpen] = React.useState(false);
  const [leaveOpen, setLeaveOpen] = React.useState(false);

  async function copyLocation(): Promise<void> {
    await copyWorkItemLink(selectCopyPath({ activeHref, workspaceSlug, projectId }), COPY_LINK_MESSAGES);
  }

  function runAction(id: QuickActionId): void {
    if (id === "publish") setPublishOpen(true);
    else if (id === "copy") void copyLocation();
    else if (id === "archives") onNavigate(`/${workspaceSlug}/projects/${projectId}/archives/issues`);
    else if (id === "settings") onNavigate(`/${workspaceSlug}/settings/projects/${projectId}`);
    else setLeaveOpen(true);
  }

  return (
    <>
      <Menu
        trigger={
          <IconButton label="Project actions" data-testid="project-actions-trigger">
            <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={selectQuickActions({ isAdmin, isAuthorized }).map((entry) => ({
          label: entry.label,
          itemKey: entry.id,
          onSelect: () => runAction(entry.id),
        }))}
      />
      <PublishProjectEntry
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={projectName}
        open={publishOpen}
        onOpenChange={setPublishOpen}
      />
      <LeaveProjectEntry
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={projectName}
        open={leaveOpen}
        onOpenChange={setLeaveOpen}
        onNavigate={onNavigate}
      />
    </>
  );
}
