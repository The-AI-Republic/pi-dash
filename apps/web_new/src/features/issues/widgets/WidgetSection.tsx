// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One detail-widget disclosure (parity/specs/issues.md §15, ISS-179/184).
// The section renders only when its read has content. Its open flag is a
// per-widget browser preference, so a reload keeps the last arrangement;
// the header carries the title, a count and optionally the add control.

import { ChevronDown, ChevronRight } from "lucide-react";
import * as React from "react";

import { IconButton } from "@pidash/kit";

import { loadPreference, storePreference } from "../../../shared/shell/index.js";

/** Profile key for one widget's open flag. */
export function widgetOpenKey(widget: string): string {
  return `pidash:issue-widget-open:${widget}`;
}

/** Whether the named widget starts open; anything unreadable defaults open. */
export function loadWidgetOpen(widget: string): boolean {
  return loadPreference(widgetOpenKey(widget), true, (value): value is boolean => typeof value === "boolean");
}

export function storeWidgetOpen(widget: string, open: boolean): void {
  storePreference(widgetOpenKey(widget), open);
}

export interface WidgetSectionProps {
  /** Stable widget name ("Relations", "Links"). */
  title: string;
  count: number;
  /** Add control; omitted for read-only viewers and empty states. */
  add?: React.ReactNode | undefined;
  testId?: string | undefined;
  children: React.ReactNode;
}

export function WidgetSection({ title, count, add, testId, children }: WidgetSectionProps): React.ReactElement {
  const [open, setOpen] = React.useState(() => loadWidgetOpen(title));
  function toggle(): void {
    const next = !open;
    setOpen(next);
    storeWidgetOpen(title, next);
  }
  return (
    <section data-testid={testId ?? `widget-${title.toLowerCase().replace(/\s+/g, "-")}`} aria-label={title}>
      <div className="flex items-center gap-(--space-1)">
        <button
          type="button"
          aria-expanded={open}
          onClick={toggle}
          className="flex min-w-0 flex-1 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left hover:bg-(--subtle)"
        >
          {open ? (
            <ChevronDown size={14} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          ) : (
            <ChevronRight size={14} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          )}
          <span className="text-emphasis truncate text-(--text)">{title}</span>
          <span className="text-meta shrink-0 text-(--text-muted)">{count}</span>
        </button>
        {add === undefined ? null : <span className="shrink-0">{add}</span>}
      </div>
      {open ? <div className="grid overflow-hidden">{children}</div> : null}
    </section>
  );
}

/** Small add button used by both widgets' section headers. */
export function WidgetAddButton({
  label,
  disabled = false,
  onClick,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
}): React.ReactElement {
  return (
    <IconButton label={label} size="small" variant="ghost" disabled={disabled} onClick={onClick}>
      <span aria-hidden="true" className="text-body leading-none">
        +
      </span>
    </IconButton>
  );
}
