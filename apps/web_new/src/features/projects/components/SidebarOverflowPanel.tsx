// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Projects overflow slide-over (parity/specs/shell.md §4, SHELL-052).
// The panel behind the Projects group's overflow toggle: the joined
// collection searchable by name or short code, with per-row links,
// copy-link, pointer-drag reorder and a creation action for admins
// and members. A hopeless query shows the empty state; outside
// clicks, Escape and sidebar collapse close the panel.

import type { Project } from "@pidash/api-client";
import { Copy, Ellipsis, GripVertical, Plus, Search } from "lucide-react";
import * as React from "react";

import { isClientSideClick, useShellChrome } from "../../../shared/shell/index.js";

import { useUpdateProjectOrder } from "../api/projectOrder.js";
import { filterOverflowProjects } from "../overflow.js";
import { resolveDropTarget, resolveReorder, type SidebarDropRow, type SidebarDropTarget } from "../sidebar.js";
import { copyMessages } from "./SidebarProjectRow.js";

export interface SidebarOverflowPanelProps {
  workspaceSlug: string;
  /** Joined projects in sidebar order; the panel lists them all. */
  projects: Project[];
  /** Workspace admin/member standing; guests read false. */
  canCreateProject: boolean;
  onCreateProject: () => void;
  /** Client-side navigation for plain left clicks. */
  onNavigate: (href: string) => void;
  onClose: () => void;
}

export function SidebarOverflowPanel({
  workspaceSlug,
  projects,
  canCreateProject,
  onCreateProject,
  onNavigate,
  onClose,
}: SidebarOverflowPanelProps): React.ReactElement {
  const [query, setQuery] = React.useState("");
  const [sourceId, setSourceId] = React.useState<string | null>(null);
  const [target, setTarget] = React.useState<SidebarDropTarget | null>(null);
  const targetRef = React.useRef<SidebarDropTarget | null>(null);
  const panelRef = React.useRef<HTMLDivElement>(null);
  const listRef = React.useRef<HTMLUListElement>(null);
  const reorder = useUpdateProjectOrder(workspaceSlug);
  const sidebarCollapsed = useShellChrome((state) => state.sidebarCollapsed);
  const sidebarWidth = useShellChrome((state) => state.sidebarWidth);
  const filtered = filterOverflowProjects(projects, query);

  React.useEffect(() => {
    if (sidebarCollapsed) onClose();
  }, [sidebarCollapsed, onClose]);

  React.useEffect(() => {
    // Set: effects run after mount, so the panel is always present here.
    const panel = panelRef.current as HTMLDivElement;
    const outside = (event: PointerEvent): void => {
      const point = event.target as Element;
      if (panel.contains(point)) return;
      if (point.closest('[data-testid="projects-overflow-toggle"]') !== null) return;
      onClose();
    };
    const escape = (event: KeyboardEvent): void => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [onClose]);

  React.useEffect(() => {
    if (sourceId === null) return;
    const spans = (): SidebarDropRow[] => {
      const nodes = listRef.current?.querySelectorAll("[data-overflow-row]") ?? [];
      return Array.from(nodes).map((node) => {
        const rect = node.getBoundingClientRect();
        return { id: (node as HTMLElement).dataset["overflowRow"] ?? "", top: rect.top, bottom: rect.bottom };
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
      // The math runs on the full joined order, so rows hidden by the
      // search keep their relative positions.
      const write = resolveReorder(projects, sourceId, drop);
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
  }, [sourceId, projects, reorder]);

  function rowClick(event: React.MouseEvent<HTMLAnchorElement>, href: string): void {
    if (!isClientSideClick(event)) return;
    event.preventDefault();
    onNavigate(href);
  }

  async function copyLink(project: Project): Promise<void> {
    const { copyWorkItemLink } = await import("../../../shared/shell/shareLink.js");
    await copyWorkItemLink(`/${workspaceSlug}/projects/${project.id}/issues`, copyMessages);
  }

  return (
    <div
      ref={panelRef}
      role="dialog"
      aria-label="Extra projects"
      data-testid="projects-overflow-panel"
      style={{ left: sidebarCollapsed ? 0 : sidebarWidth }}
      className="fixed inset-y-0 z-40 flex w-72 flex-col gap-(--space-2) overflow-y-auto border-r border-(--border) bg-(--surface) p-(--space-4)"
    >
      <div className="flex w-full items-center justify-between gap-(--space-2)">
        <p className="text-emphasis flex items-center gap-(--space-2) text-(--text)">
          <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          <span className="truncate">All projects</span>
        </p>
        {canCreateProject ? (
          <button
            type="button"
            aria-label="Create new project"
            data-testid="projects-overflow-create"
            onClick={onCreateProject}
            className="flex cursor-pointer items-center gap-(--space-1) rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle)"
          >
            <Plus size={14} strokeWidth={2} aria-hidden="true" />
            <span className="text-caption">New</span>
          </button>
        ) : null}
      </div>
      <label className="flex w-full items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1)">
        <Search size={14} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          aria-label="Search projects"
          placeholder="Search projects"
          data-testid="projects-overflow-search"
          className="text-body min-w-0 flex-1 bg-transparent text-(--text) outline-none placeholder:text-(--text-muted)"
        />
      </label>
      {filtered.length === 0 ? (
        <div
          data-testid="projects-overflow-empty"
          className="flex flex-col items-center gap-(--space-2) px-(--space-2) py-(--space-6) text-center"
        >
          <Search size={20} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
          <p className="text-body text-(--text)">No matching projects</p>
          <p className="text-caption text-(--text-muted)">Try a different name or short code.</p>
        </div>
      ) : (
        <ul data-testid="projects-overflow-list" ref={listRef} className="flex flex-col gap-px">
          {filtered.map((project) => {
            const href = `/${workspaceSlug}/projects/${project.id}/issues`;
            const indicator = target !== null && target.id === project.id ? (target.after ? "below" : "above") : null;
            return (
              <li key={project.id} data-overflow-row={project.id} className="relative">
                {indicator === "above" ? (
                  <span
                    data-testid="overflow-drop-indicator"
                    className="absolute inset-x-2 top-0 h-0.5 bg-(--accent)"
                  />
                ) : null}
                <div
                  className={
                    sourceId === project.id
                      ? "group/row relative flex w-full items-center rounded-(--radius-control) opacity-60"
                      : "group/row relative flex w-full items-center rounded-(--radius-control)"
                  }
                >
                  <button
                    type="button"
                    aria-label={`Rearrange ${project.name}`}
                    title="Drag to reorder"
                    data-testid="overflow-drag-handle"
                    onPointerDown={(event) => {
                      event.preventDefault();
                      setSourceId(project.id);
                    }}
                    className="absolute top-1/2 -left-3 hidden -translate-y-1/2 cursor-grab items-center justify-center rounded-(--radius-control) p-px text-(--text-muted) group-hover/row:flex"
                  >
                    <GripVertical size={14} strokeWidth={2} aria-hidden="true" />
                  </button>
                  <a
                    href={href}
                    onClick={(event) => rowClick(event, href)}
                    className="text-body flex min-w-0 flex-1 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                  >
                    <span className="text-caption shrink-0 rounded-(--radius-control) border border-(--border) px-(--space-2) py-px font-[family-name:var(--font-mono)]">
                      {project.identifier}
                    </span>
                    <span className="truncate">{project.name}</span>
                  </a>
                  <button
                    type="button"
                    aria-label={`Copy link to ${project.name}`}
                    title="Copy link"
                    data-testid="overflow-row-copy"
                    onClick={() => void copyLink(project)}
                    className="shrink-0 cursor-pointer rounded-(--radius-control) p-(--space-1) text-(--text-muted) opacity-0 group-hover/row:opacity-100 hover:bg-(--subtle) focus-visible:opacity-100"
                  >
                    <Copy size={14} strokeWidth={2} aria-hidden="true" />
                  </button>
                </div>
                {indicator === "below" ? (
                  <span
                    data-testid="overflow-drop-indicator"
                    className="absolute inset-x-2 bottom-0 h-0.5 bg-(--accent)"
                  />
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
