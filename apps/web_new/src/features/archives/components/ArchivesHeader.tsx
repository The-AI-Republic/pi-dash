// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archives header (parity/specs/archives.md §1.3, ARCH-002). The trail
// reads project, then Archives, then the active tab; the Archives crumb
// links to the work-items tab from every tab. The back control renders
// only at narrow viewports and returns to the previous screen; the
// archived-count badge renders beside the trail on the work-items tab.

import { Link } from "@tanstack/react-router";
import { ChevronLeft, ChevronRight } from "lucide-react";
import * as React from "react";

import { ProjectSwitcher, type SwitcherProject } from "../../projects/index.js";
import { useArchivesBackVisible } from "../lib/narrow.js";
import { archivesTabHref } from "../tabs.js";
import { ArchivesCountBadge } from "./ArchivesCountBadge.js";

export interface ArchivesHeaderProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  joinedProjects: SwitcherProject[];
  /** Terminal crumb, or null when the trail ends at Archives. */
  terminal: string | null;
  /** Archived total; shown only when showBadge is true. */
  count: number | null | undefined;
  showBadge: boolean;
  onSelectProject: (projectId: string) => void;
  onBack: () => void;
}

export function ArchivesHeader({
  workspaceSlug,
  projectId,
  projectName,
  joinedProjects,
  terminal,
  count,
  showBadge,
  onSelectProject,
  onBack,
}: ArchivesHeaderProps): React.ReactElement {
  const backVisible = useArchivesBackVisible();
  return (
    <div className="flex min-w-0 items-center gap-(--space-2)">
      {backVisible ? (
        <button
          type="button"
          aria-label="Back"
          data-testid="archives-back"
          onClick={onBack}
          className="shrink-0 rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <ChevronLeft size={16} strokeWidth={2} aria-hidden="true" />
        </button>
      ) : null}
      <nav aria-label="Breadcrumb" className="min-w-0 flex-1">
        <ol className="flex min-w-0 items-center gap-(--space-1)">
          <li className="flex min-w-0 items-center">
            <ProjectSwitcher
              current={{ id: projectId, name: projectName }}
              joined={joinedProjects}
              onSelectProject={onSelectProject}
            />
          </li>
          <li className="flex shrink-0 items-center gap-(--space-1)">
            <ChevronRight size={14} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
            <Link
              to={archivesTabHref(workspaceSlug, projectId, "issues")}
              className="text-body rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
            >
              Archives
            </Link>
          </li>
          {terminal === null ? null : (
            <li className="flex min-w-0 items-center gap-(--space-1)">
              <ChevronRight size={14} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
              <span aria-current="page" className="text-body max-w-40 truncate text-(--text)">
                {terminal}
              </span>
            </li>
          )}
        </ol>
      </nav>
      {showBadge ? <ArchivesCountBadge count={count} /> : null}
    </div>
  );
}
