// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view row identity (parity/specs/views.md §3, VIEW-011): logo,
// name, deep link, access badge, owner, Live marker. The identity cluster
// is one link to the detail page; the row actions (star, overflow menu)
// mount beside it, never nested inside the link (NEWFRONT-722).

import { Link } from "@tanstack/react-router";
import { Avatar, Badge, Tooltip } from "@pidash/kit";
import type { SavedView } from "@pidash/api-client";
import { Globe, Layers, Lock } from "lucide-react";
import * as React from "react";

import { ProjectMarkIcon } from "../../../shared/pickers/ProjectMarkIcon.js";
import { projectMark } from "../../../shared/pickers/projectIcon.js";
import { isSharedView, isViewLive, projectViewHref, viewAccessLabel, type ViewOwner } from "../lib/projectViews.js";

export interface ProjectViewRowProps {
  workspaceSlug: string;
  projectId: string;
  view: SavedView;
  /** Null when the roster is missing or the owner left: no avatar then. */
  owner: ViewOwner | null;
  /** Star and overflow menu. Slot owner: NEWFRONT-722. */
  trailingControl?: React.ReactNode;
  /**
   * Opens the publish dialog from the Live marker (VIEW-020, cloud
   * only). Absent on OSS: the marker then stays a plain badge.
   */
  onLiveMarkerClick?: (() => void) | undefined;
}

function ViewLogo({ view }: { view: SavedView }): React.ReactElement {
  const mark = projectMark(view.logo_props);
  if (mark.kind === "emoji") {
    return (
      <span aria-hidden="true" className="grid size-4 shrink-0 place-items-center text-[16px] leading-none">
        {mark.glyph}
      </span>
    );
  }
  if (mark.kind === "icon") {
    return <ProjectMarkIcon mark={mark} identifier={view.name} size={16} />;
  }
  return <Layers size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />;
}

function AccessBadge({ access }: { access: number }): React.ReactElement {
  const shared = isSharedView(access);
  const Glyph = shared ? Globe : Lock;
  return (
    <Tooltip content={shared ? "Shared with the project" : "Visible only to its owner"}>
      <span data-testid="project-views-row-access" className="inline-flex shrink-0">
        <Badge tone="neutral">
          <Glyph size={12} strokeWidth={1.5} aria-hidden="true" />
          {viewAccessLabel(access)}
        </Badge>
      </span>
    </Tooltip>
  );
}

export function ProjectViewRow({
  workspaceSlug,
  projectId,
  view,
  owner,
  trailingControl,
  onLiveMarkerClick,
}: ProjectViewRowProps): React.ReactElement {
  return (
    <li data-testid="project-views-row" data-view-id={view.id} className="border-b border-(--border)">
      <div className="flex items-center gap-(--space-2) px-(--space-4)">
        <Link
          to={projectViewHref(workspaceSlug, projectId, view.id)}
          className="flex h-11 min-w-0 flex-1 items-center gap-(--space-3)"
        >
          <ViewLogo view={view} />
          <span
            data-testid="project-views-row-name"
            className="text-body min-w-0 flex-1 truncate font-medium text-(--text)"
          >
            {view.name}
          </span>
          <AccessBadge access={view.access} />
          {isViewLive(view.anchor) ? (
            onLiveMarkerClick === undefined ? (
              <span data-testid="project-views-row-live" className="inline-flex shrink-0">
                <Badge tone="accent">Live</Badge>
              </span>
            ) : (
              <button
                type="button"
                data-testid="project-views-row-live"
                aria-label={`Public link for ${view.name}`}
                onClick={(event) => {
                  // The marker sits inside the row link; the click opens
                  // the publish dialog instead of navigating.
                  event.preventDefault();
                  event.stopPropagation();
                  onLiveMarkerClick();
                }}
                className="inline-flex shrink-0 cursor-pointer"
              >
                <Badge tone="accent">Live</Badge>
              </button>
            )
          ) : null}
          {owner === null ? null : (
            <Tooltip content={owner.name}>
              <span data-testid="project-views-row-owner" className="inline-flex shrink-0">
                <Avatar
                  name={owner.name}
                  {...(owner.avatarUrl === null ? {} : { src: owner.avatarUrl })}
                  size="small"
                />
              </span>
            </Tooltip>
          )}
        </Link>
        {trailingControl}
      </div>
    </li>
  );
}
