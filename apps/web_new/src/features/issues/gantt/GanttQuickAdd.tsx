// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline quick-add (parity/specs/issues.md §6, ISS-055). A sticky entry
// at the chart's bottom opens one inline title field; submitting posts
// through the shared creation seam with today and tomorrow prefilled, so
// the new row arrives already dated and draws a bar. The entry is absent
// without create standing and in a completed cycle.

import { Button } from "@pidash/kit";
import { Plus } from "lucide-react";
import * as React from "react";

export interface GanttQuickAddProps {
  /** Creates a dated issue; resolves when the submit settles. */
  onCreate: (title: string) => Promise<unknown> | void;
  /** False without project create standing (ISS-055). */
  enabled: boolean;
  /** Completed cycles suppress the entry (ISS-055). */
  completedCycle?: boolean;
  /** Completed modules suppress the entry (MOD-030). */
  completedModule?: boolean;
}

export function GanttQuickAdd({
  onCreate,
  enabled,
  completedCycle = false,
  completedModule = false,
}: GanttQuickAddProps): React.ReactElement | null {
  const [open, setOpen] = React.useState(false);
  const [title, setTitle] = React.useState("");
  const [saving, setSaving] = React.useState(false);
  const field = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    if (open) field.current?.focus();
  }, [open]);

  if (!enabled || completedCycle || completedModule) return null;

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    const name = title.trim();
    if (name === "" || saving) return;
    setSaving(true);
    try {
      await onCreate(name);
      setTitle("");
      setOpen(false);
    } catch {
      // The create seam toasts its own failure; the field stays open so
      // the title is not lost (ISS-055).
    } finally {
      setSaving(false);
    }
  };

  if (!open) {
    return (
      <div data-testid="gantt-quick-add" className="sticky bottom-0 z-10 border-t border-(--border) bg-(--surface)">
        <Button variant="ghost" className="w-full justify-start" onClick={() => setOpen(true)}>
          <Plus size={16} strokeWidth={2} aria-hidden="true" />
          New work item
        </Button>
      </div>
    );
  }

  return (
    <form
      data-testid="gantt-quick-add-form"
      onSubmit={(event) => void submit(event)}
      className="sticky bottom-0 z-10 flex items-center gap-(--space-2) border-t border-(--border) bg-(--surface) px-(--space-3) py-(--space-2)"
    >
      <input
        ref={field}
        aria-label="Work item title"
        placeholder="Work item title"
        value={title}
        onChange={(event) => setTitle(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            setTitle("");
            setOpen(false);
          }
        }}
        className="text-body min-w-0 flex-1 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1)"
      />
      <Button type="submit" variant="primary" disabled={saving || title.trim() === ""}>
        Add
      </Button>
    </form>
  );
}
