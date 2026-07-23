/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { observer } from "mobx-react";
import { X } from "lucide-react";
import { useParams, useSearchParams } from "react-router";
import useSWR from "swr";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { RunnerService, getRunnerDetail } from "@pi-dash/services";
import type { IAgentChatEvent, IAgentChatMessage, IAgentChatSession, IRunner } from "@pi-dash/types";
import { Badge, Button } from "@pi-dash/ui";
import { calculateTimeAgo, renderFormattedDate } from "@pi-dash/utils";
import { ChatComposer } from "@/components/chat/composer";
import { ChatContainer } from "@/components/chat/container";
import { type ChatHistoryItem, ChatHistoryPanel } from "@/components/chat/history-panel";
import { ChatMessage } from "@/components/chat/message";
import { useAgentChatEvents } from "@/components/runners/chat/use-agent-chat-events";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();

function assistantDeltaText(payload: Record<string, unknown>): string {
  const params = payload.params;
  if (!params || typeof params !== "object" || Array.isArray(params)) return "";
  const paramsObject = params as Record<string, unknown>;
  const delta = paramsObject.delta;
  if (typeof delta === "string") return delta;
  if (delta && typeof delta === "object" && !Array.isArray(delta)) {
    const text = (delta as Record<string, unknown>).text;
    if (typeof text === "string") return text;
  }
  const text = paramsObject.text;
  return typeof text === "string" ? text : "";
}

function isRealtimeEvent(event: IAgentChatEvent, streamStartedAt: number): boolean {
  const eventTime = Date.parse(event.created_at);
  return Number.isFinite(eventTime) && eventTime >= streamStartedAt - 500;
}

function appendAssistantDelta(
  messages: IAgentChatMessage[],
  event: IAgentChatEvent,
  delta: string,
  sessionId: string | undefined
): IAgentChatMessage[] {
  const messageId = event.message;
  if (!messageId) return messages;
  let found = false;
  const next = messages.map((message) => {
    if (message.id !== messageId) return message;
    found = true;
    if (message.status !== "streaming") return message;
    return { ...message, content: `${message.content || ""}${delta}` };
  });
  if (found) return next;

  const seq = messages.reduce((max, message) => Math.max(max, message.seq), 0) + 1;
  return [
    ...messages,
    {
      id: messageId,
      session: sessionId ?? event.session,
      role: "assistant",
      content: delta,
      content_parts: [],
      status: "streaming",
      local_item_id: "",
      local_turn_id: typeof event.payload.turn_id === "string" ? event.payload.turn_id : "",
      seq,
      created_at: event.created_at,
      completed_at: null,
    },
  ];
}

function disabledReason(runner?: IRunner, session?: IAgentChatSession | null): string | null {
  if (!runner) return "Loading";
  if (runner.status === "offline") return "Runner offline";
  if (runner.status === "revoked") return "Runner revoked";
  // "busy" no longer blocks chat: the runner serves chat concurrently with an
  // issue run in a dedicated worktree, and "busy" is also reported while a chat
  // turn is in flight. The mid-turn case is covered by the active_message check
  // below. See design make_chat_issue_parallel_working §3.4.
  if (session?.status === "closed") return "Session closed";
  if (session?.active_message_id || session?.active_turn_id) return "Response in progress";
  return null;
}

function sessionHistoryItem(session: IAgentChatSession, activeId: string | undefined): ChatHistoryItem {
  // Sessions have no title/first-message field, so entries are labelled by
  // their last activity time. renderFormattedDate falls back gracefully on a
  // freshly-created session that has no last_message_at yet.
  const stamp = session.last_message_at ?? session.created_at;
  const title = renderFormattedDate(stamp, "MMM dd, HH:mm") ?? "New chat";
  const statusSuffix = session.status === "open" ? "" : ` · ${session.status}`;
  const subtitle = session.last_message_at ? `${calculateTimeAgo(stamp)}${statusSuffix}` : `New chat${statusSuffix}`;
  return { id: session.id, title, subtitle, active: session.id === activeId };
}

const RunnerChatPage = observer(function RunnerChatPage() {
  const { runnerId } = useParams<{ runnerId: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const requestedSessionId = searchParams.get("sessionId");
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [creatingChat, setCreatingChat] = useState(false);
  const [events, setEvents] = useState<IAgentChatEvent[]>([]);
  const [liveMessages, setLiveMessages] = useState<IAgentChatMessage[]>([]);
  const warmSessionRef = useRef<string | null>(null);
  const createWarmKeyRef = useRef<string | null>(null);
  const precreatedSessionRef = useRef<IAgentChatSession | null>(null);
  const appliedDeltaSeqsRef = useRef<Set<number>>(new Set());
  const streamStartedAtRef = useRef(Date.now());

  const { data: runner } = useSWR<IRunner>(runnerId ? ["runner-detail", runnerId] : null, () =>
    getRunnerDetail(runnerId!)
  );
  const { data: sessions, mutate: mutateSessions } = useSWR<IAgentChatSession[]>(
    workspaceId && runnerId ? ["runner-chat-sessions", workspaceId, runnerId] : null,
    () => service.listChatSessions(workspaceId!, runnerId)
  );

  const session = useMemo(() => {
    const available = sessions ?? [];
    // An explicit ?sessionId (chosen from the history panel) wins, even for a
    // closed session so the user can read it back. A stale/unknown id (e.g. a
    // shared link to a session on another runner) falls through to auto-select.
    if (requestedSessionId) {
      const requested = available.find((s) => s.id === requestedSessionId);
      if (requested) return requested;
    }
    return (
      available.find((s) => s.status === "open" && s.last_message_at !== null) ??
      available.find((s) => s.status === "open") ??
      null
    );
  }, [sessions, requestedSessionId]);

  useEffect(() => {
    if (!sessions) return;
    const precreated = precreatedSessionRef.current;
    if (!precreated) return;
    const current = sessions.find((item) => item.id === precreated.id);
    precreatedSessionRef.current = current?.status === "open" ? current : null;
  }, [sessions]);

  useEffect(() => {
    warmSessionRef.current = null;
    createWarmKeyRef.current = null;
    precreatedSessionRef.current = null;
    setEvents([]);
  }, [runnerId]);

  useEffect(() => {
    appliedDeltaSeqsRef.current = new Set();
    streamStartedAtRef.current = Date.now();
    setEvents([]);
    setLiveMessages([]);
  }, [session?.id]);

  useEffect(() => {
    let cancelled = false;
    async function warmSelectedRunner() {
      // A "busy" runner (running an issue and/or already chatting) can still be
      // warmed: chat runs concurrently in a dedicated worktree. Only offline /
      // revoked runners can't serve chat (the server also rejects those). Not
      // warming a busy runner would skip the warm step that seeds
      // local_thread_id/local_session_id and break revive continuity.
      if (!workspaceId || !runnerId || !runner) return;
      if (runner.status !== "online" && runner.status !== "busy") return;
      if (session?.status === "open") {
        if (warmSessionRef.current === session.id) return;
        warmSessionRef.current = session.id;
        try {
          await service.warmChatSession(session.id);
        } catch {
          return;
        }
        return;
      }
      // Explicitly viewing a resolved session from the history panel (which may
      // be closed): don't silently spin up a brand-new session behind the user's
      // back. A stale ?sessionId (session?.id won't match) still auto-creates.
      if (session && session.id === requestedSessionId) return;
      if (!sessions) return;
      const key = `${workspaceId}:${runnerId}`;
      if (createWarmKeyRef.current === key) return;
      createWarmKeyRef.current = key;
      try {
        const created = await service.createChatSession({
          workspace: workspaceId,
          runner: runnerId,
        });
        if (cancelled) return;
        precreatedSessionRef.current = created;
        warmSessionRef.current = created.id;
        mutateSessions((current) => {
          const currentSessions = current ?? [];
          return currentSessions.some((item) => item.id === created.id)
            ? currentSessions
            : [created, ...currentSessions];
        }, false);
        await service.warmChatSession(created.id);
        mutateSessions();
      } catch {
        return;
      }
    }
    warmSelectedRunner();
    return () => {
      cancelled = true;
    };
  }, [mutateSessions, requestedSessionId, runner, runnerId, session, sessions, workspaceId]);

  const { data: messages, mutate: mutateMessages } = useSWR<IAgentChatMessage[]>(
    session?.id ? ["runner-chat-messages", session.id] : null,
    () => service.listChatMessages(session!.id)
  );

  useEffect(() => {
    setLiveMessages(messages ?? []);
  }, [messages]);

  const handleEvent = useCallback(
    (event: IAgentChatEvent) => {
      setEvents((prev) => (prev.some((item) => item.seq === event.seq) ? prev : [...prev, event]));
      if (event.kind === "assistant_delta") {
        if (!appliedDeltaSeqsRef.current.has(event.seq)) {
          appliedDeltaSeqsRef.current.add(event.seq);
          const delta = assistantDeltaText(event.payload);
          if (delta && isRealtimeEvent(event, streamStartedAtRef.current)) {
            setLiveMessages((prev) => appendAssistantDelta(prev, event, delta, session?.id));
          }
        }
        return;
      }
      if (["turn_started", "turn_completed", "chat_failed", "chat_closed", "chat_warmed"].includes(event.kind)) {
        mutateSessions();
        mutateMessages();
      }
    },
    [mutateMessages, mutateSessions, session?.id]
  );
  const handleEventError = useCallback(() => {
    mutateSessions();
    mutateMessages();
  }, [mutateMessages, mutateSessions]);
  useAgentChatEvents(session?.id, handleEvent, handleEventError);

  async function ensureSession(): Promise<IAgentChatSession> {
    if (session?.status === "open") return session;
    const precreated = precreatedSessionRef.current;
    if (precreated && precreated.workspace === workspaceId && precreated.runner === runnerId) {
      const current = sessions?.find((item) => item.id === precreated.id);
      const reusable = current ?? (sessions ? null : precreated);
      if (reusable?.status === "open") {
        return reusable;
      }
      precreatedSessionRef.current = null;
    }
    const created = await service.createChatSession({
      workspace: workspaceId!,
      runner: runnerId!,
    });
    precreatedSessionRef.current = created;
    await mutateSessions((current) => {
      const currentSessions = current ?? [];
      return currentSessions.some((item) => item.id === created.id) ? currentSessions : [created, ...currentSessions];
    }, false);
    return created;
  }

  async function send() {
    const content = draft.trim();
    if (!content || !workspaceId || !runnerId) return;
    setSending(true);
    setDraft("");
    try {
      const target = await ensureSession();
      await service.sendChatMessage(target.id, content);
      mutateMessages();
      mutateSessions();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setDraft(content);
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Chat failed",
        message: err?.error ?? "Unable to send message",
      });
    } finally {
      setSending(false);
    }
  }

  async function stop() {
    if (!session) return;
    await service.cancelChat(session.id, "user_cancelled");
    mutateSessions();
  }

  async function close() {
    if (!session) return;
    await service.closeChat(session.id);
    mutateSessions();
  }

  function selectSession(sessionId: string) {
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.set("sessionId", sessionId);
        return next;
      },
      { replace: true }
    );
  }

  async function newChat() {
    if (!workspaceId || !runnerId || creatingChat) return;
    setCreatingChat(true);
    try {
      const created = await service.createChatSession({ workspace: workspaceId, runner: runnerId });
      precreatedSessionRef.current = created;
      warmSessionRef.current = created.id;
      await mutateSessions((current) => {
        const currentSessions = current ?? [];
        return currentSessions.some((item) => item.id === created.id) ? currentSessions : [created, ...currentSessions];
      }, false);
      // Warm in the background; the session is usable without waiting for it.
      service.warmChatSession(created.id).catch(() => {});
      selectSession(created.id);
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Unable to start chat",
        message: err?.error ?? "Could not create a new chat session",
      });
    } finally {
      setCreatingChat(false);
    }
  }

  const historyItems = useMemo(() => {
    // Copy before sorting so we never mutate the SWR-cached sessions array.
    // eslint-disable-next-line unicorn/no-array-sort -- toSorted() isn't in the project's TS lib target
    const sorted = [...(sessions ?? [])].sort((a, b) => {
      const aTime = Date.parse(a.last_message_at ?? a.created_at);
      const bTime = Date.parse(b.last_message_at ?? b.created_at);
      return bTime - aTime;
    });
    return sorted.map((item) => sessionHistoryItem(item, session?.id));
  }, [sessions, session?.id]);

  const reason = disabledReason(runner, session);
  const rows = liveMessages;
  const busy = !!(session?.active_message_id || session?.active_turn_id);
  const eventStrip = events
    .filter(
      (event) =>
        !["assistant_delta", "turn_completed", "chat_closed", "chat_warmed", "chat_timing"].includes(event.kind)
    )
    .slice(-6)
    .map((event) => (
      <div key={event.seq} className="rounded border border-subtle bg-surface-1 px-3 py-2 text-11 text-secondary">
        <span className="font-mono">{event.kind}</span>
      </div>
    ));

  return (
    <div className="flex h-full min-h-[640px] w-full overflow-hidden">
      <ChatHistoryPanel
        heading="Chats"
        items={historyItems}
        onSelect={selectSession}
        onNewChat={newChat}
        newChatLabel="New chat"
        busy={creatingChat}
        emptyState={<div className="px-2 py-4 text-12 text-tertiary">No chats yet.</div>}
      />
      {/* The chat internals (header/list/composer) carry no horizontal padding —
          the host provides it (see the assistant thread page). Without this the
          content sits flush against the history panel's border. */}
      <div className="flex min-w-0 flex-1 flex-col overflow-hidden px-4 pb-4">
        <ChatContainer
          className="min-h-0 flex-1"
          header={
            <div className="flex h-12 shrink-0 items-center justify-between border-b border-subtle">
              <div className="min-w-0">
                <div className="text-15 truncate font-semibold text-primary">{runner?.name ?? "Runner"}</div>
                <div className="text-12 text-secondary">{runner?.pod_detail?.name ?? runner?.status ?? ""}</div>
              </div>
              <div className="flex items-center gap-2">
                {runner && (
                  <Badge variant={runner.status === "online" ? "accent-success" : "accent-neutral"}>
                    {runner.status}
                  </Badge>
                )}
                {session && (
                  <Button variant="neutral-primary" size="sm" onClick={close}>
                    <X className="size-4" />
                  </Button>
                )}
              </div>
            </div>
          }
          messages={rows}
          renderMessage={(m) => <ChatMessage role={m.role} content={m.content} status={m.status} />}
          emptyState={<div className="py-16 text-center text-13 text-secondary">No messages</div>}
          listFooter={eventStrip.length > 0 ? <>{eventStrip}</> : undefined}
          composer={
            <ChatComposer
              draft={draft}
              onDraftChange={setDraft}
              onSend={send}
              onStop={stop}
              busy={busy}
              sending={sending}
              disabledReason={reason}
              placeholder="Message this runner…"
            />
          }
        />
      </div>
    </div>
  );
});

export default RunnerChatPage;
