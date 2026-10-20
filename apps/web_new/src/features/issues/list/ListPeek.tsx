// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Minimal list peek (parity/specs/issues.md §2, ISS-013). A title click
// opens the row beside the list without leaving the page; Escape or the
// close control clears the peek param. Full peek content (description,
// relations, activity) is NEWFRONT-646, which replaces this body.

import { IconButton } from "@pidash/kit";
import { X } from "lucide-react";
import * as React from "react";

export interface ListPeekProps {
  issueKey: string;
  title: string;
  onClose: () => void;
}

export function ListPeek({ issueKey, title, onClose }: ListPeekProps): React.ReactElement {
  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose]);

  return (
    <div
      aria-label="Work item preview"
      className="absolute top-0 right-0 bottom-0 z-40 flex w-[min(30rem,92vw)] flex-col gap-(--space-4) overflow-y-auto border-l border-(--border) bg-(--surface) p-(--space-6)"
    >
      <div className="flex items-start justify-between gap-(--space-4)">
        <p className="text-caption min-w-0 flex-1 truncate pt-(--space-1) text-(--text-muted)">{issueKey}</p>
        <IconButton label="Close preview" size="small" onClick={onClose}>
          <X size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </div>
      <textarea
        aria-label="Work item title"
        readOnly
        rows={2}
        value={title}
        className="text-h3 w-full resize-none rounded-(--radius-control) border border-transparent bg-transparent px-(--space-2) py-(--space-1) text-(--text)"
      />
    </div>
  );
}
