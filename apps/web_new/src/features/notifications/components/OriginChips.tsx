// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Applied origin chips (written from parity/specs/notifications.md §8,
// NTF-015). One chip per active origin sits above the list; its delete
// control removes that origin, and a trailing clear-all chip resets the
// whole filter. The row disappears when no origin is active.
import { X } from "lucide-react";
import * as React from "react";

import { NOTIFICATION_ORIGIN_LABELS, type NotificationOrigin, type OriginFilter } from "../lib/stream.js";

export interface OriginChipsProps {
  origins: OriginFilter;
  onRemove: (origin: NotificationOrigin) => void;
  onClearAll: () => void;
}

export function OriginChips({ origins, onRemove, onClearAll }: OriginChipsProps): React.ReactElement | null {
  if (origins.length === 0) return null;
  return (
    <div
      role="group"
      aria-label="Applied filters"
      data-testid="inbox-origin-chips"
      className="flex flex-wrap items-center gap-(--space-2) border-b border-(--border) px-(--space-3) py-(--space-2)"
    >
      {origins.map((origin) => {
        const label = NOTIFICATION_ORIGIN_LABELS[origin];
        return (
          <span
            key={origin}
            data-testid="inbox-origin-chip"
            data-origin={origin}
            className="text-caption inline-flex items-center gap-(--space-1) rounded-full border border-(--border) bg-(--subtle) py-0.5 pr-(--space-1) pl-(--space-3) text-(--text)"
          >
            <span>{label}</span>
            <button
              type="button"
              aria-label={`Remove filter ${label}`}
              data-testid={`inbox-origin-chip-remove-${origin}`}
              onClick={() => onRemove(origin)}
              className="shrink-0 cursor-pointer rounded-full p-0.5 text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
            >
              <X size={12} aria-hidden="true" />
            </button>
          </span>
        );
      })}
      <button
        type="button"
        data-testid="inbox-origin-clear-all"
        onClick={onClearAll}
        className="text-caption cursor-pointer rounded-full px-(--space-2) py-0.5 text-(--text-muted) underline-offset-2 hover:text-(--text) hover:underline"
      >
        Clear all
      </button>
    </div>
  );
}
