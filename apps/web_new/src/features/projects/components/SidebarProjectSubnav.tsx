// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar project sub-navigation (parity/specs/shell.md §4, SHELL-050/052/055).
// The ">" expand toggle plus the section list under it, shared by the
// sidebar row and the overflow panel: flag- and role-gated entries with
// slashed addresses (tabs plus the workers panel) and the nonzero intake
// badge. Link targets belong to their own areas; the shell proves hrefs
// plus gating only.

import type { Project } from "@pidash/api-client";
import { ChevronRight } from "lucide-react";
import * as React from "react";

import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { isSubnavEntryActive, selectSubnavEntries } from "../sidebar.js";

const SUBNAV_LINK_TONE =
  "text-body flex w-full items-center justify-between gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle)";

export interface SidebarProjectExpandToggleProps {
  projectName: string;
  expanded: boolean;
  onToggle: () => void;
}

export function SidebarProjectExpandToggle({
  projectName,
  expanded,
  onToggle,
}: SidebarProjectExpandToggleProps): React.ReactElement {
  return (
    <button
      type="button"
      aria-expanded={expanded}
      aria-label={expanded ? `Collapse ${projectName}` : `Expand ${projectName}`}
      data-testid="project-row-toggle"
      onClick={onToggle}
      className="shrink-0 rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle)"
    >
      <ChevronRight
        size={14}
        strokeWidth={2}
        aria-hidden="true"
        className={expanded ? "rotate-90 transition-transform" : "transition-transform"}
      />
    </button>
  );
}

export interface SidebarProjectSubnavProps {
  workspaceSlug: string;
  project: Project;
  /** Current pathname; null while the router has not reported it. */
  currentPath: string | null;
  /** Entry activation; the caller decides client-side versus browser handling. */
  onEntryClick: (event: React.MouseEvent<HTMLAnchorElement>, href: string) => void;
  /** Runners entry activation; it navigates through the router itself. */
  onRunnersClick?: ((event: React.MouseEvent<HTMLAnchorElement>) => void) | undefined;
}

export function SidebarProjectSubnav({
  workspaceSlug,
  project,
  currentPath,
  onEntryClick,
  onRunnersClick,
}: SidebarProjectSubnavProps): React.ReactElement {
  const subnav = selectSubnavEntries({
    workspaceSlug,
    projectId: project.id,
    flags: project,
    role: project.member_role,
    intakeCount: project.intake_count,
  });
  return (
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
                onClick={onRunnersClick}
                aria-current={active ? "page" : undefined}
                data-testid="project-subnav-link"
                className={tone}
              >
                {label}
              </CrossAppLink>
            ) : (
              <a
                href={entry.href}
                onClick={(event) => onEntryClick(event, entry.href)}
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
  );
}
