/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef } from "react";
import { RunnerService } from "@pi-dash/services";
import type { IAgentChatEvent } from "@pi-dash/types";

const service = new RunnerService();

export function useAgentChatEvents(
  sessionId: string | undefined,
  onEvent: (event: IAgentChatEvent) => void,
  onError?: (error: unknown) => void,
  initialAfter = 0
) {
  const onEventRef = useRef(onEvent);
  const onErrorRef = useRef(onError);
  const lastSeqRef = useRef(initialAfter);
  onEventRef.current = onEvent;
  onErrorRef.current = onError;

  useEffect(() => {
    if (!sessionId) return;
    lastSeqRef.current = initialAfter;
    const source = new EventSource(service.chatEventsUrl(sessionId, initialAfter), {
      withCredentials: true,
    });
    source.addEventListener("chat.event", (message) => {
      try {
        const event = JSON.parse((message as MessageEvent).data) as IAgentChatEvent;
        lastSeqRef.current = Math.max(lastSeqRef.current, event.seq);
        onEventRef.current(event);
      } catch (error) {
        console.error("Failed to parse runner chat event", error);
        onErrorRef.current?.(error);
      }
    });
    source.addEventListener("error", (error) => {
      onErrorRef.current?.(error);
    });
    return () => source.close();
  }, [initialAfter, sessionId]);
}
