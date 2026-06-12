/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Send, Square } from "lucide-react";
import { Button } from "@pi-dash/ui";

interface ChatComposerProps {
  draft: string;
  onDraftChange: (value: string) => void;
  onSend: () => void;
  onStop?: () => void;
  busy?: boolean;
  sending?: boolean;
  disabledReason?: string | null;
  placeholder?: string;
  className?: string;
}

export function ChatComposer({
  draft,
  onDraftChange,
  onSend,
  onStop,
  busy = false,
  sending = false,
  disabledReason = null,
  placeholder = "Ask Pi to do something…",
  className = "shrink-0 border-t border-subtle pt-3",
}: ChatComposerProps) {
  const disabled = !!disabledReason;
  return (
    <div className={className}>
      {disabledReason && <div className="mb-2 text-12 text-secondary">{disabledReason}</div>}
      <div className="flex items-end gap-2">
        <textarea
          value={draft}
          onChange={(e) => onDraftChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              if (!disabled && draft.trim()) onSend();
            }
          }}
          disabled={disabled || sending}
          placeholder={placeholder}
          className="min-h-20 flex-1 resize-none rounded-md border border-subtle bg-surface-1 px-3 py-2 text-13 outline-none focus:border-accent-strong"
        />
        {busy && onStop ? (
          <Button onClick={onStop} variant="tertiary-danger">
            <Square className="size-4" />
          </Button>
        ) : (
          <Button onClick={onSend} disabled={disabled || !draft.trim()} loading={sending}>
            <Send className="size-4" />
          </Button>
        )}
      </div>
    </div>
  );
}
