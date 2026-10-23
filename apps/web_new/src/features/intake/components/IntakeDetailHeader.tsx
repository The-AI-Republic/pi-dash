// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request detail header (parity/specs/intake.md INT-024/027). Previous
// and next chevrons step through the active list; the work-item link
// copies from the overflow menu on open requests (admins and members
// only) and from direct controls, next to the open-work-item entry, on
// resolved ones — where the menu is gone and every viewer gets both.
// The action area is slice E's: triage entries arrive through the
// actions slot.

import { IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { Link } from "@tanstack/react-router";
import { ChevronDown, ChevronUp, ExternalLink, MoreHorizontal } from "lucide-react";
import * as React from "react";

import {
  COPY_WORK_ITEM_LINK_LABEL,
  CopyWorkItemLinkButton,
  copyWorkItemLinkAddress,
} from "../../../shared/shell/CopyWorkItemLinkButton.js";
import { workItemPath } from "../../../shared/shell/shareLink.js";

/** Accessible name of the previous-request chevron. */
export const PREV_REQUEST_LABEL = "Previous request";
/** Accessible name of the next-request chevron. */
export const NEXT_REQUEST_LABEL = "Next request";
/** Accessible name of the overflow menu trigger. */
export const REQUEST_ACTIONS_LABEL = "Request actions";
/** Visible label of the open-work-item entry. */
export const OPEN_WORK_ITEM_LABEL = "Open work item";

export interface IntakeDetailHeaderProps {
  workspaceSlug: string;
  /** Route key shown to people, e.g. "WEB-123". */
  issueKey: string;
  /** Accepted, declined or duplicate: direct controls for every viewer, no menu. */
  resolved: boolean;
  /** Admin or member standing: the overflow menu on open requests. */
  canCopy: boolean;
  onPrev: () => void;
  onNext: () => void;
  /** Triage entries (slice E); empty until that slice lands. */
  actions?: React.ReactNode;
}

export function IntakeDetailHeader({
  workspaceSlug,
  issueKey,
  resolved,
  canCopy,
  onPrev,
  onNext,
  actions,
}: IntakeDetailHeaderProps): React.ReactElement {
  const menuItems: MenuEntry[] = [
    {
      label: COPY_WORK_ITEM_LINK_LABEL,
      onSelect: () => void copyWorkItemLinkAddress(workspaceSlug, issueKey),
    },
  ];
  return (
    <div
      data-testid="intake-detail-header"
      className="flex shrink-0 items-center justify-between gap-(--space-2) px-(--space-8) pt-(--space-6)"
    >
      <div className="flex items-center gap-(--space-1)">
        <span data-testid="intake-detail-prev">
          <IconButton label={PREV_REQUEST_LABEL} variant="ghost" size="small" onClick={onPrev}>
            <ChevronUp size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </span>
        <span data-testid="intake-detail-next">
          <IconButton label={NEXT_REQUEST_LABEL} variant="ghost" size="small" onClick={onNext}>
            <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </span>
      </div>
      <div className="flex items-center gap-(--space-2)">
        {actions}
        {resolved ? (
          <>
            <CopyWorkItemLinkButton workspaceSlug={workspaceSlug} issueKey={issueKey} />
            <Link
              to={workItemPath(workspaceSlug, issueKey)}
              data-testid="intake-detail-open-work-item"
              className="text-body inline-flex items-center gap-(--space-1) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--accent) hover:underline focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-1 focus-visible:outline-none"
            >
              <ExternalLink size={14} strokeWidth={1.5} aria-hidden="true" />
              {OPEN_WORK_ITEM_LABEL}
            </Link>
          </>
        ) : canCopy ? (
          <span data-testid="intake-detail-menu">
            <Menu
              trigger={
                <IconButton label={REQUEST_ACTIONS_LABEL} variant="ghost" size="small">
                  <MoreHorizontal size={16} strokeWidth={1.5} aria-hidden="true" />
                </IconButton>
              }
              items={menuItems}
            />
          </span>
        ) : null}
      </div>
    </div>
  );
}
