// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Label chips (parity/specs/issues.md §§11/13, ISS-132/159): the
// read-only selected-label chips the sidebar, modal, and layouts
// render above the shared picker. Stale ids (labels deleted since the
// issue was read) render nothing.

import type { Label } from "@pidash/api-client";
import { X } from "lucide-react";
import * as React from "react";

export interface LabelChipsProps {
  /** Project labels for name/color lookup. */
  labels: readonly Label[];
  /** Selected label ids, in display order. */
  selectedIds: readonly string[];
  /** Removal handler; without it the chips render read-only. */
  onRemove?: ((labelId: string) => void) | undefined;
}

export function LabelChips({ labels, selectedIds, onRemove }: LabelChipsProps): React.ReactElement | null {
  const byId = new Map<string, Label>();
  for (const label of labels) byId.set(label.id, label);
  const selected: Label[] = [];
  for (const id of selectedIds) {
    const label = byId.get(id);
    if (label !== undefined) selected.push(label);
  }
  if (selected.length === 0) return null;
  return (
    <ul aria-label="Selected labels" className="flex flex-wrap items-center gap-(--space-2)">
      {selected.map((label) => (
        <li key={label.id}>
          <span className="text-meta inline-flex items-center gap-(--space-2) rounded-(--radius-full) bg-(--subtle) px-(--space-3) py-px text-(--text)">
            <span aria-hidden="true" style={{ backgroundColor: label.color }} className="size-1.5 rounded-full" />
            {label.name}
            {onRemove === undefined ? null : (
              <button
                type="button"
                aria-label={`Remove ${label.name}`}
                onClick={() => onRemove(label.id)}
                className="flex cursor-pointer items-center text-(--text-muted) hover:text-(--text)"
              >
                <X size={12} strokeWidth={2} aria-hidden="true" />
              </button>
            )}
          </span>
        </li>
      ))}
    </ul>
  );
}
