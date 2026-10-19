// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner chat page (NEWFRONT-549, RUN-025/026/028/032). Composition only:
// the runner comes from the address, the history column lists the
// runner's conversations and picks one, opening the runner prepares a
// session, and the header/composer reflect the live state. Messaging
// (send, stream, stop, close) lands on top of this page in NEWFRONT-551.

import * as React from "react";

import type { ChatSession } from "@pidash/api-client";

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

  // A created-but-unlisted session is dropped once a *later* list read has
  // answered and either lost it or reports it closed.
  React.useEffect(() => {
    if (precreated === null || sessions.data === undefined || sessions.data === listAtCreateRef.current) return;
    const current = sessions.data.find((item) => item.id === precreated.id);
    if (current === undefined || current.status !== "open") setPrecreated(null);
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
  const onCreated = React.useCallback((created: ChatSession) => {
    listAtCreateRef.current = listSnapshotRef.current;
    setPrecreated(created);
    // Warm-up only creates when nothing was showing, so the new session is
    // the one to display; it is held in memory, not written to the address.
    setPendingId(created.id);
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

  useDocumentHead({ title: runnerChatDocumentTitle(runner.data?.name), robots: "noindex, nofollow" });

  return (
    <section aria-label="Runner chat" data-testid="runner-chat" className="flex h-full min-h-0 w-full">
      <ChatHistory entries={entries} onSelect={select} onNewChat={() => void newChat.start()} busy={newChat.busy} />
      <div className="flex min-w-0 flex-1 flex-col px-(--space-4) pb-(--space-4)">
        <ChatHeader summary={summary} />
        <ChatTranscript messages={messages.data ?? []} loading={messages.isPending && session !== null} />
        <ChatComposer blockedReason={blockedReason} turnInFlight={turnInFlight} />
      </div>
    </section>
  );
}
