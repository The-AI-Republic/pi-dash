// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar secondary-destinations disclosure (parity/specs/shell.md §4,
// SHELL-053/054). The collapsing More group listing the workspace
// destinations outside the main sidebar in catalogue order, gated by
// role. Landing on a hosted member route expands the group so the
// active row is visible, and the toggle persists across reloads. The
// panel unmounts while closed, so hidden destinations never leak into
// the main sidebar's links.

import {
  Archive,
  Bot,
  CalendarClock,
  ChartColumn,
  ChevronRight,
  FolderKanban,
  MessageSquare,
  Monitor,
  Repeat,
} from "lucide-react";
import * as React from "react";

import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { isMoreEntryActive, isMoreRoute, loadMoreOpen, persistMoreOpen, selectMoreEntries } from "../more.js";

export interface SidebarMoreGroupProps {
  workspaceSlug: string;
  /** Workspace role; null reads as guest level. */
  role: number | null;
  /** Current pathname; null while the router has not reported it. */
  currentPath: string | null;
}

const DESTINATION_ICONS: Record<
  string,
  React.ComponentType<{
    size?: number | string;
    strokeWidth?: number | string;
    className?: string;
    "aria-hidden"?: boolean | "true" | "false";
  }>
> = {
  projects: FolderKanban,
  runners: Bot,
  prompts: MessageSquare,
  schedulers: CalendarClock,
  "ai-dev-machines": Monitor,
  analytics: ChartColumn,
  archives: Archive,
  "active-cycles": Repeat,
};

export function SidebarMoreGroup({ workspaceSlug, role, currentPath }: SidebarMoreGroupProps): React.ReactElement {
  const [open, setOpen] = React.useState(loadMoreOpen);
  const entries = selectMoreEntries({ workspaceSlug, role });

  function setPersisted(next: boolean): void {
    persistMoreOpen(next);
    setOpen(next);
  }

  React.useEffect(() => {
    if (currentPath !== null && isMoreRoute(currentPath, workspaceSlug)) setPersisted(true);
  }, [currentPath, workspaceSlug]);

  return (
    <section aria-label="More" data-testid="sidebar-more-group" className="flex flex-col gap-(--space-1)">
      <div className="flex w-full items-center justify-between rounded-(--radius-control) px-(--space-2) py-(--space-1)">
        <button
          type="button"
          aria-expanded={open}
          aria-label={open ? "Collapse more destinations" : "Expand more destinations"}
          data-testid="more-section-toggle"
          onClick={() => setPersisted(!open)}
          className="text-caption flex flex-1 cursor-pointer items-center gap-(--space-1) text-left text-(--text-muted)"
        >
          <span className="font-semibold">More</span>
          <ChevronRight
            size={14}
            strokeWidth={2}
            aria-hidden="true"
            className={open ? "rotate-90 transition-transform" : "transition-transform"}
          />
        </button>
      </div>
      {!open ? null : (
        <ul data-testid="more-section-panel" className="flex flex-col gap-px">
          {entries.map((entry) => {
            const active = currentPath !== null && isMoreEntryActive(currentPath, entry.href);
            const Icon = DESTINATION_ICONS[entry.key] ?? FolderKanban;
            return (
              <li key={entry.key}>
                <CrossAppLink
                  to={entry.href}
                  aria-current={active ? "page" : undefined}
                  data-testid="more-section-link"
                  className={
                    active
                      ? "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-2) text-left text-(--text)"
                      : "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                  }
                >
                  <Icon size={16} strokeWidth={1.5} aria-hidden="true" />
                  <span className="truncate">{entry.name}</span>
                </CrossAppLink>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
