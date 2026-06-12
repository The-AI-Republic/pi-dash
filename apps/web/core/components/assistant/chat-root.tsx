/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import useSWR from "swr";
import { setToast, TOAST_TYPE } from "@pi-dash/propel/toast";
import { AssistantService } from "@pi-dash/services";
import type { IUserLLMConfig } from "@pi-dash/types";
import { ChatComposer } from "@/components/chat/composer";
import { ChatMessageList } from "@/components/chat/message-list";
import { AssistantMessage } from "@/components/assistant/assistant-message";
import { AssistantSetupCard } from "@/components/assistant/setup-card";
import { useAssistantChat } from "@/components/assistant/use-assistant-chat";

const service = new AssistantService();

const API_KEY_REMINDER = "Please set your API key in Settings first to start using AI Assistant.";

export function AssistantChatRoot({ slug, threadId }: { slug: string; threadId: string }) {
  const [draft, setDraft] = useState("");
  const { data: config } = useSWR<IUserLLMConfig>("assistant-llm-config", () => service.getLLMConfig());
  const { messages, busy, sending, send, stop, error } = useAssistantChat(slug, threadId);

  // `config` is undefined while loading — only treat as "needs setup" once known.
  const needsSetup = config ? !config.has_api_key : false;

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
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <ChatMessageList
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
      />
      {error && <div className="text-danger mb-2 text-12">{error}</div>}
      <ChatComposer
        draft={draft}
        onDraftChange={setDraft}
        onSend={onSend}
        onStop={stop}
        busy={busy}
        sending={sending}
        disabledReason={needsSetup ? API_KEY_REMINDER : null}
      />
    </div>
  );
}
