// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner chat page (NEWFRONT-549 RUN-025/026/028/032, messaging
// NEWFRONT-551 RUN-027/029/030/031, desktop chat NEWFRONT-553
// RUN-033-036). Composition: the runner comes from the address, the history
// column lists the runner's conversations and picks one, opening the
// runner prepares a session, the live feed streams the reply and derives
// the activity strip, and the composer/header carry send, stop and close
// with the state the transport reports. On the desktop, the header also
// carries the approval mode and a parked approval renders above the
// composer; the cloud transport has neither verb, so neither shows.

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
import { ChatApprovalMode } from "./ChatApprovalMode.js";
import { ChatApprovalPrompt } from "./ChatApprovalPrompt.js";
import {
  approvalRemainingMs,
  pendingApprovalFromEvent,
  rememberApprovalMode,
  storedApprovalMode,
  type ApprovalMode,
  type PendingApproval,
} from "./approvalCopy.js";
import { chatTransportForRunner, subscribeChatTransport } from "./transport.js";
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

  // The desktop bootstrap installs the local transport after the runtime
  // resumes, which can be after this page mounted; follow the seam so the
  // approval controls appear and sends take the local path from then on.
  const [surface, setSurface] = React.useState(() => chatTransportForRunner(runnerId));
  React.useEffect(() => {
    setSurface(chatTransportForRunner(runnerId));
    return subscribeChatTransport(() => setSurface(chatTransportForRunner(runnerId)));
  }, [runnerId]);
  const transport = surface;
  // The approval controls belong to the local engine and to the contact it
  // serves; the cloud transport has no decision verb, so cloud and web never
  // render them, and neither does a server runner on the desktop (RUN-034/035).
  const approvalsSupported = typeof transport.decideChatApproval === "function";
  const [approvalMode, setApprovalModeState] = React.useState<ApprovalMode>(() => storedApprovalMode(runnerId));
  const approvalModeRef = React.useRef(approvalMode);
  const [pendingApproval, setPendingApproval] = React.useState<PendingApproval | null>(null);
  const [deciding, setDeciding] = React.useState(false);
  const [decisionError, setDecisionError] = React.useState<string | null>(null);

  const chooseMode = React.useCallback(
    (mode: ApprovalMode) => {
      approvalModeRef.current = mode;
      setApprovalModeState(mode);
      // Persist per runner; the value the engine captures is handed to the
      // session, so a turn already in flight keeps the mode it started with.
      rememberApprovalMode(runnerId, mode);
      // The runner-level value covers a conversation the page has not
      // created yet; the session-level one covers the one on screen.
      transport.setRunnerApprovalMode?.(runnerId, mode);
      if (session !== null) transport.setApprovalMode?.(session.id, mode);
    },
    [runnerId, session?.id, transport]
  );

  const decideApproval = React.useCallback(
    async (decision: "accept" | "decline" | "accept_for_session") => {
      const request = pendingApproval;
      if (request === null || session === null || transport.decideChatApproval === undefined) return;
      setDeciding(true);
      // Clear optimistically so a slow round-trip does not leave a stale
      // prompt; a refusal puts it back so the choice can be retried.
      setPendingApproval(null);
      try {
        await transport.decideChatApproval(session.id, request.localApprovalId, decision);
      } catch (error) {
        setDecisionError(
          error instanceof Error && error.message !== "" ? error.message : "The decision did not reach the agent."
        );
        setPendingApproval(request);
      } finally {
        setDeciding(false);
      }
    },
    [pendingApproval, session, transport]
  );

  const refetchSessions = React.useCallback(() => {
    void sessions.refetch();
  }, [sessions]);
  const refetchMessages = React.useCallback(() => {
    void messages.refetch();
  }, [messages]);

  const onApprovalCleared = React.useCallback(() => {
    setPendingApproval(null);
    setDecisionError(null);
  }, []);
  const stream = useChatStream(
    session?.id ?? null,
    messages.data ?? [],
    {
      onHistoryRefresh: () => {
        refetchSessions();
        refetchMessages();
      },
      onApprovalCleared,
    },
    runnerId
  );

  // A different conversation starts with no prompt and no carried refusal.
  React.useEffect(() => {
    setPendingApproval(null);
    setDecisionError(null);
  }, [session?.id]);

  // Fold a parked request into the page. Only one prompt shows at a time
  // (RUN-034): a newer request replaces the one on screen.
  const approvalEvent = stream.approval;
  React.useEffect(() => {
    if (!approvalsSupported || approvalEvent === null) return;
    const request = pendingApprovalFromEvent(approvalEvent.payload);
    if (request !== null) setPendingApproval(request);
  }, [approvalsSupported, approvalEvent]);

  // The runner drops a request at its own deadline; stop offering buttons
  // for it at the same moment rather than after a failed decision.
  React.useEffect(() => {
    if (pendingApproval === null) return;
    const remaining = approvalRemainingMs(pendingApproval, Date.now());
    if (remaining === null) return;
    if (remaining <= 0) {
      setPendingApproval(null);
      return;
    }
    const timer = setTimeout(() => setPendingApproval(null), remaining);
    return () => clearTimeout(timer);
  }, [pendingApproval]);

  // Load the stored mode for the contact on screen, and hand it to the
  // session the page is about to warm (RUN-035).
  React.useEffect(() => {
    if (!approvalsSupported) return;
    approvalModeRef.current = storedApprovalMode(runnerId);
    setApprovalModeState(approvalModeRef.current);
    // Hand it over before warm-up can create a conversation for this
    // runner, so that first thread spawns under the stored mode.
    transport.setRunnerApprovalMode?.(runnerId, approvalModeRef.current);
  }, [approvalsSupported, runnerId, transport]);

  React.useEffect(() => {
    if (!approvalsSupported || session === null) return;
    transport.setApprovalMode?.(session.id, approvalModeRef.current);
  }, [approvalsSupported, session, transport]);

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
        <ChatHeader
          summary={summary}
          {...(session === null ? {} : { onClose: () => void actions.close() })}
          {...(approvalsSupported
            ? {
                // The mode applies to the next thread, so a send in flight
                // does not lock it.
                actions: <ChatApprovalMode value={approvalMode} onChange={chooseMode} />,
              }
            : {})}
        />
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
        {approvalsSupported && pendingApproval !== null ? (
          <ChatApprovalPrompt
            request={pendingApproval}
            onDecide={(decision) => void decideApproval(decision)}
            busy={deciding}
          />
        ) : null}
        {decisionError === null ? null : (
          <div
            role="alert"
            data-testid="chat-approval-error"
            className="text-meta mb-(--space-2) rounded-(--radius-control) border border-(--danger) px-(--space-3) py-(--space-2) text-(--danger)"
          >
            {decisionError}
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
          onStop={() => {
            // Interrupting answers any parked request, so it must not stay
            // on screen as buttons that can no longer do anything.
            onApprovalCleared();
            void actions.stop();
          }}
          sending={actions.sending}
          dictationAvailable={dictationAvailable}
        />
      </div>
    </section>
  );
}
