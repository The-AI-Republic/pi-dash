// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add-relation picker (parity/specs/issues.md §15, ISS-180). The chosen
// kind arrives from the section menu; the dialog multi-selects work items
// from the workspace search, with a toggle that widens the search beyond
// the current project. Nothing selected is a refusal, not an empty submit.

import * as React from "react";

import { Button, Dialog, Input, Spinner, Switch } from "@pidash/kit";
import { useRelationCandidates } from "./api.js";
import { RELATION_KIND_SPECS, type RelationKind } from "./relationsModel.js";

export const RELATION_EMPTY_SELECTION = "Pick at least one work item to relate.";

export interface AddRelationDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  kind: RelationKind;
  /** True while the add write runs. */
  pending: boolean;
  onSubmit: (issueIds: string[]) => void;
  /** Error surface for an empty selection. */
  onEmptySelection: () => void;
}

export function AddRelationDialog({
  open,
  onOpenChange,
  workspaceSlug,
  projectId,
  issueId,
  kind,
  pending,
  onSubmit,
  onEmptySelection,
}: AddRelationDialogProps): React.ReactElement {
  const spec = RELATION_KIND_SPECS[kind];
  const [query, setQuery] = React.useState("");
  const [workspaceSearch, setWorkspaceSearch] = React.useState(false);
  const [picked, setPicked] = React.useState<string[]>([]);
  const candidates = useRelationCandidates(workspaceSlug, projectId, issueId, query, workspaceSearch, open);

  React.useEffect(() => {
    if (!open) {
      setQuery("");
      setWorkspaceSearch(false);
      setPicked([]);
    }
  }, [open]);

  function toggle(id: string): void {
    setPicked((current) => (current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id]));
  }

  function submit(): void {
    if (picked.length === 0) {
      onEmptySelection();
      return;
    }
    onSubmit(picked);
  }

  return (
    <Dialog
      title={`Add ${spec.label.toLowerCase()}`}
      description={spec.prompt}
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" loading={pending} onClick={submit}>
            Add selected work items
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <Input
          label="Search work items"
          placeholder="Search by name or identifier"
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
          }}
        />
        <label className="flex items-center justify-between gap-(--space-3)">
          <span className="text-body text-(--text)">Search the whole workspace</span>
          <Switch
            checked={workspaceSearch}
            onCheckedChange={setWorkspaceSearch}
            aria-label="Search the whole workspace"
          />
        </label>
        <div className="max-h-72 min-h-24 overflow-y-auto rounded-(--radius-control) border border-(--border)">
          {candidates.isPending ? (
            <div className="flex items-center justify-center gap-(--space-2) p-(--space-4)">
              <Spinner size="small" />
              <span className="text-body text-(--text-muted)">Loading work items</span>
            </div>
          ) : candidates.isError ? (
            <p role="alert" className="text-body p-(--space-4) text-(--danger)">
              Couldn't load work items. Try again.
            </p>
          ) : candidates.data === undefined || candidates.data.length === 0 ? (
            <p className="text-body p-(--space-4) text-(--text-muted)">No work items match this search.</p>
          ) : (
            <ul role="listbox" aria-label="Work item candidates" className="flex flex-col p-(--space-1)">
              {candidates.data.map((row) => {
                const selected = picked.includes(row.id);
                return (
                  <li key={row.id}>
                    <button
                      type="button"
                      role="option"
                      aria-selected={selected}
                      onClick={() => {
                        toggle(row.id);
                      }}
                      className={
                        selected
                          ? "flex w-full items-center gap-(--space-3) rounded-(--radius-control) bg-(--subtle) px-(--space-3) py-(--space-2) text-left"
                          : "flex w-full items-center gap-(--space-3) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left hover:bg-(--subtle)"
                      }
                    >
                      <span className="text-meta shrink-0 text-(--text-muted)">
                        {row.project__identifier}-{row.sequence_id}
                      </span>
                      <span className="text-body min-w-0 flex-1 truncate text-(--text)">{row.name}</span>
                      <span className="text-meta shrink-0 text-(--text-muted)">{row.state__name ?? ""}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </div>
    </Dialog>
  );
}
