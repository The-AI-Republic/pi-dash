// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sticky section quick-add (parity/specs/issues.md §2, ISS-011). One
// title field prefilled with the section's grouping value and the
// project's default state. Enter saves and keeps the form open for the
// next title, Escape or an outside click closes it, and a promise toast
// reports the outcome. Guests, completed cycles and creator-grouped
// sections get no form.

import { Plus } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useCreateWorkItem } from "../api/create.js";
import { attachCreatedIssue } from "./mutations.js";

export interface QuickAddProps {
  workspaceSlug: string;
  projectId: string;
  /** Cycle/module target appended after create. */
  cycleId?: string | undefined;
  moduleId?: string | undefined;
  /** Group value fields merged into the create payload. */
  prefill: Record<string, string | string[]>;
  /** Grouped section bucket key for the optimistic row. */
  groupKey?: string | undefined;
  /** Default state id (project default) when the section sets no state. */
  defaultStateId: string | null;
  disabled: boolean;
  /** Disables creation with a visible reason (workflow rules). */
  disabledReason?: string | undefined;
  onCreated?: ((issueId: string) => void) | undefined;
}

export function QuickAdd({
  workspaceSlug,
  projectId,
  cycleId,
  moduleId,
  prefill,
  groupKey,
  defaultStateId,
  disabled,
  disabledReason,
  onCreated,
}: QuickAddProps): React.ReactElement {
  const create = useCreateWorkItem(workspaceSlug);
  const [open, setOpen] = React.useState(false);
  const [title, setTitle] = React.useState("");
  const containerRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent): void => {
      if (containerRef.current !== null && !containerRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
    };
  }, [open]);

  if (disabled) {
    return (
      <div
        data-testid="quick-add-disabled"
        className="text-caption sticky bottom-0 flex min-h-9 items-center gap-(--space-2) border-t border-(--border) px-(--space-3) text-(--text-muted)"
        title={disabledReason}
      >
        <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
        <span>{disabledReason ?? "New work item"}</span>
      </div>
    );
  }

  function submit(): void {
    const name = title.trim();
    if (name === "") return;
    // A cycle/module grouping value is a membership, not a create field:
    // the create endpoint ignores it, so it goes through the bridge after
    // the row lands. Route context wins over the section value.
    const groupedCycle = typeof prefill["cycle_id"] === "string" ? prefill["cycle_id"] : undefined;
    const groupedModules = Array.isArray(prefill["module_ids"]) ? prefill["module_ids"] : undefined;
    const rest = { ...prefill };
    delete rest["cycle_id"];
    delete rest["module_ids"];
    const fields = {
      ...rest,
      ...(rest["state_id"] === undefined && defaultStateId !== null ? { state_id: defaultStateId } : {}),
    };
    const targetCycle = cycleId ?? groupedCycle;
    const targetModule = moduleId ?? groupedModules?.[0];
    create.mutate(
      { projectId, payload: { ...fields, name }, ...(groupKey === undefined ? {} : { tempGroupKey: groupKey }) },
      {
        onSuccess: (created) => {
          setTitle("");
          void attachCreatedIssue(getAppClient(), workspaceSlug, projectId, created.id, {
            ...(targetCycle === undefined ? {} : { cycleId: targetCycle }),
            ...(targetModule === undefined ? {} : { moduleId: targetModule }),
          }).catch(() => undefined);
          onCreated?.(created.id);
        },
      }
    );
  }

  return (
    <div ref={containerRef} data-testid="quick-add" className="sticky bottom-0 border-t border-(--border) bg-(--bg)">
      {open ? (
        <div className="flex min-h-9 items-center gap-(--space-2) px-(--space-3)">
          <Plus size={14} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" aria-hidden="true" />
          <input
            autoFocus
            aria-label="New work item title"
            placeholder="Work item title"
            value={title}
            maxLength={255}
            onChange={(event) => setTitle(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                submit();
              }
              if (event.key === "Escape") {
                event.preventDefault();
                setOpen(false);
                setTitle("");
              }
            }}
            className="text-body min-w-0 flex-1 bg-transparent py-(--space-2) text-(--text) outline-none placeholder:text-(--text-muted)"
          />
        </div>
      ) : (
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="text-caption flex min-h-9 w-full items-center gap-(--space-2) px-(--space-3) text-left text-(--text-muted) hover:text-(--text)"
        >
          <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
          <span>New work item</span>
        </button>
      )}
    </div>
  );
}
