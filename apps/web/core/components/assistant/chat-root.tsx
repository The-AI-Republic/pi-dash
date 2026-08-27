/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { setToast, TOAST_TYPE } from "@pi-dash/propel/toast";
import { ChatComposer } from "@/components/chat/composer";
import { ChatContainer } from "@/components/chat/container";
import { AssistantMessage } from "@/components/assistant/assistant-message";
import { AssistantSetupCard } from "@/components/assistant/setup-card";
import { useAssistantChat } from "@/components/assistant/use-assistant-chat";
import { useLLMConfig } from "@/components/assistant/use-llm-config";

const API_KEY_REMINDER = "Please set your API key in Settings first to start using AI Assistant.";

export function AssistantChatRoot({ slug, threadId }: { slug: string; threadId: string }) {
  const [draft, setDraft] = useState("");
  const { needsSetup } = useLLMConfig();
  const { messages, busy, sending, send, stop, error } = useAssistantChat(slug, threadId);
  const location = useLocation();
  const navigate = useNavigate();
  // The landing page hands off the first message via router state so this
  // view owns the send (arming the busy/poll fallback). Guard by threadId so
  // it fires once per thread even across re-renders/StrictMode.
  const pendingSentFor = useRef<string | null>(null);

  useEffect(() => {
    const pending = (location.state as { pendingMessage?: string } | null)?.pendingMessage;
    if (!pending || pendingSentFor.current === threadId) return;
    pendingSentFor.current = threadId;
    // Clear the state so refresh/back doesn't re-send the message.
    navigate(location.pathname, { replace: true });
    void send(pending);
  }, [location, navigate, send, threadId]);

  const onSend = async () => {
    const content = draft.trim();
    if (!content) return;
    if (needsSetup) {
      setToast({ type: TOAST_TYPE.ERROR, title: "API key required", message: API_KEY_REMINDER });
      return;
    }
    setDraft("");
    await send(content);
  };

  return (
    <ChatContainer
      messages={messages}
      renderMessage={(m) => <AssistantMessage message={m} />}
      emptyState={
        needsSetup ? (
          <AssistantSetupCard />
        ) : (
          <div className="py-16 text-center text-13 text-secondary">
            Ask about your issues, create work, or start a coding run.
          </div>
        )
      }
      error={error ? <div className="text-danger mb-2 text-12">{error}</div> : undefined}
      composer={
        <ChatComposer
          draft={draft}
          onDraftChange={setDraft}
          onSend={onSend}
          onStop={stop}
          busy={busy}
          sending={sending}
          disabledReason={needsSetup ? API_KEY_REMINDER : null}
        />
      }
    />
  );
}
