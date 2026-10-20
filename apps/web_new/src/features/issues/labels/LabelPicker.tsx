// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared label picker (parity/specs/issues.md §§11/13, ISS-132/159):
// the searchable multi-select of project labels, grouped by parent
// and child, that the list and spreadsheet rows, the sidebar, and the
// modal reuse. Labels load lazily on first open; admins type a name
// plus Enter to create inline (random color) while non-admins see an
// empty-result note. Selection is controlled: callers PATCH label_ids.

import type { Label } from "@pidash/api-client";
import { Button, Input, Popover, cn } from "@pidash/kit";
import { Check, ChevronDown } from "lucide-react";
import * as React from "react";

import { ROLE_ADMIN } from "../../../core/session/permissions.js";
import { useProjectRoles, useWorkspaceMembership } from "../../auth/index.js";
import { isProjectAdmin } from "../../projects/index.js";
import { notifyError } from "../../../shared/shell/index.js";
import { LABEL_CREATE_FAILED_TITLE, useCreateLabel, useLabels } from "./api.js";
import { randomLabelColor } from "./palette.js";
import { filterLabelTree, findLabelByName, selectLabelTree, type LabelGroup } from "./tree.js";
import { validateLabelName } from "./validation.js";

export interface LabelPickerProps {
  workspaceSlug: string;
  projectId: string;
  /** Selected label ids, controlled by the caller. */
  value: readonly string[];
  onChange: (ids: string[]) => void;
  /** Accessible name for the trigger. */
  label?: string | undefined;
  /** Lock the trigger (archived issues disable pickers). */
  disabled?: boolean | undefined;
  /** Trigger text before any label is chosen. */
  emptyText?: string | undefined;
}

interface PickerOption {
  label: Label;
  depth: 0 | 1;
}

/** Flatten the filtered tree to indented options, parents first. */
function flattenOptions(tree: readonly LabelGroup[]): PickerOption[] {
  const out: PickerOption[] = [];
  for (const group of tree) {
    out.push({ label: group.label, depth: 0 });
    for (const child of group.children) out.push({ label: child, depth: 1 });
  }
  return out;
}

export function LabelPicker({
  workspaceSlug,
  projectId,
  value,
  onChange,
  label = "Labels",
  disabled = false,
  emptyText = "Select labels",
}: LabelPickerProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const suppressClose = React.useRef(false);
  const labelsQuery = useLabels(workspaceSlug, projectId, open && !disabled);
  const rolesQuery = useProjectRoles(workspaceSlug);
  const membershipQuery = useWorkspaceMembership(workspaceSlug);
  const create = useCreateLabel(workspaceSlug, projectId);

  const rows = labelsQuery.data ?? [];
  const tree = React.useMemo(() => selectLabelTree(rows), [rows]);
  const filtered = React.useMemo(() => filterLabelTree(tree, query), [tree, query]);
  const options = React.useMemo(() => flattenOptions(filtered), [filtered]);

  // Inline create fails closed: without known admin standing the query
  // offers no create row (the server would refuse it anyway).
  const standingFailed = rolesQuery.isError || membershipQuery.isError;
  const projectRole = rolesQuery.data?.[projectId] ?? null;
  const workspaceRole = membershipQuery.data?.role ?? 0;
  const canCreate = !standingFailed && !disabled && (isProjectAdmin(projectRole) || workspaceRole >= ROLE_ADMIN);

  function toggle(labelId: string): void {
    if (value.includes(labelId)) onChange(value.filter((id) => id !== labelId));
    else onChange([...value, labelId]);
  }

  async function submitQuery(): Promise<void> {
    const trimmed = query.trim();
    if (trimmed === "") return;
    const exact = findLabelByName(rows, trimmed);
    if (exact !== null) {
      toggle(exact.id);
      return;
    }
    const first = filtered[0]?.label ?? null;
    if (first !== null) {
      toggle(first.id);
      return;
    }
    if (!canCreate) return;
    const issue = validateLabelName(trimmed);
    if (issue !== null) {
      notifyError(LABEL_CREATE_FAILED_TITLE, issue);
      return;
    }
    try {
      const created = await create.mutateAsync({ name: trimmed, color: randomLabelColor() });
      onChange([...value, created.id]);
      setQuery("");
    } catch {
      // The mutation notices (duplicates land here); the query stays
      // for another attempt.
    }
  }

  function onSearchKeyDown(event: React.KeyboardEvent<HTMLInputElement>): void {
    if (event.key === "Enter") {
      event.preventDefault();
      void submitQuery();
    } else if (event.key === "Escape" && query !== "") {
      // First Escape clears the query and holds the popover; the next
      // one closes it (common picker behavior, ISS-207).
      event.preventDefault();
      suppressClose.current = true;
      setQuery("");
    }
  }

  function onOpenChange(next: boolean): void {
    if (!next && suppressClose.current) {
      suppressClose.current = false;
      return;
    }
    setOpen(next);
    if (!next) setQuery("");
  }

  const trimmed = query.trim();
  return (
    <Popover
      trigger={
        <button
          type="button"
          aria-label={label}
          disabled={disabled}
          tabIndex={-1}
          data-testid="label-picker-trigger"
          className="text-body inline-flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-2) text-(--text-muted) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-50"
        >
          {value.length === 0 && emptyText === "" ? null : (
            <span className="min-w-0 flex-1 truncate text-left">
              {value.length === 0 ? emptyText : `${value.length} selected`}
            </span>
          )}
          <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
        </button>
      }
      open={open}
      onOpenChange={onOpenChange}
    >
      <div className="flex flex-col gap-(--space-3)">
        <Input
          label="Search labels"
          data-testid="label-picker-search"
          value={query}
          placeholder="Search labels"
          autoComplete="off"
          autoFocus
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={onSearchKeyDown}
        />
        {labelsQuery.isPending ? (
          <p data-testid="label-picker-loading" className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">
            Loading labels…
          </p>
        ) : options.length === 0 ? (
          trimmed === "" ? (
            <p data-testid="label-picker-empty" className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">
              No labels yet
            </p>
          ) : canCreate ? (
            <Button
              variant="ghost"
              data-testid="label-picker-create"
              loading={create.isPending}
              onClick={() => void submitQuery()}
            >
              {`Create "${trimmed}"`}
            </Button>
          ) : (
            <p data-testid="label-picker-empty" className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">
              No matching results
            </p>
          )
        ) : (
          <ul aria-label="Project labels" className="flex max-h-64 flex-col gap-px overflow-y-auto">
            {options.map((option) => {
              const selected = value.includes(option.label.id);
              return (
                <li key={option.label.id}>
                  <button
                    type="button"
                    aria-pressed={selected}
                    data-testid="label-picker-option"
                    data-label-id={option.label.id}
                    onClick={() => toggle(option.label.id)}
                    className={cn(
                      "flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left hover:bg-(--subtle)",
                      option.depth === 1 && "pl-(--space-8)"
                    )}
                  >
                    <span
                      aria-hidden="true"
                      style={{ backgroundColor: option.label.color }}
                      className="size-2.5 shrink-0 rounded-full"
                    />
                    <span className="text-body min-w-0 flex-1 truncate text-(--text)">{option.label.name}</span>
                    {selected ? (
                      <Check size={16} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--accent)" />
                    ) : null}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </Popover>
  );
}
