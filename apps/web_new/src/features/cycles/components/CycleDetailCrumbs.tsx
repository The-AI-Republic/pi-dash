// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail trail (parity/specs/cycles.md §13, CYC-033, CYC-034).
// Project → Cycles → the cycle itself, where the last crumb is the
// cycle switcher control (a searchable CyclePicker with completed
// cycles listed), never an inert label. The lifecycle marker rides
// beside the switcher name. Narrow screens collapse the trail to a
// back control returning to the cycles list.

import { Link, useNavigate } from "@tanstack/react-router";
import { Badge } from "@pidash/kit";
import { ChevronDown, ChevronLeft, ChevronRight } from "lucide-react";
import * as React from "react";

import { CyclePicker } from "../../../shared/pickers/index.js";
import { CYCLES_CRUMB, cyclesListHref, projectIssuesHref, switchCycleTarget } from "../lib/cycleScreen.js";
import { lifecycleLabel, lifecycleTone } from "../lib/cycleGroups.js";

export interface CycleDetailCrumbsProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  cycleId: string;
  cycleName: string;
  cycleStatus: string | undefined;
}

export function CycleDetailCrumbs({
  workspaceSlug,
  projectId,
  projectName,
  cycleId,
  cycleName,
  cycleStatus,
}: CycleDetailCrumbsProps): React.ReactElement {
  const navigate = useNavigate();
  const listHref = cyclesListHref(workspaceSlug, projectId);
  return (
    <>
      <nav aria-label="Breadcrumb" data-testid="cycle-detail-crumbs" className="hidden shrink-0 md:block">
        <ol className="flex min-w-0 items-center gap-(--space-1)">
          <li className="flex min-w-0 items-center gap-(--space-1)">
            <Link
              to={projectIssuesHref(workspaceSlug, projectId)}
              data-testid="cycle-crumb-project"
              className="text-body flex min-w-0 items-center gap-(--space-1) rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
            >
              <span className="max-w-40 truncate">{projectName}</span>
            </Link>
          </li>
          <li className="flex min-w-0 items-center gap-(--space-1)" aria-hidden="true">
            <ChevronRight size={14} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" />
          </li>
          <li className="flex min-w-0 items-center gap-(--space-1)">
            <Link
              to={listHref}
              data-testid="cycle-crumb-cycles"
              className="text-body flex min-w-0 items-center gap-(--space-1) rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
            >
              {CYCLES_CRUMB}
            </Link>
          </li>
          <li className="flex min-w-0 items-center gap-(--space-1)" aria-hidden="true">
            <ChevronRight size={14} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" />
          </li>
          <li className="flex min-w-0 items-center gap-(--space-1)">
            <CyclePicker
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              value={cycleId}
              onChange={(id) => {
                const target = switchCycleTarget(workspaceSlug, projectId, cycleId, id);
                if (target !== null) {
                  void navigate({ to: target });
                }
              }}
              allowClear={false}
              includeCompleted
              trigger={
                <button
                  type="button"
                  data-testid="cycle-switcher-trigger"
                  aria-label={`Switch cycle, current ${cycleName}`}
                  className="text-body flex min-w-0 items-center gap-(--space-1) rounded-(--radius-control) text-(--text)"
                >
                  <span className="max-w-40 truncate">{cycleName}</span>
                  <Badge tone={lifecycleTone(cycleStatus)} dot>
                    {lifecycleLabel(cycleStatus)}
                  </Badge>
                  <ChevronDown
                    size={14}
                    strokeWidth={1.5}
                    aria-hidden="true"
                    className="shrink-0 text-(--text-muted)"
                  />
                </button>
              }
            />
          </li>
        </ol>
      </nav>
      <div className="shrink-0 md:hidden">
        <Link
          to={listHref}
          data-testid="cycle-mobile-back"
          aria-label="Back to cycles"
          className="text-body inline-flex items-center gap-(--space-1) rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
        >
          <ChevronLeft size={16} strokeWidth={1.5} aria-hidden="true" />
          {CYCLES_CRUMB}
        </Link>
      </div>
    </>
  );
}
