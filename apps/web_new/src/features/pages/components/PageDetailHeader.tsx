// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail breadcrumb and action strip (parity/specs/pages.md §4.3, §6.5,
// §7, PAGE-024/045/055/056). Project context, the always-present path back
// to the collection, the searchable sibling switcher, the sync indicator
// and the action cluster: the navigation-pane toggle, favourite, link copy,
// and the options menu whose history entry writes the info pane. Later
// header slices add the remaining menu entries at the same mount.

import { IconButton, Menu } from "@pidash/kit";
import { Link, useNavigate } from "@tanstack/react-router";
import { Ellipsis, Link2, PanelRight } from "lucide-react";
import * as React from "react";

import { ProjectMark } from "../../../shared/pickers/ProjectMark.js";
import type { PageRow as PageRowData } from "@pidash/api-client";
import { pageTabHref } from "../lib/pageList.js";
import type { PageSiblingOption } from "../lib/pageDetail.js";
import { PageFavoriteStar } from "./PageFavoriteStar.js";
import { PageSiblingSwitcher } from "./PageSiblingSwitcher.js";
import { PageSyncBadge } from "./PageSyncBadge.js";
import type { PageSyncStatus } from "../lib/pageSync.js";

export const PAGE_CRUMB = "Pages";
export const PAGE_COPY_LINK_LABEL = "Copy link";
export const PAGE_COPIED_LINK_LABEL = "Copied link";
export const PAGE_OPTIONS_LABEL = "Page options";
export const PAGE_HISTORY_ITEM = "Version history";
export const PAGE_OPEN_PANE_LABEL = "Open navigation pane";

/** How long the copy control reports its success before reverting. */
export const PAGE_COPY_CONFIRM_MS = 1600;

export interface PageDetailHeaderProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  projectLogo: Record<string, unknown>;
  projectIdentifier: string;
  page: Pick<PageRowData, "id" | "name" | "is_favorite">;
  siblings: readonly PageSiblingOption[];
  syncStatus: PageSyncStatus;
  showStar: boolean;
  onSelectSibling: (pageId: string) => void;
  onCopyLink: () => void;
  onOpenHistory: () => void;
  /** Reveal the navigation pane (PAGE-045). */
  onOpenPane: () => void;
  /** Whether the navigation pane is currently open (PAGE-045). */
  paneOpen: boolean;
}

export function PageDetailHeader({
  workspaceSlug,
  projectId,
  projectName,
  projectLogo,
  projectIdentifier,
  page,
  siblings,
  syncStatus,
  showStar,
  onSelectSibling,
  onCopyLink,
  onOpenHistory,
  onOpenPane,
  paneOpen,
}: PageDetailHeaderProps): React.ReactElement {
  const navigate = useNavigate();
  const [copied, setCopied] = React.useState(false);
  const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  React.useEffect(
    () => () => {
      if (timerRef.current !== null) clearTimeout(timerRef.current);
    },
    []
  );

  function handleCopy(): void {
    onCopyLink();
    setCopied(true);
    if (timerRef.current !== null) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => setCopied(false), PAGE_COPY_CONFIRM_MS);
  }

  return (
    <div
      data-testid="page-detail-header"
      className="flex h-11 shrink-0 items-center gap-(--space-2) border-b border-(--border) px-(--space-4)"
    >
      <button
        type="button"
        onClick={() => {
          void navigate({ to: "/$ws/projects/$projectId", params: { ws: workspaceSlug, projectId } });
        }}
        className="text-body flex max-w-56 min-w-0 cursor-pointer items-center gap-(--space-1) rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
      >
        <ProjectMark logoProps={projectLogo} identifier={projectIdentifier} size={18} />
        <span className="truncate">{projectName}</span>
      </button>
      <span aria-hidden="true" className="text-(--text-muted)">
        /
      </span>
      <Link
        to={pageTabHref(workspaceSlug, projectId, "public")}
        className="text-body rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
      >
        {PAGE_CRUMB}
      </Link>
      <span aria-hidden="true" className="text-(--text-muted)">
        /
      </span>
      <PageSiblingSwitcher
        currentName={page.name.trim() === "" ? "Untitled" : page.name}
        options={siblings}
        onSelect={onSelectSibling}
      />
      <span className="flex-1" aria-hidden="true" />
      <PageSyncBadge status={syncStatus} />
      <IconButton
        label={PAGE_OPEN_PANE_LABEL}
        aria-expanded={paneOpen}
        data-testid="page-pane-toggle"
        size="small"
        onClick={onOpenPane}
      >
        <PanelRight size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
      {showStar ? <PageFavoriteStar workspaceSlug={workspaceSlug} projectId={projectId} page={page} /> : null}
      <IconButton label={copied ? PAGE_COPIED_LINK_LABEL : PAGE_COPY_LINK_LABEL} size="small" onClick={handleCopy}>
        <Link2 size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
      <Menu
        trigger={
          <IconButton label={PAGE_OPTIONS_LABEL} size="small">
            <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={[{ label: PAGE_HISTORY_ITEM, onSelect: onOpenHistory }]}
      />
    </div>
  );
}
