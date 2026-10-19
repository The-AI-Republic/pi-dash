// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner chat page (NEWFRONT-549 RUN-025/026/028/032, messaging
// NEWFRONT-551 RUN-027/029/030/031). Composition: the runner comes from
// the address, the history column lists the runner's conversations and
// picks one, opening the runner prepares a session, the live feed streams
// the reply and derives the activity strip, and the composer/header carry
// send, stop and close with the state the transport reports.

import * as React from "react";

import type { ChatSession } from "@pidash/api-client";

import { useDictationConfig, dictationOffered } from "../../../shared/dictation/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { useRunnersArea } from "../area.js";
import {
  buildHistoryEntries,
  chatHeaderSummary,
  composerBlockReason,
  isRunnerBusy,
  selectSession,
} from "./chatState.js";
import { ChatComposer } from "./ChatComposer.js";
import { ChatHeader } from "./ChatHeader.js";
import { ChatHistory } from "./ChatHistory.js";
import { ChatTranscript } from "./ChatTranscript.js";
import { useChatMessages, useChatRunner, useChatSessions } from "./queries.js";
import { useChatActions } from "./useChatActions.js";
import { useChatStream } from "./useChatStream.js";
import { useNewChat } from "./useNewChat.js";
import { useWarmUp } from "./useWarmUp.js";

/** Page title while the runner row is still unknown. */
export const RUNNER_CHAT_PAGE_TITLE = "Runner chat";

export interface RunnerChatPageProps {
  /** Workspace slug from the route. */
  workspaceSlug: string;
  /** Runner id from the address; deep-linkable. */
  runnerId: string;
  /** `?sessionId=` when a specific conversation is linked. */
  requestedSessionId: string | null;
  /** Writes an explicit selection into the address. */
  onSelectSession: (sessionId: string) => void;
}

/** Document title for a resolved runner. */
export function runnerChatDocumentTitle(runnerName: string | undefined): string {
  if (runnerName === undefined || runnerName === "") return RUNNER_CHAT_PAGE_TITLE;
  return `${runnerName} - ${RUNNER_CHAT_PAGE_TITLE}`;
}

export function RunnerChatPage({
  workspaceSlug,
  runnerId,
  requestedSessionId,
  onSelectSession,
}: RunnerChatPageProps): React.ReactElement {
  const area = useRunnersArea();
  const runner = useChatRunner(workspaceSlug, runnerId, area.allowed);
  const sessions = useChatSessions(workspaceSlug, area.workspaceId, runnerId, area.allowed);
  const [pendingId, setPendingId] = React.useState<string | null>(null);
  const [precreated, setPrecreated] = React.useState<ChatSession | null>(null);
  // The list snapshot that was current when the session was created; until
  // a later read answers, the cache cannot say whether it still exists.
  const listAtCreateRef = React.useRef<ChatSession[] | undefined>(undefined);

  // A different contact starts over: no pick, no locally created session,
  // no inherited conversation state.
  React.useEffect(() => {
    setPendingId(null);
    setPrecreated(null);
    listAtCreateRef.current = undefined;
  }, [runnerId]);

  // Once the address carries the pick, the in-memory override is redundant.
  React.useEffect(() => {
    if (pendingId !== null && requestedSessionId === pendingId) setPendingId(null);
  }, [pendingId, requestedSessionId]);

  const session = React.useMemo(
    () =>
      selectSession({
        sessions: sessions.data,
        pendingId,
        requestedId: requestedSessionId,
        precreated,
      }),
    [pendingId, precreated, requestedSessionId, sessions.data]
  );

  // A created session is dropped once a *later* list read has answered and
  // no longer knows it. If the read lists it closed, the list copy is what
  // the page shows, so the composer can say why it is shut.
  React.useEffect(() => {
    if (precreated === null || sessions.data === undefined || sessions.data === listAtCreateRef.current) return;
    if (!sessions.data.some((item) => item.id === precreated.id)) setPrecreated(null);
  }, [precreated, sessions.data]);

  const select = React.useCallback(
    (sessionId: string) => {
      setPendingId(sessionId);
      onSelectSession(sessionId);
    },
    [onSelectSession]
  );

  const listSnapshotRef = React.useRef(sessions.data);
  listSnapshotRef.current = sessions.data;
  // A session created here is held in memory only. It is displayed as a
  // fallback, so a click or a deep link that lands while the create runs
  // still wins; New chat adds an explicit pick itself through `select`.
  const onCreated = React.useCallback((created: ChatSession) => {
    listAtCreateRef.current = listSnapshotRef.current;
    setPrecreated(created);
  }, []);

  const autoCreating = useWarmUp({
    workspaceSlug,
    workspaceId: area.workspaceId,
    runnerId,
    runner: runner.data,
    sessions: sessions.data,
    session,
    pendingId,
    requestedId: requestedSessionId,
    enabled: area.allowed,
    onCreated,
  });

  const newChat = useNewChat({
    workspaceSlug,
    workspaceId: area.workspaceId,
    runnerId,
    onSelect: select,
    onCreated,
    busy: autoCreating,
  });

  const messages = useChatMessages(workspaceSlug, session?.id ?? null, area.allowed);
  const summary = chatHeaderSummary(runner.data, session);
  const blockedReason = composerBlockReason(runner.data, session);
  const turnInFlight = isRunnerBusy(session);
  const entries = React.useMemo(
    () => buildHistoryEntries(sessions.data ?? [], session?.id),
    [session?.id, sessions.data]
  );

  const [draft, setDraft] = React.useState("");
  // A different contact starts with an empty draft; a failed draft is
  // restored later on the same contact only.
  React.useEffect(() => {
    setDraft("");
  }, [runnerId]);

  const refetchSessions = React.useCallback(() => {
    void sessions.refetch();
  }, [sessions]);
  const refetchMessages = React.useCallback(() => {
    void messages.refetch();
  }, [messages]);

  const stream = useChatStream(session?.id ?? null, messages.data ?? [], {
    onHistoryRefresh: () => {
      refetchSessions();
      refetchMessages();
    },
    onApprovalCleared: () => undefined,
  });

  const actions = useChatActions({
    workspaceSlug,
    workspaceId: area.workspaceId,
    runnerId,
    session,
    onSelectSession: select,
    onCreated,
  });

  const speech = useDictationConfig(area.allowed);
  const dictationAvailable = dictationOffered(speech.data);

  useDocumentHead({ title: runnerChatDocumentTitle(runner.data?.name), robots: "noindex, nofollow" });

  return (
    <section aria-label="Runner chat" data-testid="runner-chat" className="flex h-full min-h-0 w-full">
      <ChatHistory entries={entries} onSelect={select} onNewChat={() => void newChat.start()} busy={newChat.busy} />
      <div className="flex min-w-0 flex-1 flex-col px-(--space-4) pb-(--space-4)">
        <ChatHeader summary={summary} {...(session === null ? {} : { onClose: () => void actions.close() })} />
        <ChatTranscript
          messages={stream.messages}
          loading={messages.isPending && session !== null}
          activity={stream.activity}
        />
        {stream.error === null && actions.sendError === null ? null : (
          <div
            role="alert"
            className="text-meta mb-(--space-2) flex items-start justify-between gap-(--space-2) rounded-(--radius-control) border border-(--danger) bg-(--surface) px-(--space-3) py-(--space-2) text-(--danger)"
          >
            <span className="min-w-0">{stream.error ?? actions.sendError}</span>
            <button
              type="button"
              className="shrink-0 underline"
              onClick={() => {
                stream.dismissError();
                actions.dismissSendError();
              }}
            >
              Dismiss
            </button>
          </div>
        )}
        <ChatComposer
          blockedReason={blockedReason}
          draft={draft}
          onDraftChange={setDraft}
          onSend={(value) => {
            // The draft clears at once; a refusal puts it back verbatim.
            setDraft("");
            void actions.send(value, setDraft);
          }}
          turnInFlight={turnInFlight}
          onStop={() => void actions.stop()}
          sending={actions.sending}
          dictationAvailable={dictationAvailable}
        />
      </div>
    </section>
  );
}
