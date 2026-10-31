// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Applied-filter strip (SHELL-030). Removable pills per picked value,
// a match count, and one-click clearing that leaves the archive view
// alone. Written from parity/specs/shell.md §3.

import { Avatar } from "@pidash/kit";
import { X } from "lucide-react";
import * as React from "react";
import type { WorkspaceMember } from "@pidash/api-client";

import {
  ACCESS_OPTIONS,
  formatCreatedChip,
  hasNarrowingListFilters,
  type ListFilters,
  type ValueFilterDimension,
} from "../lib/list-display.js";

export interface ProjectsAppliedFiltersProps {
  filters: ListFilters;
  /** Resolved memberships for person chip names; unknown ids hide. */
  members: WorkspaceMember[];
  filteredCount: number;
  totalCount: number;
  onRemoveMyProjects: () => void;
  onRemoveValue: (dimension: ValueFilterDimension, value: string) => void;
  onRemoveDimension: (dimension: ValueFilterDimension) => void;
  onClearAll: () => void;
}

function Chip({
  label,
  avatar,
  onRemove,
}: {
  label: string;
  avatar?: { name: string; src: string } | undefined;
  onRemove: () => void;
}): React.ReactElement {
  return (
    <span className="text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px">
      {avatar === undefined ? null : <Avatar name={avatar.name} src={avatar.src} size="small" />}
      {label}
      <button
        type="button"
        aria-label={`Remove ${label}`}
        onClick={onRemove}
        className="grid cursor-pointer place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
      >
        <X size={12} strokeWidth={2} aria-hidden="true" />
      </button>
    </span>
  );
}

function personName(members: WorkspaceMember[], id: string): { name: string; src: string } | null {
  const found = members.find((membership) => membership.member.id === id);
  if (!found) return null;
  return { name: found.member.display_name, src: found.member.avatar_url ?? "" };
}

interface ChipModel {
  key: string;
  label: string;
  person: { name: string; src: string } | null;
}

function visibleChip(chip: ChipModel | null): chip is ChipModel {
  return chip !== null && chip.label !== "";
}

export function ProjectsAppliedFilters({
  filters,
  members,
  filteredCount,
  totalCount,
  onRemoveMyProjects,
  onRemoveValue,
  onRemoveDimension,
  onClearAll,
}: ProjectsAppliedFiltersProps): React.ReactElement | null {
  if (!hasNarrowingListFilters(filters)) return null;

  const groups: ReadonlyArray<{ dimension: ValueFilterDimension; label: string }> = [
    { dimension: "access", label: "Access" },
    { dimension: "lead", label: "Lead" },
    { dimension: "members", label: "Members" },
    { dimension: "createdAt", label: "Created date" },
  ];

  const accessLabel = (value: number): string => ACCESS_OPTIONS.find((option) => option.value === value)?.label ?? "";

  return (
    <div data-testid="projects-applied-filters" className="flex flex-wrap items-center gap-(--space-2)">
      {groups.map((group) => {
        const values =
          group.dimension === "access"
            ? filters.access.map((value) => ({ key: `${value}`, label: accessLabel(value), person: null }))
            : group.dimension === "createdAt"
              ? filters.createdAt.map((value) => ({ key: value, label: formatCreatedChip(value), person: null }))
              : filters[group.dimension].map((id) => {
                  const person = personName(members, id);
                  return person === null
                    ? null
                    : { key: id, label: person.name, person: { name: person.name, src: person.src } };
                });
        const chips = values.filter(visibleChip);
        if (chips.length === 0) return null;
        return (
          <span
            key={group.dimension}
            className="inline-flex flex-wrap items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) px-(--space-2) py-(--space-1)"
          >
            <span className="text-meta text-(--text-muted)">{group.label}</span>
            {chips.map((chip) => (
              <Chip
                key={chip.key}
                label={chip.label}
                avatar={chip.person ?? undefined}
                onRemove={() => onRemoveValue(group.dimension, chip.key)}
              />
            ))}
            <button
              type="button"
              aria-label={`Clear ${group.label} filters`}
              onClick={() => onRemoveDimension(group.dimension)}
              className="grid cursor-pointer place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
            >
              <X size={12} strokeWidth={2} aria-hidden="true" />
            </button>
          </span>
        );
      })}
      {filters.myProjects ? (
        <span className="inline-flex flex-wrap items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) px-(--space-2) py-(--space-1)">
          <span className="text-meta text-(--text-muted)">Projects</span>
          <Chip label="My projects" onRemove={onRemoveMyProjects} />
        </span>
      ) : null}
      <button
        type="button"
        onClick={onClearAll}
        className="text-meta cursor-pointer rounded-(--radius-full) border border-(--border) px-(--space-3) py-px hover:bg-(--subtle)"
      >
        Clear all
      </button>
      <span
        data-testid="projects-filter-count"
        title={`${filteredCount} of ${totalCount} projects match the applied filters.`}
        className="text-meta ml-auto rounded-full bg-(--subtle) px-(--space-3) py-px font-medium"
      >
        {filteredCount}/{totalCount}
      </span>
    </div>
  );
}
