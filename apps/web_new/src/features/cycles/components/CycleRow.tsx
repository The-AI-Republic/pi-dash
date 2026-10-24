// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One cycle row (parity/specs/cycles.md §2, CYC-002). Plain cycle rows,
// not issue rows: the name links the canonical detail address, then
// the completion ring, the lifecycle marker, the project-timezone date
// span, and the extras for the row's state — the work-item count on
// draft and upcoming rows, the creator avatar on the active row,
// creator plus assignees elsewhere. The favorite star is the working
// toggle (CYC-029). The row menu trigger (slice E) sits beside the
// quick-look eye.

import { Link } from "@tanstack/react-router";
import { Avatar, Badge, IconButton } from "@pidash/kit";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import { Eye, Layers, Users } from "lucide-react";
import * as React from "react";

import { groupOfCycle, lifecycleLabel, lifecycleTone } from "../lib/cycleGroups.js";
import { UNKNOWN_CYCLE_MEMBER, type CycleMemberEntry } from "../lib/cycleMembers.js";
import { cycleDetailPath } from "../lib/cycleScreen.js";
import { cycleProgressPercent } from "../lib/cycleProgress.js";
import { canTransferFromRow, transferableWorkCount } from "../lib/cycleTransfer.js";
import type { CycleProjectRef } from "../dialog/CycleDialog.js";
import { CycleDateSpan } from "./CycleDateSpan.js";
import { CycleFavoriteStar } from "./CycleFavoriteStar.js";
import { CycleProgressRing } from "./CycleProgressRing.js";
import { CycleRowMenu } from "./CycleRowMenu.js";
import { CycleTransferAction } from "./CycleTransferAction.js";

export interface CycleRowProps {
  workspaceSlug: string;
  projectId: string;
  cycle: CycleRowData;
  /** IANA name of the project's timezone. */
  projectTimeZone: string;
  /** Workspace directory behind the creator and assignee avatars. */
  directory: ReadonlyMap<string, CycleMemberEntry>;
  /**
   * Members and admins on live rows only; guests see no star at all,
   * and archived rows never offer one (slice E passes false).
   */
  showFavoriteMarker: boolean;
  /** Open (or toggle shut) this row's quick look. */
  onPeek: (cycleId: string) => void;
  /** Members and admins mutate; guests keep the menu's link actions only. */
  canMutate: boolean;
  /** The project in the address: the row menu's edit dialog label. */
  currentProject: CycleProjectRef;
}

function memberAvatar(entry: CycleMemberEntry, key: string): React.ReactElement {
  const image = entry.avatarUrl === null ? {} : { src: entry.avatarUrl };
  return <Avatar key={key} name={entry.displayName} size="small" className="ring-1 ring-(--surface)" {...image} />;
}

export function CycleRow({
  workspaceSlug,
  projectId,
  cycle,
  projectTimeZone,
  directory,
  showFavoriteMarker,
  onPeek,
  canMutate,
  currentProject,
}: CycleRowProps): React.ReactElement {
  const group = groupOfCycle(cycle.status);
  const isCurrent = group === null;
  const assigneeIds = cycle.assignee_ids ?? [];
  const creator =
    cycle.created_by === null || cycle.created_by === undefined
      ? null
      : (directory.get(cycle.created_by) ?? UNKNOWN_CYCLE_MEMBER);
  return (
    <div
      data-testid="cycle-row"
      data-cycle-id={cycle.id}
      className="group flex items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-3)"
    >
      <Link
        to={cycleDetailPath(workspaceSlug, projectId, cycle.id)}
        className="flex min-w-0 flex-1 items-center gap-(--space-3)"
      >
        <span className="text-body min-w-0 flex-1 truncate font-medium text-(--text)">{cycle.name}</span>
        <CycleProgressRing percent={cycleProgressPercent(cycle)} />
        <Badge tone={lifecycleTone(cycle.status)} dot>
          {lifecycleLabel(cycle.status)}
        </Badge>
        <CycleDateSpan startDate={cycle.start_date} endDate={cycle.end_date} projectTimeZone={projectTimeZone} />
        {group === "upcoming" ? (
          <span className="text-caption inline-flex shrink-0 items-center gap-(--space-1) text-(--text-muted)">
            <Layers size={14} strokeWidth={1.5} aria-hidden="true" />
            <span>{cycle.total_issues ?? 0}</span>
            <span className="sr-only">work items</span>
          </span>
        ) : null}
        {isCurrent ? (
          creator === null ? null : (
            memberAvatar(creator, creator.displayName)
          )
        ) : (
          <span className="flex shrink-0 -space-x-1">
            {creator === null ? null : memberAvatar(creator, creator.displayName)}
            {assigneeIds.map((id) => memberAvatar(directory.get(id) ?? UNKNOWN_CYCLE_MEMBER, id))}
            {assigneeIds.length === 0 ? (
              <span
                aria-label="No assignees"
                className="text-caption inline-flex size-6 items-center justify-center rounded-(--radius-full) bg-(--subtle) text-(--text-muted)"
              >
                <Users size={14} strokeWidth={1.5} aria-hidden="true" />
              </span>
            ) : null}
          </span>
        )}
      </Link>
      {showFavoriteMarker ? (
        <CycleFavoriteStar workspaceSlug={workspaceSlug} projectId={projectId} cycle={cycle} />
      ) : null}
      <IconButton
        label={`Quick look at ${cycle.name}`}
        size="small"
        onClick={() => onPeek(cycle.id)}
        className="shrink-0 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100"
      >
        <Eye size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
      <CycleRowMenu
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        cycle={cycle}
        canMutate={canMutate}
        currentProject={currentProject}
      />
      {canMutate && canTransferFromRow(cycle) ? (
        <CycleTransferAction
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          sourceCycleId={cycle.id}
          label={`Transfer ${transferableWorkCount(cycle)} work items`}
          variant="secondary"
          size="small"
          className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100"
        />
      ) : null}
    </div>
  );
}
