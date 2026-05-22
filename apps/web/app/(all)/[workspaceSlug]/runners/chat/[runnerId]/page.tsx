/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useMemo, useState } from "react";
import { observer } from "mobx-react";
import { Send, Square, X } from "lucide-react";
import { useParams } from "react-router";
import useSWR from "swr";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { RunnerService } from "@pi-dash/services";
import type { IAgentChatEvent, IAgentChatMessage, IAgentChatSession, IRunner } from "@pi-dash/types";
import { Badge, Button } from "@pi-dash/ui";
import { useAgentChatEvents } from "@/components/runners/chat/use-agent-chat-events";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();

function disabledReason(runner?: IRunner, session?: IAgentChatSession | null): string | null {
  if (!runner) return "Loading";
  if (runner.status === "offline") return "Runner offline";
  if (runner.status === "revoked") return "Runner revoked";
  if (runner.status === "busy") return "Runner busy";
  if (session?.status === "closed") return "Session closed";
  if (session?.active_message_id || session?.active_turn_id) return "Response in progress";
  return null;
}

const RunnerChatPage = observer(function RunnerChatPage() {
  const { runnerId } = useParams<{ runnerId: string }>();
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [events, setEvents] = useState<IAgentChatEvent[]>([]);

  const { data: runner } = useSWR<IRunner>(runnerId ? ["runner-detail", runnerId] : null, () =>
    service.getDetail(runnerId!)
  );
  const { data: sessions, mutate: mutateSessions } = useSWR<IAgentChatSession[]>(
    workspaceId && runnerId ? ["runner-chat-sessions", workspaceId, runnerId] : null,
    () => service.listChatSessions(workspaceId!, runnerId)
  );

  const session = useMemo(
    () =>
      (sessions ?? []).find((s) => s.status === "open" && s.last_message_at !== null) ??
      (sessions ?? []).find((s) => s.status === "open") ??
      null,
    [sessions]
  );

  const { data: messages, mutate: mutateMessages } = useSWR<IAgentChatMessage[]>(
    session?.id ? ["runner-chat-messages", session.id] : null,
    () => service.listChatMessages(session!.id)
  );

  const handleEvent = useCallback(
    (event: IAgentChatEvent) => {
      setEvents((prev) => (prev.some((item) => item.seq === event.seq) ? prev : [...prev, event]));
      if (["assistant_delta", "turn_started", "turn_completed", "chat_failed", "chat_closed"].includes(event.kind)) {
        mutateSessions();
        mutateMessages();
      }
    },
    [mutateMessages, mutateSessions]
  );
  useAgentChatEvents(session?.id, 0, handleEvent);

  async function ensureSession(): Promise<IAgentChatSession> {
    if (session?.status === "open") return session;
    const created = await service.createChatSession({
      workspace: workspaceId!,
      runner: runnerId!,
    });
    await mutateSessions();
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

  const reason = disabledReason(runner, session);
  const rows = messages ?? [];

  return (
    <div className="flex h-full min-h-[640px] flex-col overflow-hidden">
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-subtle">
        <div className="min-w-0">
          <div className="text-15 truncate font-semibold text-primary">{runner?.name ?? "Runner"}</div>
          <div className="text-12 text-secondary">{runner?.pod_detail?.name ?? runner?.status ?? ""}</div>
        </div>
        <div className="flex items-center gap-2">
          {runner && (
            <Badge variant={runner.status === "online" ? "accent-success" : "accent-neutral"}>{runner.status}</Badge>
          )}
          {session && (
            <Button variant="neutral-primary" size="sm" onClick={close}>
              <X className="size-4" />
            </Button>
          )}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto py-4">
        {rows.length === 0 ? (
          <div className="py-16 text-center text-13 text-secondary">No messages</div>
        ) : (
          <div className="flex flex-col gap-3">
            {rows.map((message) => (
              <div key={message.id} className={`flex ${message.role === "user" ? "justify-end" : "justify-start"}`}>
                <div
                  className={`max-w-[72%] rounded-md px-3 py-2 text-13 ${
                    message.role === "user" ? "bg-brand-accent text-white" : "bg-layer-1 text-primary"
                  }`}
                >
                  <div className="whitespace-pre-wrap">{message.content || message.status}</div>
                </div>
              </div>
            ))}
            {events
              .filter((event) => !["assistant_delta", "turn_completed", "chat_closed"].includes(event.kind))
              .slice(-6)
              .map((event) => (
                <div
                  key={event.seq}
                  className="bg-layer-0 rounded border border-subtle px-3 py-2 text-11 text-secondary"
                >
                  <span className="font-mono">{event.kind}</span>
                </div>
              ))}
          </div>
        )}
      </div>

      <div className="shrink-0 border-t border-subtle pt-3">
        {reason && <div className="mb-2 text-12 text-secondary">{reason}</div>}
        <div className="flex items-end gap-2">
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            disabled={!!reason || sending}
            className="bg-layer-0 focus:border-brand-accent min-h-20 flex-1 resize-none rounded-md border border-subtle px-3 py-2 text-13 outline-none"
          />
          {session?.active_message_id || session?.active_turn_id ? (
            <Button onClick={stop} variant="tertiary-danger">
              <Square className="size-4" />
            </Button>
          ) : (
            <Button onClick={send} disabled={!!reason || !draft.trim()} loading={sending}>
              <Send className="size-4" />
            </Button>
          )}
        </div>
      </div>
    </div>
  );
});

export default RunnerChatPage;
