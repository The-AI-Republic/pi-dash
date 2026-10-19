// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation gate (parity/specs/agents.md §9, AGT-039/AGT-040, plus the
// AGT-038 handoff). The shell a conversation renders inside: a centered
// column whose body is the empty state (setup card when no provider key,
// the plain hint otherwise) and whose footer is the composer in its locked
// or unlocked state. Transcript rendering, streaming and the stop action
// are the thread-view slice's internals; this shell owns the gates around
// them and delivers the one-shot first message exactly once per thread.

import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { useAssistantConfig } from "../api/queries.js";
import { startFailureDetail, useDeliverFirstMessage } from "../api/mutations.js";
import { EMPTY_CONVERSATION_HINT, HANDOFF_FAILED_TITLE } from "../lib/copy.js";
import { readPendingMessage } from "../lib/navigation.js";
import { needsSetup } from "../lib/gate.js";
import { notifyError } from "../../../shared/shell/index.js";
import { Composer } from "./Composer.js";
import { SetupCard } from "./SetupCard.js";

export interface ConversationGateProps {
  workspaceSlug: string;
  threadId: string;
  /** Router state carried by the navigation that opened this conversation. */
  state: unknown;
  /** True once the thread-view slice renders a transcript; it then owns the body. */
  hasTranscript?: boolean;
  /** Renders the transcript region when the thread-view slice is present. */
  transcript?: React.ReactNode;
}

export function ConversationGate({
  workspaceSlug,
  threadId,
  state,
  hasTranscript = false,
  transcript,
}: ConversationGateProps): React.ReactElement {
  const config = useAssistantConfig(workspaceSlug);
  const deliver = useDeliverFirstMessage();
  const navigate = useNavigate();
  const [draft, setDraft] = React.useState("");
  const handoff = readPendingMessage(state);
  const deliveredFor = React.useRef<string | null>(null);

  React.useEffect(() => {
    if (handoff === null || deliveredFor.current === threadId) return;
    deliveredFor.current = threadId;
    // Drop the handoff from history before sending so a refresh or a back
    // navigation can never resend it (AGT-053's exactly-once rule).
    void navigate({ href: "", replace: true, state: {} });
    deliver.mutate(
      { workspaceSlug, threadId, content: handoff },
      {
        onError: (error) => notifyError(HANDOFF_FAILED_TITLE, startFailureDetail(error)),
      }
    );
  }, [deliver, handoff, navigate, threadId, workspaceSlug]);

  const setup = needsSetup(config.data);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div
        data-testid="assistant-thread"
        data-thread-id={threadId}
        className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col gap-(--space-4) overflow-y-auto px-(--space-6) py-(--space-6)"
      >
        {hasTranscript ? (
          transcript
        ) : setup ? (
          <SetupCard />
        ) : (
          <p data-testid="assistant-empty-hint" className="text-body m-auto text-(--text-muted)">
            {EMPTY_CONVERSATION_HINT}
          </p>
        )}
      </div>
      <div className="mx-auto w-full max-w-3xl shrink-0 px-(--space-6) pb-(--space-4)">
        <Composer draft={draft} onDraftChange={setDraft} onSubmit={() => undefined} configured={!setup} />
      </div>
    </div>
  );
}
