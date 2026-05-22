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
  after: number,
  onEvent: (event: IAgentChatEvent) => void
) {
  const onEventRef = useRef(onEvent);
  onEventRef.current = onEvent;

  useEffect(() => {
    if (!sessionId) return;
    const source = new EventSource(service.chatEventsUrl(sessionId, after), {
      withCredentials: true,
    });
    source.addEventListener("chat.event", (message) => {
      const event = JSON.parse((message as MessageEvent).data) as IAgentChatEvent;
      onEventRef.current(event);
    });
    return () => source.close();
  }, [after, sessionId]);
}
