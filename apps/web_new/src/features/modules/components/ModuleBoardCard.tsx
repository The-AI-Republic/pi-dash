// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One module card in the board layout (parity/specs/modules.md §3.6,
// MOD-020). The card carries the module's identity at a glance: the name
// (truncated, with the full name on hover), the status pill, the peek
// trigger, the done-over-total count, the lead or a no-lead marker, a
// segmented bar by work-item state, the date range, and the favorite and
// quick-menu actions.
//
// A press on the card body opens the detail page; a press on any control
// does that control's thing and never navigates. The status, date, star and
// menu controls themselves are slices E and H's; this card defines the
// slots — each with a stable test handle — and keeps them out of the
// navigation surface.

import { Link, useNavigate } from "@tanstack/react-router";
import { Avatar, IconButton, Tooltip } from "@pidash/kit";
import type { ModuleRow } from "@pidash/api-client";
import { Info, SquareUser } from "lucide-react";
import * as React from "react";

import { moduleDetailHref } from "../lib/moduleList.js";
import { modulePressHitControl } from "../lib/modulePress.js";
import { moduleLeadSlot, moduleWorkItemCountText } from "../lib/moduleIdentity.js";
import { ModuleSegmentedBar } from "./ModuleSegmentedBar.js";

/** The lead as the card shows it: a person, or the no-lead mark. */
export interface ModuleCardLead {
  name: string;
  avatarUrl: string | null;
}

export interface ModuleBoardCardProps {
  workspaceSlug: string;
  projectId: string;
  module: ModuleRow;
  /** The lead's directory entry, when the surface resolved one. */
  lead?: ModuleCardLead | null;
  /** The lifecycle status pill. Slot owner: NEWFRONT-797 (MOD-014). */
  statusControl?: React.ReactNode;
  /** The date range control. Slot owner: NEWFRONT-797 (MOD-013). */
  dates?: React.ReactNode;
  /** The favorite star. Slot owner: NEWFRONT-797 (MOD-016). */
  favorite?: React.ReactNode;
  /** The quick menu. Slot owner: NEWFRONT-800 (MOD-017). */
  menu?: React.ReactNode;
  /** Open this module's peek from the card's info control. */
  onOpenPeek?: ((moduleId: string) => void) | undefined;
}

/** A control wrapper that swallows the press so the card body stays put. */
function ControlRegion({ children, ...rest }: React.HTMLAttributes<HTMLDivElement>): React.ReactElement {
  return (
    <div
      onClick={(event) => {
        event.stopPropagation();
      }}
      onKeyDown={(event) => {
        event.stopPropagation();
      }}
      {...rest}
    >
      {children}
    </div>
  );
}

function CardLead({ lead }: { lead: ModuleCardLead | null }): React.ReactElement {
  if (lead === null) {
    return (
      <span
        data-testid="modules-card-lead"
        data-lead-kind="none"
        title="No lead"
        className="inline-flex items-center text-(--text-muted)"
      >
        <SquareUser size={16} strokeWidth={1.5} aria-hidden="true" />
        <span className="sr-only">No lead</span>
      </span>
    );
  }
  return (
    <span data-testid="modules-card-lead" data-lead-kind="avatar" className="inline-flex items-center">
      <Avatar name={lead.name} size="small" {...(lead.avatarUrl === null ? {} : { src: lead.avatarUrl })} />
    </span>
  );
}

export function ModuleBoardCard({
  workspaceSlug,
  projectId,
  module,
  lead = null,
  statusControl,
  dates,
  favorite,
  menu,
  onOpenPeek,
}: ModuleBoardCardProps): React.ReactElement {
  const navigate = useNavigate();
  const href = moduleDetailHref(workspaceSlug, projectId, module.id);
  const leadSlot = moduleLeadSlot(module);

  const openDetail = (event: React.MouseEvent<HTMLElement>): void => {
    if (modulePressHitControl(event)) return;
    void navigate({ to: href });
  };

  return (
    <li
      data-testid="modules-board-card"
      data-module-id={module.id}
      data-module-name={module.name}
      data-module-status={module.status}
      onClick={openDetail}
      className="relative flex cursor-pointer flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-4) hover:border-(--text-muted)"
    >
      <div className="flex items-start justify-between gap-(--space-3)">
        <Tooltip content={module.name}>
          <Link to={href} data-testid="modules-board-card-link" className="min-w-0 flex-1">
            <span
              data-testid="modules-board-card-name"
              title={module.name}
              className="text-body block truncate font-medium text-(--text)"
            >
              {module.name}
            </span>
          </Link>
        </Tooltip>
        <ControlRegion data-testid="modules-card-status" className="flex shrink-0 items-center gap-(--space-1)">
          {statusControl}
          <IconButton
            label={`Open preview of ${module.name}`}
            size="small"
            data-testid="modules-board-card-peek"
            onClick={(event) => {
              event.stopPropagation();
              onOpenPeek?.(module.id);
            }}
          >
            <Info size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </ControlRegion>
      </div>

      <div className="flex items-center justify-between gap-(--space-2)">
        <span data-testid="modules-board-card-count" className="text-meta text-(--text-muted)">
          {moduleWorkItemCountText(module)}
        </span>
        <CardLead lead={leadSlot.kind === "avatar" ? lead : null} />
      </div>

      <ModuleSegmentedBar module={module} />

      <ControlRegion data-testid="modules-card-dates" className="py-0.5">
        {dates}
      </ControlRegion>

      <div
        data-testid="modules-card-actions"
        className="absolute right-(--space-4) bottom-(--space-4) flex items-center gap-(--space-1)"
      >
        <ControlRegion data-testid="modules-card-favorite">{favorite}</ControlRegion>
        <ControlRegion data-testid="modules-card-menu">{menu}</ControlRegion>
      </div>
    </li>
  );
}
