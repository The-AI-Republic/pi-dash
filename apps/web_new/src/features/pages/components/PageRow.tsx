// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One page row (parity/specs/pages.md §3.5 and §3.8, PAGE-006). The row
// exposes the document mark, title (untitled fallback), owner, visibility
// and creation date, plus the star marker and overflow trigger. Hovering
// the metadata reveals each detail; the row itself opens the detail view
// and its context menu mirrors the overflow menu.

import { Avatar, ContextMenu, IconButton, Menu, Tooltip } from "@pidash/kit";
import type { PageRow as PageRowData } from "@pidash/api-client";
import { CalendarDays, Ellipsis, FileText, Globe2, Lock, Star } from "lucide-react";
import * as React from "react";
import { Link } from "@tanstack/react-router";

import type { PageCreatorOption } from "../lib/pageList.js";
import { pageDetailHref, pageDisplayTitle } from "../lib/pageList.js";

export interface PageRowMenuStrings {
  openInNewTab: string;
  copyLink: string;
}

export interface PageRowProps {
  workspaceSlug: string;
  projectId: string;
  page: PageRowData;
  owner: PageCreatorOption | null;
  /** Members and admins only; guests never see a star. */
  showStar: boolean;
  /** Viewer-safe actions until the row-action slices own the full menu. */
  menu: PageRowMenuStrings;
  onOpenInNewTab: () => void;
  onCopyLink: () => void;
}

function formatCreatedOn(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Created on an unknown date";
  return `Created on ${date.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" })}`;
}

function PageMark({ page }: { page: PageRowData }): React.ReactElement {
  const emoji = page.logo_props.emoji?.value;
  const icon = page.logo_props.icon;
  if (page.logo_props.in_use === "emoji" && emoji !== undefined && emoji !== "") {
    return (
      <span aria-hidden="true" className="text-body inline-flex size-6 shrink-0 items-center justify-center">
        {emoji}
      </span>
    );
  }
  if (page.logo_props.in_use === "icon" && icon !== undefined) {
    return (
      <span
        aria-hidden="true"
        className="text-body inline-flex size-6 shrink-0 items-center justify-center rounded-(--radius-control)"
        style={icon.color === undefined ? undefined : { color: icon.color }}
      >
        {icon.name.slice(0, 1).toUpperCase()}
      </span>
    );
  }
  return <FileText size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />;
}

function VisibilityGlyph({ access }: { access: number }): React.ReactElement {
  const label = access === 1 ? "Private" : "Public";
  return (
    <Tooltip content={label}>
      <span
        data-testid="page-row-visibility"
        className="inline-flex cursor-default items-center text-(--text-muted)"
        aria-label={label}
      >
        {access === 1 ? (
          <Lock size={14} strokeWidth={1.5} aria-hidden="true" />
        ) : (
          <Globe2 size={14} strokeWidth={1.5} aria-hidden="true" />
        )}
      </span>
    </Tooltip>
  );
}

export function PageRow({
  workspaceSlug,
  projectId,
  page,
  owner,
  showStar,
  menu,
  onOpenInNewTab,
  onCopyLink,
}: PageRowProps): React.ReactElement {
  const title = pageDisplayTitle(page.name);
  const ownerName = owner?.name ?? "Unknown member";
  const entries = [
    { label: menu.openInNewTab, onSelect: onOpenInNewTab },
    { label: menu.copyLink, onSelect: onCopyLink },
  ];
  const row = (
    <div
      data-testid="page-row"
      data-page-id={page.id}
      className="group flex items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-3) hover:bg-(--subtle)"
    >
      <PageMark page={page} />
      <Link
        to={pageDetailHref(workspaceSlug, projectId, page.id)}
        className="text-body min-w-0 flex-1 truncate font-medium text-(--text) hover:underline"
      >
        {title}
      </Link>
      <Tooltip content={ownerName}>
        <span className="inline-flex">
          <Avatar
            name={ownerName}
            size="small"
            {...(owner?.avatarUrl === null || owner?.avatarUrl === undefined ? {} : { src: owner.avatarUrl })}
          />
        </span>
      </Tooltip>
      <VisibilityGlyph access={page.access} />
      <Tooltip content={formatCreatedOn(page.created_at)}>
        <span
          className="inline-flex cursor-default items-center text-(--text-muted)"
          aria-label={formatCreatedOn(page.created_at)}
        >
          <CalendarDays size={14} strokeWidth={1.5} aria-hidden="true" />
        </span>
      </Tooltip>
      {showStar ? (
        <span
          data-testid="page-row-star"
          role="img"
          aria-label={page.is_favorite ? "Marked as favorite" : "Not marked as favorite"}
          className="inline-flex shrink-0"
        >
          <Star
            size={16}
            strokeWidth={1.5}
            aria-hidden="true"
            className={page.is_favorite ? "fill-(--accent) text-(--accent)" : "text-(--text-muted)"}
          />
        </span>
      ) : null}
      <Menu
        trigger={
          <IconButton label={`Page actions for ${title}`} size="small" className="shrink-0">
            <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={entries}
      />
    </div>
  );
  return <ContextMenu trigger={row} items={entries} />;
}
