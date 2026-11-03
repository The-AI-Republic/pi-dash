// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace views tab strip (parity/specs/views.md §10, VIEW-033). One tab
// per static default, then one per custom view in list order; the active
// tab is marked and scrolled into view on mount and on change. Each tab
// carries its quick menu beside its link (defaults offer open/copy only;
// customs follow the ownership rules). A trailing add button, rendered
// for workspace members and admins only, opens the create dialog; the
// strip also hosts the edit and delete dialogs its tab menus open. The
// strip reads the same custom list as the views list screen.

import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import type { SavedView } from "@pidash/api-client";
import { IconButton } from "@pidash/kit";
import { Plus } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useMe, usePermissions } from "../../../core/session/index.js";
import { workspaceViewsQueryOptions } from "../api/queries.js";
import { DeleteWorkspaceViewDialog } from "../dialog/DeleteWorkspaceViewDialog.js";
import { WorkspaceViewDialog } from "../dialog/WorkspaceViewDialog.js";
import {
  capWorkspaceViewName,
  sortWorkspaceViewsNewestFirst,
  WORKSPACE_DEFAULT_VIEWS,
  workspaceCustomViewHref,
  workspaceDefaultViewHref,
  type WorkspaceDefaultView,
} from "../lib/workspaceViews.js";
import { resolveTabScroll, tabScrollBehavior } from "../lib/workspaceTabStrip.js";
import { WorkspaceViewMenu } from "./WorkspaceViewMenu.js";

export interface WorkspaceViewsTabStripProps {
  workspaceSlug: string;
  /** Route key of a default or id of a custom; null while unresolved. */
  activeViewId: string | null;
}

interface TabShellProps {
  href: string;
  label: string;
  active: boolean;
  tabRef: ((element: HTMLDivElement | null) => void) | null;
  menu: React.ReactNode;
  tabKey: string;
}

function TabShell({ href, label, active, tabRef, menu, tabKey }: TabShellProps): React.ReactElement {
  return (
    <div ref={tabRef} data-testid="workspace-views-tab" data-tab-key={tabKey} className="flex shrink-0 items-center">
      <Link
        to={href}
        aria-current={active ? "page" : undefined}
        title={label}
        className={`text-body flex h-10 max-w-56 items-center truncate border-b-2 px-(--space-3) font-medium whitespace-nowrap ${
          active ? "border-(--accent) text-(--text)" : "border-transparent text-(--text-muted) hover:text-(--text)"
        }`}
      >
        {capWorkspaceViewName(label)}
      </Link>
      {menu}
    </div>
  );
}

export function WorkspaceViewsTabStrip({
  workspaceSlug,
  activeViewId,
}: WorkspaceViewsTabStripProps): React.ReactElement {
  const customs = useQuery(workspaceViewsQueryOptions(getAppClient(), workspaceSlug));
  const me = useMe();
  const permissions = usePermissions(workspaceSlug);
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<SavedView | null>(null);
  const [deleting, setDeleting] = React.useState<SavedView | null>(null);
  const stripRef = React.useRef<HTMLElement | null>(null);
  const activeRef = React.useRef<HTMLDivElement | null>(null);

  const views = React.useMemo(() => sortWorkspaceViewsNewestFirst(customs.data ?? []), [customs.data]);
  const meId = me.data?.id ?? null;

  // The active tab lands in view on mount and whenever it (or the loaded
  // customs) changes. The effect runs after mount, so the strip ref is set.
  React.useEffect(() => {
    const tab = activeRef.current;
    if (tab === null) return;
    const strip = stripRef.current!;
    const plan = resolveTabScroll(tab.getBoundingClientRect(), strip.getBoundingClientRect());
    if (!plan.scroll) return;
    const media =
      typeof window.matchMedia === "function" ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
    tab.scrollIntoView?.({ block: "nearest", inline: plan.inline, behavior: tabScrollBehavior(media) });
  }, [activeViewId, customs.data]);

  function customMenu(view: SavedView): React.ReactNode {
    const isOwner = meId !== null && view.owned_by === meId;
    return (
      <WorkspaceViewMenu
        href={workspaceCustomViewHref(workspaceSlug, view.id)}
        standing={{ isOwner, canDelete: isOwner || permissions.isAdmin }}
        onEdit={() => setEditing(view)}
        onDelete={() => setDeleting(view)}
        triggerLabel={`Actions for ${view.name}`}
      />
    );
  }

  function defaultMenu(view: WorkspaceDefaultView): React.ReactNode {
    return (
      <WorkspaceViewMenu
        href={workspaceDefaultViewHref(workspaceSlug, view.key)}
        standing={{ isOwner: false, canDelete: false }}
        onEdit={null}
        onDelete={null}
        triggerLabel={`Actions for ${view.label}`}
      />
    );
  }

  return (
    <>
      <nav
        ref={stripRef}
        aria-label="Workspace views"
        data-testid="workspace-views-tab-strip"
        className="flex shrink-0 items-center gap-(--space-1) overflow-x-auto border-b border-(--border) px-(--space-4)"
      >
        {WORKSPACE_DEFAULT_VIEWS.map((view) => (
          <TabShell
            key={view.key}
            tabKey={view.key}
            href={workspaceDefaultViewHref(workspaceSlug, view.key)}
            label={view.label}
            active={view.key === activeViewId}
            tabRef={
              view.key === activeViewId
                ? (element) => {
                    activeRef.current = element;
                  }
                : null
            }
            menu={defaultMenu(view)}
          />
        ))}
        {views.map((view) => (
          <TabShell
            key={view.id}
            tabKey={view.id}
            href={workspaceCustomViewHref(workspaceSlug, view.id)}
            label={view.name}
            active={view.id === activeViewId}
            tabRef={
              view.id === activeViewId
                ? (element) => {
                    activeRef.current = element;
                  }
                : null
            }
            menu={customMenu(view)}
          />
        ))}
        {permissions.canEdit && (
          <IconButton
            label="Add view"
            data-testid="workspace-views-tab-add"
            onClick={() => {
              setCreating(true);
            }}
          >
            <Plus size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        )}
      </nav>
      <WorkspaceViewDialog open={creating} onOpenChange={setCreating} workspaceSlug={workspaceSlug} mode="create" />
      {editing !== null && (
        <WorkspaceViewDialog
          open={true}
          onOpenChange={(next) => {
            if (!next) setEditing(null);
          }}
          workspaceSlug={workspaceSlug}
          mode="edit"
          view={editing}
        />
      )}
      {deleting !== null && (
        <DeleteWorkspaceViewDialog
          open={true}
          onOpenChange={(next) => {
            if (!next) setDeleting(null);
          }}
          workspaceSlug={workspaceSlug}
          view={deleting}
        />
      )}
    </>
  );
}
