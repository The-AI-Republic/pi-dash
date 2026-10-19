// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Composer shell (NEWFRONT-549, RUN-032). Renders the draft box and,
// whenever chat cannot proceed, the one reason why. The send, stop and
// dictation controls arrive with the messaging slice; this shell owns the
// gate so a blocked chat never looks writable.

import { Send } from "lucide-react";
import * as React from "react";

import { Button } from "@pidash/kit";

export const CHAT_COMPOSER_PLACEHOLDER = "Message this runner…";

export interface ChatComposerProps {
  /** Blocking reason; null means chat can proceed. */
  blockedReason: string | null;
  /** The send verb lands with the messaging slice (NEWFRONT-551). */
  onSend?: (() => void) | undefined;
  sendEnabled?: boolean;
  /** The turn in flight, if any: the send control is inert while true. */
  turnInFlight?: boolean;
}

export function ChatComposer({
  blockedReason,
  onSend,
  sendEnabled = false,
  turnInFlight = false,
}: ChatComposerProps): React.ReactElement {
  const blocked = blockedReason !== null;
  return (
    <div data-testid="runner-chat-composer" className="shrink-0 border-t border-(--border) pt-(--space-3)">
      {blocked ? (
        <div data-testid="runner-chat-block-reason" className="text-meta mb-(--space-2) text-(--text-muted)">
          {blockedReason}
        </div>
      ) : null}
      <div className="flex items-end gap-(--space-2)">
        <textarea
          aria-label="Message"
          placeholder={CHAT_COMPOSER_PLACEHOLDER}
          rows={3}
          disabled={blocked || turnInFlight}
          className="text-body min-h-20 flex-1 resize-none rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
        />
        <Button
          aria-label="Send message"
          size="small"
          variant="primary"
          disabled={blocked || turnInFlight || !sendEnabled}
          onClick={onSend}
        >
          <Send size={16} strokeWidth={1.5} aria-hidden="true" />
        </Button>
      </div>
    </div>
  );
}
