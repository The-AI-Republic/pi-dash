// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared add-existing search modal (parity/specs/issues.md §2, ISS-012).
// Calendar/kanban/list group headers reuse it: search the project's
// issues, tick one or more, then attach them to the cycle or module the
// caller owns. The modal never writes membership itself — the caller
// passes the attach handler.

import { useQuery } from "@tanstack/react-query";
import { searchIssues, type IssueSearchHit } from "@pidash/api-client";
import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";

export interface AddExistingModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  projectId: string;
  /** Pre-select nothing; rows already attached stay out via the server. */
  onAttach: (issueIds: string[]) => Promise<void> | void;
  /** Exclude rows already in the target view. */
  excludeIds?: readonly string[] | undefined;
  /** Cycle search excludes rows already attached to any cycle. */
  excludeCycleMembers?: boolean | undefined;
  /** Module id excludes rows already attached to that module. */
  excludeModuleId?: string | undefined;
}

/** Search key for the modal's project hits. */
export function addExistingSearchKey(workspaceSlug: string, projectId: string, search: string) {
  return [...workspaceScope(workspaceSlug), "issues", "add-existing", projectId, search] as const;
}

export function AddExistingModal({
  open,
  onOpenChange,
  workspaceSlug,
  projectId,
  onAttach,
  excludeIds = [],
  excludeCycleMembers = false,
  excludeModuleId,
}: AddExistingModalProps): React.ReactElement {
  const [query, setQuery] = React.useState("");
  const [selected, setSelected] = React.useState<string[]>([]);
  const [submitting, setSubmitting] = React.useState(false);
  const hits = useQuery({
    queryKey: addExistingSearchKey(workspaceSlug, projectId, query),
    queryFn: () =>
      searchIssues(getAppClient(), workspaceSlug, projectId, {
        search: query,
        ...(excludeCycleMembers ? { cycle: true } : {}),
        ...(excludeModuleId === undefined ? {} : { module: excludeModuleId }),
      }),
    enabled: open,
    staleTime: REFERENCE_STALE_TIME_MS,
  });

  React.useEffect(() => {
    if (!open) {
      setQuery("");
      setSelected([]);
      setSubmitting(false);
    }
  }, [open]);

  function toggle(id: string): void {
    setSelected((current) => (current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id]));
  }

  async function submit(): Promise<void> {
    if (selected.length === 0) return;
    setSubmitting(true);
    try {
      await onAttach(selected);
      onOpenChange(false);
    } finally {
      setSubmitting(false);
    }
  }

  const rows = (hits.data ?? []).filter((row: IssueSearchHit) => !excludeIds.includes(row.id));

  return (
    <Dialog
      title="Add existing work items"
      description="Search this project and attach the ones you pick."
      open={open}
      onOpenChange={(next) => onOpenChange(next)}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => void submit()} disabled={selected.length === 0 || submitting}>
            Add selected work items
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <Input
          label="Search work items"
          placeholder="Search work items"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        {hits.isPending ? <p className="text-body text-(--text-muted)">Searching…</p> : null}
        {hits.isError ? <p className="text-body text-(--danger)">Couldn't search work items.</p> : null}
        {!hits.isPending && rows.length === 0 ? (
          <p className="text-body text-(--text-muted)">No matching work items.</p>
        ) : null}
        <ul className="flex max-h-72 flex-col overflow-y-auto">
          {rows.map((row) => {
            const checked = selected.includes(row.id);
            return (
              <li key={row.id}>
                <label
                  htmlFor={`issue-${row.id}`}
                  className="flex cursor-pointer items-center gap-(--space-3) rounded-(--radius-control) px-(--space-2) py-(--space-2) hover:bg-(--subtle)"
                >
                  <input
                    id={`issue-${row.id}`}
                    type="checkbox"
                    checked={checked}
                    onChange={() => toggle(row.id)}
                    className="size-4"
                  />
                  <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
                    {row.project__identifier}-{row.sequence_id}
                  </span>
                  <span className="text-body min-w-0 flex-1 truncate text-(--text)">{row.name}</span>
                </label>
              </li>
            );
          })}
        </ul>
      </div>
    </Dialog>
  );
}
