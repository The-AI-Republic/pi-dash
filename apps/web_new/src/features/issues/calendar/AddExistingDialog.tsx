// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add-existing work items to a calendar day (parity/specs/issues.md §4,
// ISS-026). The picker reads root items through the shared search
// contract, hides anything whose start date falls after the chosen day,
// then re-dates every checked item onto that day.

import { useQuery } from "@tanstack/react-query";
import { Button, Dialog } from "@pidash/kit";
import type { IssueSearchHit } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { canAttachToDay } from "./model.js";
import { calendarSearchQueryOptions } from "./query.js";

export const ADD_EXISTING_TITLE = "Add existing work items";
export const ADD_EXISTING_EMPTY = "No work items found";
export const ADD_EXISTING_SUBMIT = "Add selected work items";

export interface AddExistingDialogProps {
  open: boolean;
  day: string;
  workspaceSlug: string;
  projectId: string;
  submitting: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (issueIds: string[]) => void;
}

/** Choices that may land on the day, with the day's own items excluded. */
export function attachableHits(hits: readonly IssueSearchHit[], day: string): IssueSearchHit[] {
  return hits.filter((hit) => canAttachToDay({ start_date: hit.start_date }, day));
}

export function AddExistingDialog({
  open,
  day,
  workspaceSlug,
  projectId,
  submitting,
  onOpenChange,
  onSubmit,
}: AddExistingDialogProps): React.ReactElement {
  const { data, isLoading } = useQuery({
    ...calendarSearchQueryOptions(getAppClient(), workspaceSlug, projectId),
    enabled: open,
  });
  const [selected, setSelected] = React.useState<Set<string>>(() => new Set());
  const hits = attachableHits(data ?? [], day);

  React.useEffect(() => {
    if (!open) setSelected(new Set());
  }, [open]);

  function toggle(id: string): void {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <Dialog
      title={ADD_EXISTING_TITLE}
      description="Pick work items to place on this day."
      open={open}
      onOpenChange={(next) => onOpenChange(next)}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={selected.size === 0 || submitting}
            onClick={() => onSubmit([...selected])}
          >
            {ADD_EXISTING_SUBMIT}
          </Button>
        </>
      }
    >
      {isLoading ? (
        <p className="text-body text-(--text-muted)">Loading work items…</p>
      ) : hits.length === 0 ? (
        <p className="text-body text-(--text-muted)">{ADD_EXISTING_EMPTY}</p>
      ) : (
        <div className="flex max-h-80 flex-col gap-(--space-1) overflow-y-auto">
          {hits.map((hit) => (
            <label
              key={hit.id}
              htmlFor={`issue-${hit.id}`}
              className="flex cursor-pointer items-center gap-(--space-3) rounded-(--radius-control) px-(--space-2) py-(--space-2) hover:bg-(--subtle)"
            >
              <input
                id={`issue-${hit.id}`}
                type="checkbox"
                checked={selected.has(hit.id)}
                onChange={() => toggle(hit.id)}
              />
              <span className="text-caption font-[family-name:var(--font-mono)] text-(--text-muted)">
                {hit.project__identifier}-{hit.sequence_id}
              </span>
              <span className="text-body min-w-0 flex-1 truncate text-(--text)">{hit.name}</span>
            </label>
          ))}
        </div>
      )}
    </Dialog>
  );
}
