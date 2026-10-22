// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant landing (parity/specs/agents.md §9, AGT-038–AGT-040). A
// configured member gets a greeting and a standalone composer; without a
// provider key the setup card takes the chat entry's place. A submit
// creates exactly one untitled conversation and hands the first message
// to the conversation view as one-shot navigation state, so the view —
// not the landing — owns the send and its failure line. Guests reach this
// address like anyone else; the server refuses their writes and the
// refusal surfaces as a notice, exactly as the backend-driven exclusion
// describes (AGT-041).

import { Spinner } from "@pidash/kit";
import { Sparkles } from "lucide-react";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { startFailureDetail, useStartConversation } from "../api/mutations.js";
import { useAssistantConfig } from "../api/queries.js";
import { useComposerDictation } from "../hooks/useComposerDictation.js";
import { ASSISTANT_CAPTION, ASSISTANT_HEADLINE, START_FAILED_TITLE } from "../lib/copy.js";
import { needsLaneSetup } from "../lib/gate.js";
import { notifyError } from "../../../shared/shell/index.js";
import { appendDictation } from "../../../shared/dictation/index.js";
import { Composer } from "./Composer.js";
import { SetupCard } from "./SetupCard.js";

export interface AssistantLandingProps {
  workspaceSlug: string;
}

// The config read decides the branch; the parity driver's cold-open wait
// polls each surface's `data-settled` attribute so a pending read cannot
// read as "no greeting" or "no setup card".
export function AssistantLanding({ workspaceSlug }: AssistantLandingProps): React.ReactElement {
  const config = useAssistantConfig(workspaceSlug);
  const start = useStartConversation();
  const navigate = useNavigate();
  const [draft, setDraft] = React.useState("");
  const appendTranscript = React.useCallback((text: string) => {
    setDraft((current) => appendDictation(current, text));
  }, []);
  const dictation = useComposerDictation(appendTranscript);

  const onStart = (message: string): void => {
    // The composer locks while starting, so no guard is needed here.
    start.mutate(
      { workspaceSlug },
      {
        onSuccess: (thread) => {
          void navigate({
            to: "/$ws/assistant/$threadId",
            params: { ws: workspaceSlug, threadId: thread.id },
            state: { pendingMessage: message },
          });
        },
        onError: (error) => {
          notifyError(START_FAILED_TITLE, startFailureDetail(error));
        },
      }
    );
  };

  return (
    <div className="flex h-full w-full overflow-auto px-(--space-4)">
      <div
        data-testid="assistant-landing"
        data-settled={config.isPending ? "false" : "true"}
        className="m-auto flex w-full max-w-2xl flex-col gap-(--space-6) py-(--space-8)"
      >
        {config.isPending ? (
          <Spinner label="Loading assistant" />
        ) : needsLaneSetup(config.data) ? (
          <SetupCard />
        ) : (
          <>
            <header className="flex flex-col items-center gap-(--space-2) text-center">
              <Sparkles aria-hidden="true" size={26} className="text-(--accent)" />
              <h1 className="text-h2 text-(--text)">{ASSISTANT_HEADLINE}</h1>
              <p className="text-body text-(--text-muted)">{ASSISTANT_CAPTION}</p>
            </header>
            <Composer
              draft={draft}
              onDraftChange={setDraft}
              onSubmit={onStart}
              starting={start.isPending}
              bordered={false}
              dictation={dictation}
            />
          </>
        )}
      </div>
    </div>
  );
}
