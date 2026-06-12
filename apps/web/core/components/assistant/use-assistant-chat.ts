/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import useSWR from "swr";
import { AssistantService } from "@pi-dash/services";
import type { IAssistantEvent, IAssistantMessage } from "@pi-dash/types";

const service = new AssistantService();

const bySeq = (a: IAssistantMessage, b: IAssistantMessage): number => a.seq - b.seq;

function deltaText(payload: Record<string, unknown>): string {
  const params = payload?.params;
  if (params && typeof params === "object" && !Array.isArray(params)) {
    const delta = (params as Record<string, unknown>).delta;
    if (typeof delta === "string") return delta;
  }
  return "";
}

type ById = Record<string, IAssistantMessage>;

export interface UseAssistantChat {
  messages: IAssistantMessage[];
  busy: boolean;
  sending: boolean;
  send: (content: string) => Promise<void>;
  stop: () => Promise<void>;
  error: string | null;
}

/**
 * Live transcript for one thread.
 *
 * Updates come from two sources that reinforce each other so the UI renders
 * live like a web chat regardless of SSE reliability:
 *   1. SSE stream — smooth token-level deltas + tool/lifecycle events.
 *   2. SWR polling while a turn is active — guarantees the response appears
 *      within ~1s even if the SSE stream is buffered/blocked.
 * The merge prefers in-flight streamed content so polling never clobbers a
 * partially-streamed message.
 */
export function useAssistantChat(slug: string | undefined, threadId: string | undefined): UseAssistantChat {
  const [byId, setById] = useState<ById>({});
  const [busy, setBusy] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { data: base, mutate } = useSWR<IAssistantMessage[]>(
    slug && threadId ? ["assistant-messages", slug, threadId] : null,
    () => service.listMessages(slug!, threadId!, 0),
    // Poll while a turn is running so messages render without depending on SSE.
    { refreshInterval: busy ? 1000 : 0 }
  );

  // Reset transcript when the thread changes.
  useEffect(() => {
    setById({});
    setBusy(false);
    setError(null);
  }, [slug, threadId]);

  // Merge the durable transcript (SWR fetch/poll) into local state without
  // clobbering content that the SSE stream may be ahead on.
  useEffect(() => {
    if (!base) return;
    setById((prev) => {
      const next: ById = { ...prev };
      for (const m of base) {
        const cur = next[m.id];
        const take = !cur || m.status === "completed" || (m.content?.length ?? 0) >= (cur.content?.length ?? 0);
        if (take) next[m.id] = { ...cur, ...m };
      }
      return next;
    });
  }, [base]);

  // Fallback turn-completion detection from polled state (covers a missed SSE
  // turn_completed): no streaming row remains and the latest item is a reply.
  useEffect(() => {
    if (!busy) return;
    const list = Object.values(byId);
    if (list.length === 0) return;
    if (list.some((m) => m.status === "streaming")) return;
    let last: IAssistantMessage | undefined;
    for (const m of list) if (!last || m.seq > last.seq) last = m;
    if (last && last.role !== "user") setBusy(false);
  }, [byId, busy]);

  // SSE stream — smooth deltas + immediate lifecycle.
  useEffect(() => {
    if (!slug || !threadId) return;
    const source = new EventSource(service.eventsUrl(slug, threadId, 0), { withCredentials: true });
    source.addEventListener("chat.event", (raw) => {
      let event: IAssistantEvent;
      try {
        event = JSON.parse((raw as MessageEvent).data) as IAssistantEvent;
      } catch {
        return;
      }
      const payload = event.payload || {};
      switch (event.kind) {
        case "turn_started":
          setBusy(true);
          break;
        case "assistant_delta": {
          const id = event.message;
          const chunk = deltaText(payload);
          if (id && chunk) {
            setById((prev) =>
              prev[id] ? { ...prev, [id]: { ...prev[id], content: (prev[id].content || "") + chunk } } : prev
            );
          }
          break;
        }
        case "message_created":
        case "message_completed":
        case "tool_call":
        case "tool_result": {
          const m = payload.message as IAssistantMessage | undefined;
          if (m) setById((prev) => ({ ...prev, [m.id]: { ...prev[m.id], ...m } }));
          break;
        }
        case "turn_failed":
          setError((payload.detail as string) || (payload.error_code as string) || "The assistant failed.");
          setBusy(false);
          mutate();
          break;
        case "turn_cancelled":
        case "turn_completed":
          setBusy(false);
          mutate();
          break;
        default:
          break;
      }
    });
    return () => source.close();
  }, [slug, threadId, mutate]);

  const send = useCallback(
    async (content: string) => {
      if (!slug || !threadId || !content.trim()) return;
      setSending(true);
      setError(null);
      try {
        const res = await service.sendMessage(slug, threadId, content.trim());
        setById((prev) => ({ ...prev, [res.message.id]: res.message }));
        setBusy(true); // starts polling + shows the stop button immediately
        mutate();
      } catch (e: unknown) {
        const err = e as { error?: string; detail?: string } | null;
        setError(err?.detail || err?.error || "Unable to send message");
      } finally {
        setSending(false);
      }
    },
    [slug, threadId, mutate]
  );

  const stop = useCallback(async () => {
    if (!slug || !threadId) return;
    try {
      await service.cancel(slug, threadId);
    } catch {
      /* no-op */
    }
    setBusy(false);
  }, [slug, threadId]);

  const messages = useMemo(() => {
    // eslint-disable-next-line unicorn/no-array-sort -- fresh copy; toSorted not in tsconfig lib target
    return Object.values(byId).sort(bySeq);
  }, [byId]);

  return { messages, busy, sending, send, stop, error };
}
