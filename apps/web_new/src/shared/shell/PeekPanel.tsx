// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared work-item peek panel (SHELL-016; written from parity/specs/shell.md
// §10). The shell owns this panel shell — a side panel in the fullscreen
// portal with the title rendered as an editable field — and the issues epic
// reuses it for full peek with its own data and commit. Body content arrives
// as children, so this layer never reaches into feature data.

import * as React from "react";
import { X } from "lucide-react";

import { IconButton } from "@pidash/kit";
import { FULL_SCREEN_PORTAL_ID, OverlayPortal } from "./document.js";

export interface PeekPanelProps {
  /** Context line above the title (e.g. the work-item key). */
  eyebrow?: React.ReactNode;
  /** Current title; the field commits through onCommitTitle. */
  title: string;
  /** Accessible name for the title field. */
  titleLabel: string;
  onClose: () => void;
  /** Commits a renamed title; without it edits stay local to the field. */
  onCommitTitle?: ((next: string) => void) | undefined;
  /** Controls beside the close button (e.g. copy link). */
  actions?: React.ReactNode;
  children?: React.ReactNode;
}

export function PeekPanel({
  eyebrow,
  title,
  titleLabel,
  onClose,
  onCommitTitle,
  actions,
  children,
}: PeekPanelProps): React.ReactNode {
  const [draft, setDraft] = React.useState(title);
  React.useEffect(() => {
    setDraft(title);
  }, [title]);
  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose]);

  const commit = (): void => {
    const next = draft.trim();
    if (next === "" || next === title) {
      setDraft(title);
      return;
    }
    onCommitTitle?.(next);
  };

  return (
    <OverlayPortal rootId={FULL_SCREEN_PORTAL_ID}>
      <aside
        aria-label="Work item preview"
        data-testid="issue-preview"
        className="fixed top-0 right-0 bottom-0 z-40 flex w-[min(30rem,92vw)] flex-col gap-(--space-4) overflow-y-auto border-l border-(--border) bg-(--surface) p-(--space-6)"
      >
        <div className="flex items-start justify-between gap-(--space-4)">
          <p className="text-caption min-w-0 flex-1 truncate pt-(--space-1) text-(--text-muted)">{eyebrow}</p>
          <div className="flex shrink-0 items-center gap-(--space-1)">
            {actions}
            <IconButton label="Close preview" size="small" onClick={onClose}>
              <X size={16} strokeWidth={1.5} aria-hidden="true" />
            </IconButton>
          </div>
        </div>
        <input
          aria-label={titleLabel}
          value={draft}
          onChange={(event) => {
            setDraft(event.target.value);
          }}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") commit();
          }}
          className="text-h3 w-full rounded-(--radius-control) border border-transparent bg-transparent px-(--space-2) py-(--space-1) text-(--text) hover:border-(--border) focus:border-(--accent) focus:outline-none"
        />
        {children}
      </aside>
    </OverlayPortal>
  );
}
