// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Applied-filter chips (parity/specs/intake.md INT-009): one group per
// section with a removable pill per value, and a clear-all control on
// every group but status. The status group answers to the last-value
// guard instead: a single remaining status keeps its pill with no
// remove control. Values the reference reads no longer know are
// skipped; the filter itself stays active.

import { X } from "lucide-react";
import * as React from "react";

import type { IntakeStatus } from "@pidash/api-client";

import { PRIORITY_LABELS } from "../../../shared/pickers/index.js";
import { intakeDateChipLabel, type IntakeDateValue } from "../lib/dates.js";
import {
  clearFilterGroup,
  INTAKE_FILTER_SECTION_TITLES,
  toggleFilterId,
  toggleFilterStatus,
  type IntakeFilterSection,
  type IntakeFilterSelection,
} from "../lib/filters.js";
import { INTAKE_STATUS_LABELS } from "../lib/statuses.js";

export interface IntakeChipsProps {
  selection: IntakeFilterSelection;
  memberNameById: ReadonlyMap<string, string>;
  labelNameById: ReadonlyMap<string, string>;
  onSelectionChange: (next: IntakeFilterSelection) => void;
}

interface ChipValue {
  /** Stable identity for removal. */
  id: string;
  display: string;
  onRemove: (() => void) | null;
}

function priorityDisplay(level: string): string {
  return (PRIORITY_LABELS as Readonly<Record<string, string>>)[level] ?? level;
}

export function IntakeChips({
  selection,
  memberNameById,
  labelNameById,
  onSelectionChange,
}: IntakeChipsProps): React.ReactElement {
  const remove =
    (next: IntakeFilterSelection): (() => void) =>
    () =>
      onSelectionChange(next);
  const dateValue = (section: "createdAt" | "updatedAt", value: IntakeDateValue): ChipValue => ({
    id: `${section}:${intakeDateChipLabel(value)}`,
    display: intakeDateChipLabel(value),
    onRemove: remove(clearFilterGroup(selection, section)),
  });
  const groups: { key: IntakeFilterSection; values: ChipValue[]; onClear: (() => void) | null }[] = [
    {
      key: "status",
      values: selection.statuses.map((status: IntakeStatus) => ({
        id: `status:${status}`,
        display: INTAKE_STATUS_LABELS[status],
        onRemove: selection.statuses.length === 1 ? null : remove(toggleFilterStatus(selection, status)),
      })),
      onClear: null,
    },
    {
      key: "priority",
      values: selection.priorities.map((level) => ({
        id: `priority:${level}`,
        display: priorityDisplay(level),
        onRemove: remove(toggleFilterId(selection, "priorities", level)),
      })),
      onClear: selection.priorities.length === 0 ? null : remove(clearFilterGroup(selection, "priority")),
    },
    {
      key: "assignees",
      values: selection.assignees.flatMap((id) => {
        const display = memberNameById.get(id);
        if (display === undefined) return [];
        return [{ id: `assignees:${id}`, display, onRemove: remove(toggleFilterId(selection, "assignees", id)) }];
      }),
      onClear: selection.assignees.length === 0 ? null : remove(clearFilterGroup(selection, "assignees")),
    },
    {
      key: "createdBy",
      values: selection.createdBy.flatMap((id) => {
        const display = memberNameById.get(id);
        if (display === undefined) return [];
        return [{ id: `createdBy:${id}`, display, onRemove: remove(toggleFilterId(selection, "createdBy", id)) }];
      }),
      onClear: selection.createdBy.length === 0 ? null : remove(clearFilterGroup(selection, "createdBy")),
    },
    {
      key: "labels",
      values: selection.labels.flatMap((id) => {
        const display = labelNameById.get(id);
        if (display === undefined) return [];
        return [{ id: `labels:${id}`, display, onRemove: remove(toggleFilterId(selection, "labels", id)) }];
      }),
      onClear: selection.labels.length === 0 ? null : remove(clearFilterGroup(selection, "labels")),
    },
    {
      key: "createdAt",
      values: selection.createdAt === null ? [] : [dateValue("createdAt", selection.createdAt)],
      onClear: selection.createdAt === null ? null : remove(clearFilterGroup(selection, "createdAt")),
    },
    {
      key: "updatedAt",
      values: selection.updatedAt === null ? [] : [dateValue("updatedAt", selection.updatedAt)],
      onClear: selection.updatedAt === null ? null : remove(clearFilterGroup(selection, "updatedAt")),
    },
  ];
  const visible = groups.filter((group) => group.values.length > 0);
  return (
    <div data-testid="intake-chips" className="flex flex-col gap-(--space-1)">
      {visible.map((group) => {
        const title = INTAKE_FILTER_SECTION_TITLES[group.key];
        return (
          <div
            key={group.key}
            data-testid="intake-chip-group"
            data-chip-group={group.key}
            className="flex flex-wrap items-center gap-(--space-1)"
          >
            <span className="text-meta text-(--text-muted)">{title}</span>
            {group.values.map((value) => (
              <span
                key={value.id}
                data-testid="intake-chip-value"
                data-chip-value={value.display}
                className="text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px text-(--text)"
              >
                {value.display}
                {value.onRemove === null ? null : (
                  <button
                    type="button"
                    aria-label={`Remove ${value.display} ${title} filter`}
                    onClick={value.onRemove}
                    className="grid size-4 place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
                  >
                    <X size={12} strokeWidth={2} aria-hidden="true" />
                  </button>
                )}
              </span>
            ))}
            {group.onClear === null ? null : (
              <button
                type="button"
                data-testid="intake-chip-clear"
                data-chip-group={group.key}
                aria-label={`Clear ${title} filters`}
                onClick={group.onClear}
                className="grid size-4 place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
              >
                <X size={12} strokeWidth={2} aria-hidden="true" />
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}
