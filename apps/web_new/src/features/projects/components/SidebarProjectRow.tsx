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
import { IconButton, Menu } from "@pidash/kit";
import { ChevronRight, Ellipsis, GripVertical } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { isClientSideClick } from "../../../shared/shell/index.js";
import type { CopyLinkMessages } from "../../../shared/shell/shareLink.js";
import { projectPropertiesQueryOptions } from "../api/tabPreferences.js";
import {
  isProjectAdmin,
  isProjectAuthorized,
  parseNavigationPrefs,
  selectProjectTabs,
  selectQuickActions,
  type QuickActionId,
} from "../tabs.js";
import { isSubnavEntryActive, resolveProjectRowHref, revealRow, selectSubnavEntries } from "../sidebar.js";
import { LeaveProjectEntry } from "./LeaveProjectEntry.js";
import { PublishProjectEntry } from "./PublishProjectEntry.js";

/** Copy confirmation the parity driver reads (SHELL-050). */
export const ROW_LINK_COPIED_TITLE = "Link copied";
export const ROW_LINK_COPIED_BODY = "Paste it anywhere to share this project.";
export const ROW_COPY_FAILED_TITLE = "Couldn't copy the address";
export const ROW_COPY_FAILED_BODY = "The clipboard refused the write — try again.";

const ROW_LINK_TONE =
  "text-body flex min-w-0 flex-1 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left hover:bg-(--subtle)";
const SUBNAV_LINK_TONE =
  "text-body flex w-full items-center justify-between gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle)";

/** Toast copy for "Copy link"; shared by the row and the overflow panel. */
export const copyMessages: CopyLinkMessages = Object.freeze({
  copied: ROW_LINK_COPIED_TITLE,
  copiedBody: ROW_LINK_COPIED_BODY,
  failed: ROW_COPY_FAILED_TITLE,
  failedBody: ROW_COPY_FAILED_BODY,
});

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
  const [publishOpen, setPublishOpen] = React.useState(false);
  const [leaveOpen, setLeaveOpen] = React.useState(false);
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
  const subnav = selectSubnavEntries({
    workspaceSlug,
    projectId: project.id,
    flags: project,
    role: project.member_role,
    intakeCount: project.intake_count,
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

  async function copyLink(): Promise<void> {
    // Loaded on the click so the shell's first paint stays free of it.
    const { copyWorkItemLink } = await import("../../../shared/shell/shareLink.js");
    await copyWorkItemLink(`/${workspaceSlug}/projects/${project.id}/issues`, copyMessages);
  }

  function runAction(id: QuickActionId): void {
    if (id === "publish") setPublishOpen(true);
    else if (id === "copy") void copyLink();
    else if (id === "archives")
      globalThis.window.location.assign(`/${workspaceSlug}/projects/${project.id}/archives/issues`);
    else if (id === "settings") globalThis.window.location.assign(`/${workspaceSlug}/settings/projects/${project.id}`);
    else setLeaveOpen(true);
  }

  const actions = selectQuickActions({
    isAdmin: isProjectAdmin(project.member_role),
    isAuthorized: isProjectAuthorized(project.member_role),
  });

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
          <button
            type="button"
            aria-expanded={expanded}
            aria-label={expanded ? `Collapse ${project.name}` : `Expand ${project.name}`}
            data-testid="project-row-toggle"
            onClick={() => setExpanded(!expanded)}
            className="shrink-0 rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle)"
          >
            <ChevronRight
              size={14}
              strokeWidth={2}
              aria-hidden="true"
              className={expanded ? "rotate-90 transition-transform" : "transition-transform"}
            />
          </button>
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
      </div>
      {tabbed || !expanded ? null : (
        <ul
          aria-label={`${project.name} sections`}
          data-testid="project-subnav"
          className="mt-px flex flex-col gap-px pl-6"
        >
          {subnav.map((entry) => {
            const active = currentPath !== null && isSubnavEntryActive(currentPath, entry.href);
            const tone = active
              ? `${SUBNAV_LINK_TONE} bg-(--subtle) text-(--text)`
              : `${SUBNAV_LINK_TONE} text-(--text-muted)`;
            const label = (
              <>
                <span className="truncate">{entry.name}</span>
                {entry.badge === null ? null : (
                  <span className="text-caption shrink-0 text-(--text-muted)">{entry.badge}</span>
                )}
              </>
            );
            return (
              <li key={entry.key}>
                {entry.key === "runners" ? (
                  <CrossAppLink
                    to={entry.href}
                    aria-current={active ? "page" : undefined}
                    data-testid="project-subnav-link"
                    className={tone}
                  >
                    {label}
                  </CrossAppLink>
                ) : (
                  <a
                    href={entry.href}
                    onClick={(event) => entryClick(event, entry.href)}
                    aria-current={active ? "page" : undefined}
                    data-testid="project-subnav-link"
                    className={tone}
                  >
                    {label}
                  </a>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {dropIndicator === "below" ? (
        <span data-testid="project-drop-indicator" className="absolute inset-x-2 bottom-0 h-0.5 bg-(--accent)" />
      ) : null}
      <PublishProjectEntry projectName={project.name} open={publishOpen} onOpenChange={setPublishOpen} />
      <LeaveProjectEntry
        workspaceSlug={workspaceSlug}
        projectId={project.id}
        projectName={project.name}
        open={leaveOpen}
        onOpenChange={setLeaveOpen}
        onNavigate={onNavigate}
      />
    </li>
  );
}
