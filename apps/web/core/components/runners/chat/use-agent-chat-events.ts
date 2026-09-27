/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef } from "react";
import { getChatTransport } from "@pi-dash/services";
import type { IAgentChatEvent } from "@pi-dash/types";

export function useAgentChatEvents(
  sessionId: string | undefined,
  onEvent: (event: IAgentChatEvent) => void,
  onError?: (error: unknown) => void,
  initialAfter = 0
) {
  const onEventRef = useRef(onEvent);
  const onErrorRef = useRef(onError);
  onEventRef.current = onEvent;
  onErrorRef.current = onError;

  useEffect(() => {
    if (!sessionId) return;
    // Route the event stream through the pluggable chat transport: the
    // cloud default opens the same SSE `EventSource` as before, while a
    // desktop build can subscribe to Tauri events instead — the hook is
    // agnostic. The refs keep the latest callbacks without re-subscribing.
    const unsubscribe = getChatTransport().subscribeChatEvents(
      sessionId,
      initialAfter,
      (event) => onEventRef.current(event),
      (error) => onErrorRef.current?.(error)
    );
    return unsubscribe;
  }, [initialAfter, sessionId]);
}
