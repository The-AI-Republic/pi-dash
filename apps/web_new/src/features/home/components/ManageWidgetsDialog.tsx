// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Manage-widgets dialog (SHELL-018–019; written from parity/specs/shell.md
// §2). Each surfaced widget gets a visibility toggle plus a drag handle:
// toggles apply instantly and silently, drops persist the dragged row's
// position with a confirmation notice, and failures roll back with a
// failure notice. The retired key never appears here. Dragging is
// pointer-based (press the handle, move, release), so it works with mouse
// and touch alike; the drop slot follows the pointer past each row's
// middle.

import { GripVertical } from "lucide-react";
import * as React from "react";

import type { HomeWidgetKey } from "@pidash/api-client";
import { Dialog, Skeleton } from "@pidash/kit";

import { useWidgetPrefs } from "../api/widgetPrefs.js";
import { useReorderWidget, useToggleWidget } from "../api/widgetUpdates.js";
import { planWidgetReorder, surfacedWidgetPrefs, widgetDisplayName } from "../lib/manageWidgets.js";

export interface ManageWidgetsDialogProps {
  workspaceSlug: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function ManageWidgetsDialog({
  workspaceSlug,
  open,
  onOpenChange,
}: ManageWidgetsDialogProps): React.ReactElement {
  const prefs = useWidgetPrefs(workspaceSlug);
  const { mutate: flipToggle } = useToggleWidget(workspaceSlug);
  const { mutate: moveWidget } = useReorderWidget(workspaceSlug);
  const rows = React.useMemo(() => surfacedWidgetPrefs(prefs.data ?? []), [prefs.data]);
  const [dragKey, setDragKey] = React.useState<HomeWidgetKey | null>(null);
  const [liveOrder, setLiveOrder] = React.useState<HomeWidgetKey[] | null>(null);
  const prefData = prefs.data;

  const position = React.useMemo(
    () => new Map((liveOrder ?? []).map((key, index) => [key, index] as const)),
    [liveOrder]
  );
  const visible = React.useMemo(
    () => [...rows].sort((a, b) => (position.get(a.key) ?? 0) - (position.get(b.key) ?? 0)),
    [rows, position]
  );

  React.useEffect(() => {
    if (dragKey === null) return;
    const finish = (): void => {
      const order = liveOrder;
      const key = dragKey;
      const data = prefData;
      setDragKey(null);
      setLiveOrder(null);
      if (order === null) return;
      const plan = planWidgetReorder(data ?? [], key, order.indexOf(key));
      if (plan === null) return;
      moveWidget(plan);
    };
    window.addEventListener("mouseup", finish);
    return () => {
      window.removeEventListener("mouseup", finish);
    };
  }, [dragKey, liveOrder, prefData, moveWidget]);

  function moveDrag(overKey: HomeWidgetKey, clientY: number, row: HTMLElement): void {
    const dragging = dragKey;
    if (dragging === null || overKey === dragging) return;
    const rect = row.getBoundingClientRect();
    const after = clientY >= rect.top + rect.height / 2;
    setLiveOrder((current) => {
      const base = current ?? rows.map((entry) => entry.key);
      const next = base.filter((key) => key !== dragging);
      next.splice(next.indexOf(overKey) + (after ? 1 : 0), 0, dragging);
      return next;
    });
  }

  return (
    <Dialog
      title="Manage widgets"
      description="Show, hide, and reorder your dashboard widgets."
      open={open}
      onOpenChange={onOpenChange}
    >
      {prefs.isPending ? (
        <div data-testid="manage-widgets-skeleton" aria-busy="true" className="flex flex-col gap-(--space-2)">
          <Skeleton height="44px" />
          <Skeleton height="44px" />
        </div>
      ) : rows.length > 0 ? (
        <div role="list" aria-label="Widgets" className="flex flex-col">
          {visible.map((pref) => {
            const name = widgetDisplayName(pref.key);
            return (
              <div
                key={pref.key}
                role="listitem"
                data-widget-row={pref.key}
                aria-current={dragKey === pref.key ? true : undefined}
                onMouseMove={(event) => {
                  moveDrag(pref.key, event.clientY, event.currentTarget);
                }}
                className="flex items-center justify-between gap-(--space-3) border-b border-(--border) py-(--space-3) select-none last:border-b-0"
              >
                <div className="flex min-w-0 items-center gap-(--space-2)">
                  <button
                    type="button"
                    aria-label={`Reorder ${name}`}
                    onMouseDown={(event) => {
                      event.preventDefault();
                      setDragKey(pref.key);
                      setLiveOrder(null);
                    }}
                    className="cursor-grab rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text) active:cursor-grabbing"
                  >
                    <GripVertical size={16} strokeWidth={1.5} aria-hidden="true" />
                  </button>
                  <span className="text-body truncate text-(--text)">{name}</span>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={pref.is_enabled}
                  aria-label={name}
                  onClick={() => {
                    flipToggle({ key: pref.key, is_enabled: !pref.is_enabled });
                  }}
                  className="relative h-6 w-11 shrink-0 cursor-pointer rounded-(--radius-full) bg-(--border) transition-colors duration-(--motion-duration) data-[on=true]:bg-(--accent)"
                  data-on={pref.is_enabled}
                >
                  <span
                    aria-hidden="true"
                    className="shadow absolute top-0.5 left-0.5 size-5 rounded-full bg-(--bg) transition-transform duration-(--motion-duration) data-[on=true]:translate-x-5"
                    data-on={pref.is_enabled}
                  />
                </button>
              </div>
            );
          })}
        </div>
      ) : null}
    </Dialog>
  );
}
