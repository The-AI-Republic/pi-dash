// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One stored workspace view on the list (parity/specs/views.md §9,
// VIEW-032). The whole identity cluster is an anchor to the view's detail;
// the name is capped so a very long title never pushes the menu off the
// row, and the description sits underneath only when the record has one.
// The overflow menu is a sibling of the anchor: a button inside a link is
// not reachable by keyboard on its own.

import type { SavedView } from "@pidash/api-client";
import { Link } from "@tanstack/react-router";
import { Layers } from "lucide-react";
import * as React from "react";

import { capWorkspaceViewName, workspaceCustomViewHref } from "../lib/workspaceViews.js";
import { WorkspaceViewRowMenu } from "./WorkspaceViewRowMenu.js";

export interface WorkspaceViewRowProps {
  workspaceSlug: string;
  view: SavedView;
  /** Opens the edit dialog; absent until the CRUD slice supplies it. */
  onEdit?: (() => void) | undefined;
  /** Opens the delete confirmation; absent until the CRUD slice supplies it. */
  onDelete?: (() => void) | undefined;
}

export function WorkspaceViewRow({ workspaceSlug, view, onEdit, onDelete }: WorkspaceViewRowProps): React.ReactElement {
  const description = view.description.trim();
  return (
    <li
      data-testid="workspace-views-custom-row"
      data-view-id={view.id}
      className="flex items-center gap-(--space-2) border-b border-(--border) px-(--space-4)"
    >
      <Link
        to={workspaceCustomViewHref(workspaceSlug, view.id)}
        className="flex min-w-0 flex-1 flex-col gap-(--space-1) py-(--space-2)"
      >
        <span className="flex items-center gap-(--space-3)">
          <Layers size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          <span className="text-body truncate font-medium text-(--text)">{capWorkspaceViewName(view.name)}</span>
        </span>
        {description === "" ? null : <span className="text-meta truncate text-(--text-muted)">{description}</span>}
      </Link>
      <WorkspaceViewRowMenu viewName={view.name} onEdit={() => onEdit?.()} onDelete={() => onDelete?.()} />
    </li>
  );
}
