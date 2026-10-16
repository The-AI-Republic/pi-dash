// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace top bar (SHELL-065, SHELL-066, SHELL-099, SHELL-100, from
// parity/specs/shell.md §5). Left: the workspace menu mount plus the
// sidebar toggle. Middle: the command-search mount. Right: the inbox
// link with its unread dot, the help mount, the repository star link,
// and the compact account mount, which appears only where the sidebar
// is unmounted or collapsed. The workspace menu, search box, help and
// account mounts are filled by their owning slices (NEWFRONT-287,
// NEWFRONT-292); this slice owns the composition, the toggle, the
// inbox dot rule and the star link. No edition badge mounts here.
// On desktop (platform.window present) the header doubles as the
// window drag region, like TitleBar.
import { IconButton } from "@pidash/kit";
import { Inbox, PanelLeft, Star } from "lucide-react";
import * as React from "react";

import { platform } from "../../core/platform/index.js";
import { REPOSITORY_URL } from "./RepoStar.js";
import { isNotificationsPath } from "./sidebarAutohide.js";
import { useWindowTitle } from "./windowTitle.js";

export interface TopBarProps {
  workspaceSlug: string;
  /** Location section mirrored into the desktop window title. */
  section: string;
  /** Current pathname, for the inbox active state. */
  currentPath: string;
  /** False on routes that unmount the sidebar (then the toggle hides and the account mount shows). */
  sidebarMounted?: boolean;
  sidebarCollapsed: boolean;
  /** Edition/desktop gate for the toggle; visible unless an edition hides it. */
  sidebarToggleEnabled?: boolean;
  onToggleSidebar: () => void;
  unreadTotal: number;
  unreadMentions: number;
  /** Filled by the workspace-menu slice (NEWFRONT-287). */
  workspaceMenu?: React.ReactNode;
  /** Filled by the palette slice (NEWFRONT-292, SHELL-081). */
  searchBox?: React.ReactNode;
  /** Filled by the help-menu slice (NEWFRONT-287). */
  helpMenu?: React.ReactNode;
  /** Compact account control (NEWFRONT-287); shown only as the fallback. */
  accountMenu?: React.ReactNode;
}

export function TopBar({
  workspaceSlug,
  section,
  currentPath,
  sidebarMounted = true,
  sidebarCollapsed,
  sidebarToggleEnabled = true,
  onToggleSidebar,
  unreadTotal,
  unreadMentions,
  workspaceMenu,
  searchBox,
  helpMenu,
  accountMenu,
}: TopBarProps): React.ReactElement {
  const desktop = platform.window;
  const title = section ? `${section} — Pi Dash` : "Pi Dash";
  useWindowTitle(title);

  const showToggle = sidebarMounted && sidebarToggleEnabled;
  const toggleHint = sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar";
  const hasUnread = unreadTotal > 0 || unreadMentions > 0;
  const inboxActive = isNotificationsPath(currentPath);
  const showAccountFallback = !sidebarMounted || sidebarCollapsed;

  return (
    <header
      {...(desktop ? { "data-tauri-drag-region": true } : {})}
      className={`flex h-12 shrink-0 items-center gap-(--space-4) border-b border-(--border) bg-(--bg) px-(--space-4)${
        desktop ? " select-none" : ""
      }${desktop?.trafficLightInset ? " pl-20" : ""}`}
    >
      <div className="flex flex-1 items-center gap-(--space-2)">
        {workspaceMenu}
        {showToggle ? (
          // Native title, not the kit Tooltip: the toggle is the only
          // tooltip user in the shell, and the floating-positioning
          // subtree would ride the initial-JS chain (size budget).
          <IconButton label={toggleHint} title={toggleHint} data-testid="sidebar-toggle" onClick={onToggleSidebar}>
            <PanelLeft size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        ) : null}
      </div>
      <div className="flex shrink-0 items-center">{searchBox}</div>
      <div className="flex flex-1 items-center justify-end gap-(--space-2)">
        <a
          href={`/${workspaceSlug}/notifications/`}
          aria-label="Inbox"
          aria-current={inboxActive ? "page" : undefined}
          className="text-body inline-flex items-center rounded-(--radius-control) px-(--space-2) py-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <div className="relative">
            <Inbox size={16} strokeWidth={1.5} aria-hidden="true" />
            {hasUnread ? (
              <span aria-hidden="true" className="absolute top-0 right-0 size-2 rounded-full bg-(--danger)" />
            ) : null}
          </div>
        </a>
        {helpMenu}
        <a
          href={REPOSITORY_URL}
          target="_blank"
          rel="noopener noreferrer"
          aria-label="Star us on GitHub"
          className="text-body inline-flex shrink-0 items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <Star size={16} strokeWidth={1.5} aria-hidden="true" />
          <span className="hidden md:inline">Star us on GitHub</span>
        </a>
        {showAccountFallback ? accountMenu : null}
      </div>
    </header>
  );
}
