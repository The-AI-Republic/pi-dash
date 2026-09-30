// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Real-time entry point (Data layer page). Today this is a local fan-out
// stub: the only place that may turn server events into cache patches or
// invalidations. Wiring the server push channel lands here once the open
// question on live issue updates (Q4, investigated by NEWFRONT-27) settles,
// fed by platform.stream underneath.

import type { Unsubscribe } from "../platform/types.js";

export interface RealtimeEvent {
  topic: string;
  payload: unknown;
}

export type RealtimeHandler = (event: RealtimeEvent) => void;

export interface RealtimeHub {
  publish(event: RealtimeEvent): void;
  subscribe(topic: string, handler: RealtimeHandler): Unsubscribe;
}

/** Local fan-out with no transport. Server wiring replaces the inside,
 * never the call sites. */
export function createRealtimeHub(): RealtimeHub {
  const handlers = new Map<string, Set<RealtimeHandler>>();
  return {
    publish: (event) => {
      for (const handler of handlers.get(event.topic) ?? []) {
        handler(event);
      }
    },
    subscribe: (topic, handler) => {
      let set = handlers.get(topic);
      if (!set) {
        set = new Set();
        handlers.set(topic, set);
      }
      set.add(handler);
      return () => {
        set.delete(handler);
        if (set.size === 0) handlers.delete(topic);
      };
    },
  };
}
