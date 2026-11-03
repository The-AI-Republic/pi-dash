// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Context chip (shell spec, SHELL-090/091). Names the palette's entity
// subject (indicator plus route key or entity name) above the filter
// input; clearing it dismisses the contextual group. Backspace on an
// empty root query dismisses the same way.

import { X } from "lucide-react";
import * as React from "react";

export interface ContextChipProps {
  /** Short entity label ("Work item"). */
  indicator: string;
  /** Subject name (route key or entity name). */
  subject: string;
  /** Hide the chip and its contextual group. */
  onClear: () => void;
}

export function ContextChip({ indicator, subject, onClear }: ContextChipProps): React.ReactElement {
  return (
    <div className="flex items-center gap-(--space-2) px-(--space-2) pb-(--space-2)">
      <span className="text-caption inline-flex min-w-0 items-center gap-(--space-2) rounded-full border border-(--border) bg-(--subtle) py-0.5 pr-(--space-1) pl-(--space-3) text-(--text-muted)">
        <span className="truncate">
          {indicator} <span aria-hidden="true">·</span> <span className="text-(--text)">{subject}</span>
        </span>
        <button
          type="button"
          title="Clear context (Backspace)"
          aria-label="Clear context (Backspace)"
          tabIndex={-1}
          onClick={onClear}
          className="shrink-0 rounded-full p-0.5 text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <X aria-hidden size={12} />
        </button>
      </span>
    </div>
  );
}
