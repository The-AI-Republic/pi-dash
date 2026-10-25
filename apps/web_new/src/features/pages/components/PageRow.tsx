// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One page row (parity/specs/pages.md §3.5 and §3.8, PAGE-006/009–012). The
// row exposes the document mark, title (untitled fallback), owner,
// visibility and creation date, plus a working favourite star and the
// overflow trigger. Hovering the metadata reveals each detail; the row
// itself opens the detail view and its context menu mirrors the overflow
// menu, which comes from the one action builder so both stay in step.

import { Avatar, ContextMenu, IconButton, Menu, Tooltip } from "@pidash/kit";
import type { PageRow as PageRowData } from "@pidash/api-client";
import { CalendarDays, Ellipsis, FileText, Globe2, Lock } from "lucide-react";
import * as React from "react";
import { Link } from "@tanstack/react-router";

import type { PageCreatorOption } from "../lib/pageList.js";
import { pageDetailHref, pageDisplayTitle } from "../lib/pageList.js";
import { pageRowActions } from "../lib/pageActions.js";
import { PageFavoriteStar } from "./PageFavoriteStar.js";
import { projectMark } from "../../../shared/pickers/projectIcon.js";
import { ProjectMarkIcon } from "../../../shared/pickers/ProjectMarkIcon.js";

export interface PageRowProps {
  workspaceSlug: string;
  projectId: string;
  page: PageRowData;
  owner: PageCreatorOption | null;
  /** Members and admins only; guests never see a star. */
  showStar: boolean;
  /** Duplication is a member-and-admin entry; other viewers never see it. */
  canDuplicate: boolean;
  onOpenInNewTab: () => void;
  onCopyLink: () => void;
  onDuplicate: () => void;
}

function formatCreatedOn(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Created on an unknown date";
  return `Created on ${date.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" })}`;
}

const PAGE_MARK_FALLBACK = (
  <FileText size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
);

function PageMark({ page }: { page: PageRowData }): React.ReactElement {
  const mark = projectMark(page.logo_props);
  if (mark.kind === "emoji") {
    return (
      <span aria-hidden="true" className="text-body inline-flex size-6 shrink-0 items-center justify-center">
        {mark.glyph}
      </span>
    );
  }
  if (mark.kind === "icon") {
    return (
      <span
        aria-hidden="true"
        className="text-body inline-flex size-6 shrink-0 items-center justify-center rounded-(--radius-control)"
        style={mark.color === null ? undefined : { color: mark.color }}
      >
        <ProjectMarkIcon mark={mark} identifier={mark.name} size={16} fallback={PAGE_MARK_FALLBACK} />
      </span>
    );
  }
  return PAGE_MARK_FALLBACK;
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
  canDuplicate,
  onOpenInNewTab,
  onCopyLink,
  onDuplicate,
}: PageRowProps): React.ReactElement {
  const title = pageDisplayTitle(page.name);
  const ownerName = owner?.name ?? "Unknown member";
  const entries = pageRowActions(
    { duplicate: canDuplicate },
    { openInNewTab: onOpenInNewTab, copyLink: onCopyLink, duplicate: onDuplicate }
  );
  const row = (
    <div
      data-testid="page-row"
      data-page-id={page.id}
      className="group flex items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-3) hover:bg-(--subtle)"
    >
      <Link
        to={pageDetailHref(workspaceSlug, projectId, page.id)}
        aria-label={title}
        className="flex min-w-0 flex-1 items-center gap-(--space-3) text-(--text)"
      >
        <PageMark page={page} />
        <span className="text-body min-w-0 flex-1 truncate font-medium hover:underline">{title}</span>
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
      </Link>
      {showStar ? <PageFavoriteStar workspaceSlug={workspaceSlug} projectId={projectId} page={page} /> : null}
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
