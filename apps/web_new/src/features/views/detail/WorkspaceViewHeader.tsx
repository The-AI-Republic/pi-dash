// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace view detail header (parity/specs/views.md §11, VIEW-038).
// The trail is a "Views" link to the list and the view switcher; the
// right side carries the layout selector (cloud), the filter and Display
// controls, the Add-view action for members and admins, and the quick
// menu. The screen resolves which controls exist (locked views hide the
// first three); this header only orders them.

import type { SavedView } from "@pidash/api-client";
import { Button } from "@pidash/kit";
import { ChevronRight } from "lucide-react";
import * as React from "react";

import { WORKSPACE_VIEWS_CRUMB, workspaceViewsListHref, type WorkspaceDefaultView } from "../lib/workspaceViews.js";
import { WorkspaceViewMenu, type WorkspaceViewMenuStanding } from "../components/WorkspaceViewMenu.js";
import { WorkspaceViewSwitcher } from "./WorkspaceViewSwitcher.js";

export interface WorkspaceViewHeaderProps {
  workspaceSlug: string;
  /** Static default definition, or null for a custom. */
  defaultView: WorkspaceDefaultView | null;
  /** Custom record, or null for a default. */
  view: SavedView | null;
  /** Detail path the menu's open/copy entries act on. */
  detailHref: string;
  customs: readonly SavedView[];
  customsLoading: boolean;
  customsFailed: boolean;
  standing: WorkspaceViewMenuStanding;
  /** Cloud layout selector, or nothing on OSS and locked views. */
  layoutControl: React.ReactNode;
  /** Filter row toggle, or nothing on locked views. */
  filterControl: React.ReactNode;
  /** Display control, or nothing on locked views. */
  displayControl: React.ReactNode;
  /** Members and admins may add a view; guests read only. */
  canAddView: boolean;
  onOpenView: (viewId: string) => void;
  onAddView: () => void;
  /** Opens the edit dialog; null hides the entry. */
  onEdit: (() => void) | null;
  /** Opens the delete confirmation; null hides the entry. */
  onDelete: (() => void) | null;
}

export function WorkspaceViewHeader({
  workspaceSlug,
  defaultView,
  view,
  detailHref,
  customs,
  customsLoading,
  customsFailed,
  standing,
  layoutControl,
  filterControl,
  displayControl,
  canAddView,
  onOpenView,
  onAddView,
  onEdit,
  onDelete,
}: WorkspaceViewHeaderProps): React.ReactElement {
  const current =
    defaultView === null ? (
      <WorkspaceViewSwitcher
        currentKey={view?.id ?? ""}
        currentName={view?.name ?? ""}
        customs={customs}
        loading={customsLoading}
        failed={customsFailed}
        onPick={onOpenView}
      />
    ) : (
      <WorkspaceViewSwitcher
        currentKey={defaultView.key}
        currentName={defaultView.label}
        customs={customs}
        loading={customsLoading}
        failed={customsFailed}
        onPick={onOpenView}
      />
    );
  const currentName = defaultView === null ? (view?.name ?? "") : defaultView.label;
  return (
    <header
      data-testid="workspace-view-header"
      className="flex items-center justify-between gap-(--space-2) border-b border-(--border) px-(--space-3) py-(--space-2)"
    >
      <nav aria-label="Breadcrumb">
        <ol className="flex min-w-0 items-center gap-(--space-1)">
          <li>
            <a
              href={workspaceViewsListHref(workspaceSlug)}
              className="text-meta text-(--text-muted) hover:text-(--text)"
            >
              {WORKSPACE_VIEWS_CRUMB}
            </a>
          </li>
          <li aria-hidden="true">
            <ChevronRight size={12} strokeWidth={1.5} className="text-(--text-muted)" />
          </li>
          <li className="min-w-0">{current}</li>
        </ol>
      </nav>
      <div className="flex shrink-0 items-center gap-(--space-1)">
        {layoutControl}
        {filterControl}
        {displayControl}
        {canAddView ? (
          <Button variant="primary" size="small" onClick={() => onAddView()} data-testid="workspace-view-add">
            Add view
          </Button>
        ) : null}
        <WorkspaceViewMenu
          href={detailHref}
          standing={standing}
          onEdit={onEdit}
          onDelete={onDelete}
          triggerLabel={`Actions for ${currentName}`}
        />
      </div>
    </header>
  );
}
