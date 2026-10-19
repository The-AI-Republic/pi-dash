// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Composer shell (parity/specs/agents.md §9, AGT-038/AGT-040). One
// multiline draft box with a send action; the thread slice adds the
// streaming stop state and the voice cell. Without a provider key the
// whole input locks and the reminder line explains why, so a submit can
// never reach the server. Submits go through the form, which makes the
// send button's `type="submit"` and an unmodified Enter the same path.

import { Button, cn } from "@pidash/kit";
import { Send } from "lucide-react";
import * as React from "react";

import {
  API_KEY_REMINDER,
  ASSISTANT_COMPOSER_LABEL,
  ASSISTANT_PLACEHOLDER,
  ASSISTANT_SEND_LABEL,
} from "../lib/copy.js";

export interface ComposerProps {
  draft: string;
  onDraftChange: (value: string) => void;
  onSubmit: (draft: string) => void;
  /** A start is in flight: the box locks so a second submit cannot fire. */
  starting?: boolean;
  /** False only when no provider key exists. */
  configured?: boolean;
  placeholder?: string;
  /** Standalone composers draw no top rule; docked ones do. */
  bordered?: boolean;
  className?: string;
}

export function Composer({
  draft,
  onDraftChange,
  onSubmit,
  starting = false,
  configured = true,
  placeholder = ASSISTANT_PLACEHOLDER,
  bordered = true,
  className,
}: ComposerProps): React.ReactElement {
  const locked = !configured || starting;
  const canSend = configured && !starting && draft.trim() !== "";

  const submit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (!canSend) return;
    onSubmit(draft.trim());
  };

  return (
    <form
      onSubmit={submit}
      data-testid="assistant-composer"
      data-locked={String(!configured)}
      className={cn(
        "flex shrink-0 flex-col gap-(--space-2)",
        bordered && "border-t border-(--border) pt-(--space-3)",
        className
      )}
    >
      {configured ? null : (
        <p data-testid="assistant-composer-reason" className="text-meta text-(--text-muted)">
          {API_KEY_REMINDER}
        </p>
      )}
      <div className="flex items-end gap-(--space-2)">
        <textarea
          aria-label={ASSISTANT_COMPOSER_LABEL}
          placeholder={placeholder}
          value={draft}
          rows={3}
          disabled={locked}
          onChange={(event) => onDraftChange(event.target.value)}
          onKeyDown={(event) => {
            // Unmodified Enter submits; Shift+Enter keeps the newline the
            // browser inserts natively.
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              if (canSend) onSubmit(draft.trim());
            }
          }}
          className="text-body min-h-20 flex-1 resize-none rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-3) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none disabled:cursor-not-allowed disabled:opacity-60"
        />
        <Button
          type="submit"
          variant="primary"
          disabled={!canSend}
          loading={starting}
          aria-label={ASSISTANT_SEND_LABEL}
        >
          <Send aria-hidden="true" size={16} />
        </Button>
      </div>
    </form>
  );
}
