// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail root layout (parity/specs/issues.md §13, ISS-144). The main
// column plus the collapsible properties sidebar. The collapse flag is
// global (detail/store): wide viewports honor it with a collapse toggle
// and an expand rail, while narrow viewports (< 768px) always render
// the sidebar content inline under the main column. Sidebar rows arrive
// through the sidebar slot. The parent banner (ISS-158) sits directly
// above the title when the item has a parent.

import { IconButton } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import { PanelRightClose, PanelRightOpen } from "lucide-react";
import * as React from "react";

import { useNarrowViewport } from "../../../shared/shell/narrowViewport.js";
import { DetailMainColumn } from "./DetailMainColumn.js";
import { ParentBanner } from "./ParentBanner.js";
import { useDetailUiStore } from "./store.js";

export interface IssueDetailRootProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  /** Project member/admin standing on a live project; guests read. Archived and synced rows still render read-only. */
  editable: boolean;
  /** Properties sidebar content (rows from later slices). */
  sidebar?: React.ReactNode;
  /** Header row, left of the save indicator (identifier/type). */
  topSlot?: React.ReactNode;
  /** Agent controls on surfaces without the widget action row (notification peek). */
  agentActions?: React.ReactNode;
  /** Block under the description-meta row (the activity feed). */
  activity?: React.ReactNode;
  /** Render the parent banner above the title (default true). */
  showParentBanner?: boolean;
}

export function IssueDetailRoot({
  workspaceSlug,
  projectId,
  issue,
  editable,
  sidebar,
  topSlot,
  agentActions,
  activity,
  showParentBanner = true,
}: IssueDetailRootProps): React.ReactElement {
  const collapsed = useDetailUiStore((state) => state.sidebarCollapsed);
  const setCollapsed = useDetailUiStore((state) => state.setSidebarCollapsed);
  const narrow = useNarrowViewport();
  // Responsive automation only: narrowing collapses, and widening back
  // out restores. A collapse the user chose on a wide viewport stays.
  const wasNarrowRef = React.useRef(narrow);
  React.useEffect(() => {
    const wasNarrow = wasNarrowRef.current;
    wasNarrowRef.current = narrow;
    if (narrow) {
      if (!collapsed) setCollapsed(true);
    } else if (wasNarrow && collapsed) {
      setCollapsed(false);
    }
  }, [narrow, collapsed, setCollapsed]);

  return (
    <div data-testid="issue-detail-root" className="flex flex-col gap-(--space-8) p-(--space-8) md:flex-row">
      <div className="flex min-w-0 flex-1 flex-col gap-(--space-4)">
        {showParentBanner ? (
          <ParentBanner workspaceSlug={workspaceSlug} projectId={projectId} issue={issue} editable={editable} />
        ) : null}
        <DetailMainColumn
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          issue={issue}
          editable={editable}
          topSlot={topSlot}
          agentActions={agentActions}
          activity={activity}
        />
      </div>
      {sidebar === undefined ? null : (
        <>
          <aside
            aria-label="Properties"
            className={
              collapsed
                ? "flex min-w-0 flex-col gap-(--space-4) md:hidden"
                : "flex min-w-0 flex-col gap-(--space-4) md:w-80 md:shrink-0"
            }
          >
            <div className="flex items-center justify-between gap-(--space-2)">
              <p className="text-meta font-medium text-(--text-muted)">Properties</p>
              <span className="hidden md:block">
                <IconButton label="Collapse properties" variant="ghost" size="small" onClick={() => setCollapsed(true)}>
                  <PanelRightClose size={16} strokeWidth={1.5} />
                </IconButton>
              </span>
            </div>
            {sidebar}
          </aside>
          {collapsed ? (
            <div className="hidden shrink-0 md:block">
              <IconButton label="Expand properties" variant="ghost" size="small" onClick={() => setCollapsed(false)}>
                <PanelRightOpen size={16} strokeWidth={1.5} />
              </IconButton>
            </div>
          ) : null}
        </>
      )}
    </div>
  );
}
