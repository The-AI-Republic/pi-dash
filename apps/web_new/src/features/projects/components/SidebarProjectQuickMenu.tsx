// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar project quick menu (parity/specs/shell.md §4, SHELL-050/052).
// The "…" menu every project row offers, shared by the sidebar row and
// the overflow panel: standing-gated entries (publish for project
// admins, archives for admin and member, leave for everyone else, copy
// and settings for all) with the publish and leave dialogs mounted
// beside it. Copy writes the tracking address; archives and settings
// load outside the client router.

import type { Project } from "@pidash/api-client";
import { IconButton, Menu } from "@pidash/kit";
import { Ellipsis } from "lucide-react";
import * as React from "react";

import type { CopyLinkMessages } from "../../../shared/shell/shareLink.js";
import { isProjectAdmin, isProjectAuthorized, selectQuickActions, type QuickActionId } from "../tabs.js";
import { LeaveProjectEntry } from "./LeaveProjectEntry.js";
import { PublishProjectEntry } from "./PublishProjectEntry.js";

/** Copy confirmation the parity driver reads (SHELL-050). */
export const ROW_LINK_COPIED_TITLE = "Link copied";
export const ROW_LINK_COPIED_BODY = "Paste it anywhere to share this project.";
export const ROW_COPY_FAILED_TITLE = "Couldn't copy the address";
export const ROW_COPY_FAILED_BODY = "The clipboard refused the write — try again.";

/** Toast copy for "Copy link"; shared by the row and the overflow panel. */
export const copyMessages: CopyLinkMessages = Object.freeze({
  copied: ROW_LINK_COPIED_TITLE,
  copiedBody: ROW_LINK_COPIED_BODY,
  failed: ROW_COPY_FAILED_TITLE,
  failedBody: ROW_COPY_FAILED_BODY,
});

export interface SidebarProjectQuickMenuProps {
  workspaceSlug: string;
  project: Project;
  /** Client-side navigation for the leave flow. */
  onNavigate: (href: string) => void;
  /** After an action that navigates away (archives, settings). */
  onActionNavigate?: (() => void) | undefined;
}

export function SidebarProjectQuickMenu({
  workspaceSlug,
  project,
  onNavigate,
  onActionNavigate,
}: SidebarProjectQuickMenuProps): React.ReactElement {
  const [publishOpen, setPublishOpen] = React.useState(false);
  const [leaveOpen, setLeaveOpen] = React.useState(false);

  async function copyLink(): Promise<void> {
    // Loaded on the click so the shell's first paint stays free of it.
    const { copyWorkItemLink } = await import("../../../shared/shell/shareLink.js");
    await copyWorkItemLink(`/${workspaceSlug}/projects/${project.id}/issues`, copyMessages);
  }

  function runAction(id: QuickActionId): void {
    if (id === "publish") setPublishOpen(true);
    else if (id === "copy") void copyLink();
    else if (id === "archives") {
      globalThis.window.location.assign(`/${workspaceSlug}/projects/${project.id}/archives/issues`);
      onActionNavigate?.();
    } else if (id === "settings") {
      globalThis.window.location.assign(`/${workspaceSlug}/settings/projects/${project.id}`);
      onActionNavigate?.();
    } else setLeaveOpen(true);
  }

  const actions = selectQuickActions({
    isAdmin: isProjectAdmin(project.member_role),
    isAuthorized: isProjectAuthorized(project.member_role),
  });

  return (
    <>
      <Menu
        trigger={
          <IconButton
            label={`Actions for ${project.name}`}
            data-testid="project-row-menu"
            className="shrink-0 opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100"
          >
            <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={actions.map((entry) => ({
          label: entry.label,
          itemKey: entry.id,
          onSelect: () => runAction(entry.id),
        }))}
      />
      <PublishProjectEntry
        workspaceSlug={workspaceSlug}
        projectId={project.id}
        projectName={project.name}
        open={publishOpen}
        onOpenChange={setPublishOpen}
      />
      <LeaveProjectEntry
        workspaceSlug={workspaceSlug}
        projectId={project.id}
        projectName={project.name}
        open={leaveOpen}
        onOpenChange={setLeaveOpen}
        onNavigate={onNavigate}
      />
    </>
  );
}
