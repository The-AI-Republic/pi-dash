// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar project row (parity/specs/shell.md §4, SHELL-050/051/055).
// Links the member's default tab, expands inline sub-navigation in
// accordion mode, scrolls the active project into view, offers the
// standing-gated quick menu, and drags from a hover handle. The row
// reads the default tab from the member store without fetching it:
// a cold store links tracking until a project route warms it. The
// sub-navigation entries (SHELL-055: tabs plus the workers panel,
// flag- and role-gated with the nonzero intake badge) render from
// the shared selector so the shell proves hrefs plus gating only;
// link targets belong to their own areas.

import { useQuery } from "@tanstack/react-query";
import type { Project } from "@pidash/api-client";
import { GripVertical } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { isClientSideClick } from "../../../shared/shell/index.js";
import { projectPropertiesQueryOptions } from "../api/tabPreferences.js";
import { parseNavigationPrefs, selectProjectTabs } from "../tabs.js";
import { resolveProjectRowHref, revealRow } from "../sidebar.js";
import {
  SidebarProjectQuickMenu,
  copyMessages,
  ROW_COPY_FAILED_BODY,
  ROW_COPY_FAILED_TITLE,
  ROW_LINK_COPIED_BODY,
  ROW_LINK_COPIED_TITLE,
} from "./SidebarProjectQuickMenu.js";
import { SidebarProjectExpandToggle, SidebarProjectSubnav } from "./SidebarProjectSubnav.js";

/** Copy confirmation the parity driver reads (SHELL-050). */
export { copyMessages, ROW_COPY_FAILED_BODY, ROW_COPY_FAILED_TITLE, ROW_LINK_COPIED_BODY, ROW_LINK_COPIED_TITLE };

const ROW_LINK_TONE =
  "text-body flex min-w-0 flex-1 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left hover:bg-(--subtle)";

export interface SidebarProjectRowProps {
  workspaceSlug: string;
  project: Project;
  /** Current pathname; null while the router has not reported it. */
  currentPath: string | null;
  /** Project id of the route in view, if any. */
  activeProjectId: string | null;
  /** Tabbed mode navigates on click; accordion mode expands instead. */
  tabbed: boolean;
  /** Client-side navigation for plain left clicks. */
  onNavigate: (href: string) => void;
  /** Drop indicator slot while another row drags over this one. */
  dropIndicator: "above" | "below" | null;
  /** True while this row is the drag source. */
  isDragging: boolean;
  /** Starts a reorder drag from this row's handle. */
  onDragStart: (projectId: string) => void;
}

export function SidebarProjectRow({
  workspaceSlug,
  project,
  currentPath,
  activeProjectId,
  tabbed,
  onNavigate,
  dropIndicator,
  isDragging,
  onDragStart,
}: SidebarProjectRowProps): React.ReactElement {
  const isUrlProject = activeProjectId === project.id;
  const [expanded, setExpanded] = React.useState(isUrlProject);
  // Cache-only read: the member store warms on project routes, and a
  // cold store links the tracking fallback (SHELL-050).
  const stored = useQuery({
    ...projectPropertiesQueryOptions(getAppClient(), workspaceSlug, project.id),
    enabled: false,
  });
  const tabs = selectProjectTabs({
    workspaceSlug,
    projectId: project.id,
    flags: project,
    role: project.member_role,
  });
  const href = resolveProjectRowHref({
    tabs,
    defaultTab: parseNavigationPrefs(stored.data?.preferences).defaultTab,
    workspaceSlug,
    projectId: project.id,
  });
  const reveal = React.useCallback((row: HTMLLIElement | null) => revealRow(row, isUrlProject), [isUrlProject]);
  const sortable = project.sort_order !== null;
  const highlighted = tabbed && isUrlProject;

  React.useEffect(() => {
    if (activeProjectId === project.id) setExpanded(true);
  }, [activeProjectId, project.id]);

  function rowClick(event: React.MouseEvent<HTMLAnchorElement>): void {
    if (!isClientSideClick(event)) return;
    event.preventDefault();
    if (tabbed) onNavigate(href);
    else setExpanded(!expanded);
  }

  function entryClick(event: React.MouseEvent<HTMLAnchorElement>, target: string): void {
    if (!isClientSideClick(event)) return;
    event.preventDefault();
    onNavigate(target);
  }

  return (
    <li ref={reveal} data-project-row={project.id} className="relative">
      {dropIndicator === "above" ? (
        <span data-testid="project-drop-indicator" className="absolute inset-x-2 top-0 h-0.5 bg-(--accent)" />
      ) : null}
      <div
        className={
          isDragging
            ? "group/row relative flex w-full items-center rounded-(--radius-control) opacity-60"
            : "group/row relative flex w-full items-center rounded-(--radius-control)"
        }
      >
        {sortable ? (
          <button
            type="button"
            aria-label={`Rearrange ${project.name}`}
            title="Drag to reorder"
            data-testid="project-drag-handle"
            onPointerDown={(event) => {
              event.preventDefault();
              onDragStart(project.id);
            }}
            className="absolute top-1/2 -left-3 hidden -translate-y-1/2 cursor-grab items-center justify-center rounded-(--radius-control) p-px text-(--text-muted) group-hover/row:flex"
          >
            <GripVertical size={14} strokeWidth={2} aria-hidden="true" />
          </button>
        ) : (
          <span
            title="Join this project to reorder it"
            className="absolute top-1/2 -left-3 hidden -translate-y-1/2 group-hover/row:flex"
            aria-hidden="true"
          >
            <GripVertical size={14} strokeWidth={2} className="opacity-40" />
          </span>
        )}
        {tabbed ? null : (
          <SidebarProjectExpandToggle
            projectName={project.name}
            expanded={expanded}
            onToggle={() => setExpanded(!expanded)}
          />
        )}
        <a
          href={href}
          onClick={rowClick}
          aria-current={highlighted ? "page" : undefined}
          data-testid="project-row-link"
          className={
            highlighted ? `${ROW_LINK_TONE} bg-(--subtle) text-(--text)` : `${ROW_LINK_TONE} text-(--text-muted)`
          }
        >
          <span className="truncate">{project.name}</span>
        </a>
        <SidebarProjectQuickMenu workspaceSlug={workspaceSlug} project={project} onNavigate={onNavigate} />
      </div>
      {tabbed || !expanded ? null : (
        <SidebarProjectSubnav
          workspaceSlug={workspaceSlug}
          project={project}
          currentPath={currentPath}
          onEntryClick={entryClick}
        />
      )}
      {dropIndicator === "below" ? (
        <span data-testid="project-drop-indicator" className="absolute inset-x-2 bottom-0 h-0.5 bg-(--accent)" />
      ) : null}
    </li>
  );
}
