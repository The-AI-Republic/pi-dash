// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar projects group (parity/specs/shell.md §4, SHELL-048/049/051/052).
// The collapsing disclosure bundling the workspace aggregate row with
// the joined projects: placeholder rows while the collection loads, a
// direct create action for empty workspaces, collapse memory across
// client-side navigation with a fresh open on every load, auto-open on
// project, drafts and views routes, pointer-drag reordering with
// server persistence, and the overflow toggle opening the searchable
// slide-over once the cap leaves rows out. Drafts ride the personal
// entries above.

import { Skeleton } from "@pidash/kit";
import type { Project } from "@pidash/api-client";
import { ChevronRight, Ellipsis, List, Plus } from "lucide-react";
import * as React from "react";

import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { useUpdateProjectOrder } from "../api/projectOrder.js";
import { useProjectListCap, useWorkspaceProperties } from "../api/tabPreferences.js";
import { hasOverflow } from "../overflow.js";
import { isTabbedMode } from "../tabs.js";
import {
  isAggregateRowActive,
  isProjectsGroupRoute,
  resolveDropTarget,
  resolveReorder,
  selectSidebarProjects,
  sliceVisibleProjects,
  type SidebarDropRow,
  type SidebarDropTarget,
} from "../sidebar.js";
// The slide-over only mounts after the user opens it, and its search box,
// drag math and row menu are heavy relative to the sidebar; it rides its
// own chunk so first paint never carries it (Quality gates budget).
const LazySidebarOverflowPanel = React.lazy(() =>
  import("./SidebarOverflowPanel.js").then((module) => ({ default: module.SidebarOverflowPanel }))
);
import { SidebarProjectRow } from "./SidebarProjectRow.js";

export interface SidebarProjectsGroupProps {
  workspaceSlug: string;
  /** Full project collection; empty while pending or when none exist. */
  projects: Project[];
  projectsStatus: "pending" | "error" | "success";
  /** Current pathname; null while the router has not reported it. */
  currentPath: string | null;
  /** Project id of the route in view, if any. */
  activeProjectId: string | null;
  /** Workspace admin/member standing; guests read false. */
  canCreateProject: boolean;
  onCreateProject: () => void;
  /** Client-side navigation for plain left clicks. */
  onNavigate: (href: string) => void;
}

const PLACEHOLDER_ROWS = 4;

export function SidebarProjectsGroup({
  workspaceSlug,
  projects,
  projectsStatus,
  currentPath,
  activeProjectId,
  canCreateProject,
  onCreateProject,
  onNavigate,
}: SidebarProjectsGroupProps): React.ReactElement {
  const cap = useProjectListCap(workspaceSlug);
  const workspaceProperties = useWorkspaceProperties(workspaceSlug);
  const tabbed = isTabbedMode(workspaceProperties.data?.navigation_control_preference ?? "ACCORDION");
  const reorder = useUpdateProjectOrder(workspaceSlug);
  const joined = React.useMemo(() => selectSidebarProjects(projects), [projects]);
  const visible = sliceVisibleProjects(joined, cap);
  const [closed, setClosed] = React.useState(false);
  const [overflowOpen, setOverflowOpen] = React.useState(false);
  const [sourceId, setSourceId] = React.useState<string | null>(null);
  const [target, setTarget] = React.useState<SidebarDropTarget | null>(null);
  const targetRef = React.useRef<SidebarDropTarget | null>(null);
  const listRef = React.useRef<HTMLUListElement>(null);
  const aggregateActive = currentPath !== null && isAggregateRowActive(currentPath, workspaceSlug);

  React.useEffect(() => {
    if (currentPath !== null && isProjectsGroupRoute(currentPath)) setClosed(false);
  }, [currentPath]);

  React.useEffect(() => {
    if (sourceId === null) return;
    const spans = (): SidebarDropRow[] => {
      const nodes = listRef.current?.querySelectorAll("[data-project-row]") ?? [];
      return Array.from(nodes).map((node) => {
        const rect = node.getBoundingClientRect();
        return { id: (node as HTMLElement).dataset["projectRow"] ?? "", top: rect.top, bottom: rect.bottom };
      });
    };
    const move = (event: PointerEvent): void => {
      const next = resolveDropTarget(spans(), event.clientY, sourceId);
      targetRef.current = next;
      setTarget(next);
    };
    const finish = (commit: boolean): void => {
      const drop = targetRef.current;
      targetRef.current = null;
      setTarget(null);
      setSourceId(null);
      if (!commit || drop === null) return;
      const write = resolveReorder(joined, sourceId, drop);
      if (write !== null) reorder.mutate(write);
    };
    const up = (): void => finish(true);
    const cancel = (): void => finish(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("blur", cancel);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("blur", cancel);
    };
  }, [sourceId, joined, reorder]);

  return (
    <section aria-label="Projects" data-testid="sidebar-projects-group" className="flex flex-col gap-(--space-1)">
      <div className="group flex w-full items-center justify-between rounded-(--radius-control) px-(--space-2) py-(--space-1)">
        <button
          type="button"
          aria-expanded={!closed}
          aria-label={closed ? "Expand projects group" : "Collapse projects group"}
          data-testid="projects-group-toggle"
          onClick={() => setClosed(!closed)}
          className="text-caption flex flex-1 cursor-pointer items-center gap-(--space-1) text-left text-(--text-muted)"
        >
          <span className="font-semibold">Projects</span>
          <ChevronRight
            size={14}
            strokeWidth={2}
            aria-hidden="true"
            className={closed ? "transition-transform" : "rotate-90 transition-transform"}
          />
        </button>
        {canCreateProject ? (
          <button
            type="button"
            aria-label="Create new project"
            data-testid="sidebar-create-project"
            onClick={onCreateProject}
            className="hidden cursor-pointer rounded-(--radius-control) p-(--space-1) text-(--text-muted) group-hover:flex hover:bg-(--subtle)"
          >
            <Plus size={14} strokeWidth={2} aria-hidden="true" />
          </button>
        ) : null}
      </div>
      {closed || projectsStatus === "pending" ? null : (
        <ul data-testid="sidebar-projects-list" ref={listRef} className="flex flex-col gap-px">
          <li>
            <CrossAppLink
              to={`/${workspaceSlug}/workspace-views/all-issues/`}
              aria-current={aggregateActive ? "page" : undefined}
              className={
                aggregateActive
                  ? "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-2) text-left text-(--text)"
                  : "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
              }
            >
              <List size={16} strokeWidth={1.5} aria-hidden="true" />
              <span className="truncate">Work Items</span>
            </CrossAppLink>
          </li>
          {visible.map((project) => (
            <SidebarProjectRow
              key={project.id}
              workspaceSlug={workspaceSlug}
              project={project}
              currentPath={currentPath}
              activeProjectId={activeProjectId}
              tabbed={tabbed}
              onNavigate={onNavigate}
              dropIndicator={target !== null && target.id === project.id ? (target.after ? "below" : "above") : null}
              isDragging={sourceId === project.id}
              onDragStart={setSourceId}
            />
          ))}
        </ul>
      )}
      {closed || projectsStatus !== "success" || !hasOverflow(joined.length, cap) ? null : (
        <button
          type="button"
          aria-expanded={overflowOpen}
          aria-label={overflowOpen ? "Close extra projects" : "Open extra projects"}
          data-testid="projects-overflow-toggle"
          onClick={() => setOverflowOpen(!overflowOpen)}
          className="text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          <span className="truncate">{overflowOpen ? "Hide" : "More"}</span>
        </button>
      )}
      {overflowOpen ? (
        <React.Suspense fallback={null}>
          <LazySidebarOverflowPanel
            workspaceSlug={workspaceSlug}
            projects={joined}
            canCreateProject={canCreateProject}
            onCreateProject={onCreateProject}
            onNavigate={onNavigate}
            onClose={() => setOverflowOpen(false)}
          />
        </React.Suspense>
      ) : null}
      {closed || projectsStatus !== "pending" ? null : (
        <div role="status" aria-label="Loading projects" data-testid="sidebar-project-placeholders">
          {Array.from({ length: PLACEHOLDER_ROWS }, (_, index) => (
            <Skeleton key={`placeholder-${index}`} height="28px" />
          ))}
        </div>
      )}
      {closed || projectsStatus === "pending" || joined.length > 0 || !canCreateProject ? null : (
        <button
          type="button"
          data-testid="sidebar-empty-create-project"
          onClick={onCreateProject}
          className="text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <Plus size={16} strokeWidth={1.5} aria-hidden="true" />
          <span className="truncate">Create new project</span>
        </button>
      )}
    </section>
  );
}
