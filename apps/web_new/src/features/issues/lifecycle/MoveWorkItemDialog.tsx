// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Move-to-project (parity/specs/issues.md §11, ISS-136). Lists the
// projects the viewer belongs to except the current one, filters them by
// name or identifier as the user types, and posts the pick. A landed
// move toasts and sends the browser to the record's new address, because
// its project identifier and sequence changed. With no other project the
// list reads an empty note instead. The target list comes from the
// caller so the dialog stays usable where projects are already loaded.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useMoveWorkItem, type WorkItemScope } from "./api.js";
import {
  MOVE_EMPTY_NOTE,
  MOVE_FAILED_BODY,
  MOVE_FAILED_TITLE,
  MOVE_SEARCH_PLACEHOLDER,
  MOVE_SUCCESS_BODY,
  MOVE_SUCCESS_TITLE,
  failureDetail,
  filterMoveTargets,
  moveTargets,
  movedWorkItemPath,
  type MovableProject,
} from "./rules.js";

export interface MoveWorkItemDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  scope: WorkItemScope;
  /** Every project the viewer belongs to; the current one is filtered out. */
  projects: readonly MovableProject[];
  /** Identifier lookup for the target project after the move. */
  identifierOf: (projectId: string) => string | undefined;
  /** Where to go after a landed move; defaults to a full navigation. */
  navigate?: ((href: string) => void) | undefined;
}

export function MoveWorkItemDialog({
  open,
  onOpenChange,
  scope,
  projects,
  identifierOf,
  navigate,
}: MoveWorkItemDialogProps): React.ReactElement {
  const move = useMoveWorkItem(scope);
  const [query, setQuery] = React.useState("");
  const targets = React.useMemo(() => moveTargets(projects, scope.projectId), [projects, scope.projectId]);
  const shown = React.useMemo(() => filterMoveTargets(targets, query), [targets, query]);

  React.useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  function pick(projectId: string): void {
    move.mutate(projectId, {
      onSuccess: (moved) => {
        notifySuccess(MOVE_SUCCESS_TITLE, MOVE_SUCCESS_BODY);
        onOpenChange(false);
        const identifier = identifierOf(projectId) ?? "";
        const href = movedWorkItemPath(scope.workspaceSlug, identifier, moved.sequence_id);
        if (navigate !== undefined) navigate(href);
        else globalThis.location.assign(href);
      },
      onError: (error) => {
        notifyError(MOVE_FAILED_TITLE, failureDetail(error, MOVE_FAILED_BODY));
      },
    });
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => onOpenChange(next)}
      title="Move to project"
      description="Pick the project that should own this work item from now on. Its identifier changes with the move."
      className="max-w-lg"
      actions={<Button onClick={() => onOpenChange(false)}>Cancel</Button>}
    >
      <div className="flex flex-col gap-(--space-3)">
        <Input
          label="Find a project"
          type="search"
          placeholder={MOVE_SEARCH_PLACEHOLDER}
          autoFocus
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        {targets.length === 0 ? (
          <p data-testid="move-empty" className="text-body text-(--text-muted)">
            {MOVE_EMPTY_NOTE}
          </p>
        ) : shown.length === 0 ? (
          <p data-testid="move-empty" className="text-body text-(--text-muted)">
            {MOVE_EMPTY_NOTE}
          </p>
        ) : (
          <ul aria-label="Projects" className="flex max-h-72 flex-col gap-px overflow-y-auto">
            {shown.map((project) => (
              <li key={project.id}>
                <button
                  type="button"
                  data-testid="move-target"
                  disabled={move.isPending}
                  onClick={() => pick(project.id)}
                  className="text-body flex w-full cursor-pointer items-center justify-between gap-(--space-3) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-60"
                >
                  <span className="min-w-0 flex-1 truncate">{project.name}</span>
                  <span className="text-caption shrink-0 text-(--text-muted)">{project.identifier}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Dialog>
  );
}
