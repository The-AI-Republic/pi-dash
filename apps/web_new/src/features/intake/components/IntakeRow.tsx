// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One intake request row (parity/specs/intake.md INT-006, INT-034): the
// project-scoped id, the status chip (hidden while pending), the source
// pill (cloud only), the title, the created date with its tooltip, the
// priority marker with its tooltip, the label chips (four or more
// collapse to one count chip), and the author avatar — the intake
// identity avatar for system-authored rows. The row is a selection
// control: activating it selects the request, and the selected row is
// marked.

import { Avatar, Tooltip } from "@pidash/kit";
import type { IntakeIssueRow, WorkspaceMemberList } from "@pidash/api-client";
import * as React from "react";

import { formatDay } from "../../issues/index.js";
import { PRIORITY_ICONS, PRIORITY_LABELS, PRIORITY_TONES } from "../../../shared/pickers/index.js";
import { isIntakeIdentityCreator } from "../lib/identity.js";
import { intakeRowIdLabel } from "../lib/rows.js";
import { INTAKE_STATUS_LABELS, intakeStatusChipLabel } from "../lib/statuses.js";
import { IntakeSourcePill } from "./IntakeSourcePill.js";

export interface IntakeRowLabel {
  name: string;
  color: string;
}

export interface IntakeRowProps {
  row: IntakeIssueRow;
  projectIdentifier: string;
  labelById: ReadonlyMap<string, IntakeRowLabel>;
  directory: WorkspaceMemberList | undefined;
  selected: boolean;
  onSelect: (issueId: string) => void;
}

/** Accessible name: the identifier, title and status (INT-006). */
function rowAccessibleName(idLabel: string, title: string, status: number): string {
  const statusLabel = (INTAKE_STATUS_LABELS as Readonly<Record<number, string>>)[status] ?? "Unknown";
  return `${idLabel} ${title}, ${statusLabel}`;
}

function IntakeRowAvatar({
  row,
  directory,
}: {
  row: IntakeIssueRow;
  directory: WorkspaceMemberList | undefined;
}): React.ReactElement | null {
  const creator = row.issue.created_by;
  const entry = creator === null ? undefined : directory?.find((row) => row.member.id === creator);
  if (entry === undefined) return null;
  if (isIntakeIdentityCreator(directory, creator)) {
    return (
      <Tooltip content="Intake">
        <span data-testid="intake-row-avatar" data-avatar-kind="intake">
          <Avatar name="Intake" size="small" />
        </span>
      </Tooltip>
    );
  }
  return (
    <Tooltip content={entry.member.display_name}>
      <span data-testid="intake-row-avatar" data-avatar-kind="member">
        <Avatar
          name={entry.member.display_name}
          {...(entry.member.avatar_url === null ? {} : { src: entry.member.avatar_url })}
          size="small"
        />
      </span>
    </Tooltip>
  );
}

export function IntakeRow({
  row,
  projectIdentifier,
  labelById,
  directory,
  selected,
  onSelect,
}: IntakeRowProps): React.ReactElement {
  const issue = row.issue;
  const idLabel = intakeRowIdLabel(projectIdentifier, issue.sequence_id);
  const chip = intakeStatusChipLabel(row.status);
  const day = formatDay(issue.created_at.slice(0, 10));
  const priority = issue.priority;
  const PriorityGlyph = PRIORITY_ICONS[priority];
  const labels = (issue.label_ids ?? [])
    .map((id) => labelById.get(id))
    .filter((label): label is IntakeRowLabel => label !== undefined);
  return (
    <button
      type="button"
      data-testid="intake-row"
      data-issue-id={issue.id}
      aria-current={selected ? "true" : undefined}
      aria-label={rowAccessibleName(idLabel, issue.name, row.status)}
      onClick={() => onSelect(issue.id)}
      className={
        selected
          ? "flex w-full flex-col gap-(--space-1) border-b border-(--border) bg-(--subtle) px-(--space-4) py-(--space-2) text-left"
          : "flex w-full flex-col gap-(--space-1) border-b border-(--border) px-(--space-4) py-(--space-2) text-left hover:bg-(--subtle)"
      }
    >
      <span className="flex items-center gap-(--space-2)">
        <span data-testid="intake-row-id" className="text-meta text-(--text-muted)">
          {idLabel}
        </span>
        {chip === null ? null : (
          <span
            data-testid="intake-row-status"
            className="text-meta rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)"
          >
            {chip}
          </span>
        )}
        <IntakeSourcePill source={row.source} />
      </span>
      <h3 data-testid="intake-row-title" title={issue.name} className="text-body truncate text-(--text)">
        {issue.name}
      </h3>
      <span className="flex items-center gap-(--space-2)">
        <Tooltip content={`Created date: ${day}`}>
          <span tabIndex={0} data-testid="intake-row-date" className="text-meta text-(--text-muted)">
            {day}
          </span>
        </Tooltip>
        <Tooltip content={priority}>
          <span
            tabIndex={0}
            role="img"
            aria-label={`Priority: ${PRIORITY_LABELS[priority]}`}
            data-testid="intake-row-priority"
            data-priority-key={priority}
            className={`inline-flex items-center ${PRIORITY_TONES[priority]}`}
          >
            <PriorityGlyph size={14} aria-hidden="true" />
          </span>
        </Tooltip>
        {labels.length === 0 ? null : labels.length >= 4 ? (
          <span data-testid="intake-row-labels">
            <span
              data-testid="intake-row-label"
              className="text-meta rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)"
            >
              {labels.length} labels
            </span>
          </span>
        ) : (
          <span data-testid="intake-row-labels" className="flex items-center gap-(--space-1)">
            {labels.map((label) => (
              <span
                key={label.name}
                data-testid="intake-row-label"
                className="text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)"
              >
                <span
                  aria-hidden="true"
                  style={{ backgroundColor: label.color }}
                  className="size-1.5 shrink-0 rounded-full"
                />
                {label.name}
              </span>
            ))}
          </span>
        )}
        <span className="ml-auto inline-flex shrink-0 items-center">
          <IntakeRowAvatar row={row} directory={directory} />
        </span>
      </span>
    </button>
  );
}
