// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board quick-add (parity/specs/issues.md §5, ISS-035). Each column ends
// with a sticky entry that opens one inline title field; the submit
// creates through the shared create seam with the column's (and lane's)
// grouping value plus the project's default state, then keeps the field
// open for the next title. Enter submits, Escape closes. The entry is
// absent without project create standing, in a completed cycle, or when
// an edition workflow rule blocks creation.

import { cn } from "@pidash/kit";
import { Plus } from "lucide-react";
import * as React from "react";

export interface KanbanQuickAddProps {
  /** Project identifier prefix the form shows beside the title field. */
  projectIdentifier: string;
  disabled: boolean;
  /** Tooltip when creation is unavailable; the entry then renders nothing. */
  disabledReason?: string | undefined;
  /** Creates one work item; resolves when the write settles. */
  onCreate: (title: string) => Promise<unknown> | void;
}

export function KanbanQuickAdd({
  projectIdentifier,
  disabled,
  disabledReason,
  onCreate,
}: KanbanQuickAddProps): React.ReactElement | null {
  const [open, setOpen] = React.useState(false);
  const [title, setTitle] = React.useState("");
  const [saving, setSaving] = React.useState(false);
  const field = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    if (open) field.current?.focus();
  }, [open]);

  if (disabled) return null;

  const submit = async (): Promise<void> => {
    const name = title.trim();
    if (name === "" || saving) return;
    setSaving(true);
    try {
      await onCreate(name);
      setTitle("");
    } catch {
      // The create seam toasts the failure; the typed title stays.
    } finally {
      setSaving(false);
    }
  };

  return (
    <div data-testid="kanban-quick-add" className="sticky bottom-0 z-10 border-t border-(--border) bg-(--surface)">
      {open ? (
        <form
          data-testid="kanban-quick-add-form"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
          className="flex items-center gap-(--space-2) px-(--space-3) py-(--space-2)"
        >
          <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
            {projectIdentifier}
          </span>
          <input
            ref={field}
            aria-label="Work item title"
            placeholder="Work item title"
            value={title}
            maxLength={255}
            onChange={(event) => setTitle(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                setTitle("");
                setOpen(false);
              }
            }}
            className="text-body min-w-0 flex-1 bg-transparent text-(--text) outline-none placeholder:text-(--text-muted)"
          />
        </form>
      ) : (
        <button
          type="button"
          onClick={() => setOpen(true)}
          title={disabledReason}
          className={cn(
            "text-caption flex min-h-9 w-full items-center gap-(--space-2) px-(--space-3) text-left text-(--text-muted) hover:text-(--text)"
          )}
        >
          <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
          <span>New work item</span>
        </button>
      )}
    </div>
  );
}
