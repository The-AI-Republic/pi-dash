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

function deltaText(payload: Record<string, unknown>): string {
  const params = payload?.params;
  if (params && typeof params === "object" && !Array.isArray(params)) {
    const delta = (params as Record<string, unknown>).delta;
    if (typeof delta === "string") return delta;
  }
  return "";
}

const bySeq = (a: IAssistantMessage, b: IAssistantMessage): number => a.seq - b.seq;

function upsert(list: IAssistantMessage[], msg: IAssistantMessage): IAssistantMessage[] {
  const idx = list.findIndex((m) => m.id === msg.id);
  if (idx === -1) {
    // eslint-disable-next-line unicorn/no-array-sort -- fresh copy; toSorted not in tsconfig lib target
    return [...list, msg].sort(bySeq);
  }
  const next = [...list];
  next[idx] = { ...next[idx], ...msg };
  return next;
}

function applyDelta(list: IAssistantMessage[], messageId: string | null, chunk: string): IAssistantMessage[] {
  if (!messageId) return list;
  return list.map((m) => (m.id === messageId ? { ...m, content: (m.content || "") + chunk } : m));
}

export interface UseAssistantChat {
  messages: IAssistantMessage[];
  busy: boolean;
  sending: boolean;
  send: (content: string) => Promise<void>;
  stop: () => Promise<void>;
  error: string | null;
}

export function useAssistantChat(slug: string | undefined, threadId: string | undefined): UseAssistantChat {
  const [live, setLive] = useState<IAssistantMessage[]>([]);
  const [busy, setBusy] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { data: base, mutate } = useSWR<IAssistantMessage[]>(
    slug && threadId ? ["assistant-messages", slug, threadId] : null,
    () => service.listMessages(slug!, threadId!, 0)
  );

  useEffect(() => {
    setLive(base ?? []);
  }, [base]);

  // Live SSE stream (replay from 0; finished-turn deltas are pruned server-side).
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
        case "assistant_delta":
          setLive((prev) => applyDelta(prev, event.message, deltaText(payload)));
          break;
        case "message_created":
        case "message_completed":
        case "tool_call":
        case "tool_result": {
          const msg = payload.message as IAssistantMessage | undefined;
          if (msg) setLive((prev) => upsert(prev, msg));
          break;
        }
        case "turn_failed":
          setError((payload.detail as string) || (payload.error_code as string) || "The assistant failed.");
          setBusy(false);
          mutate();
          break;
        case "turn_cancelled":
          setBusy(false);
          mutate();
          break;
        case "turn_completed":
          setBusy(false);
          mutate();
          break;
        default:
          break;
      }
    });
    source.addEventListener("error", () => {
      // EventSource auto-reconnects; refetch to reconcile any missed tail.
      mutate();
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
        setLive((prev) => upsert(prev, res.message));
        setBusy(true);
      } catch (e: unknown) {
        const err = e as { error?: string; detail?: string } | null;
        setError(err?.detail || err?.error || "Unable to send message");
      } finally {
        setSending(false);
      }
    },
    [slug, threadId]
  );

  const stop = useCallback(async () => {
    if (!slug || !threadId) return;
    try {
      await service.cancel(slug, threadId);
    } catch {
      /* no-op */
    }
  }, [slug, threadId]);

  // eslint-disable-next-line unicorn/no-array-sort -- fresh copy; toSorted not in tsconfig lib target
  const messages = useMemo(() => [...live].sort(bySeq), [live]);

  return { messages, busy, sending, send, stop, error };
}
