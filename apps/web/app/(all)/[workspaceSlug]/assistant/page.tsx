/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { Sparkles } from "lucide-react";
import { useNavigate, useParams } from "react-router";
import useSWR, { useSWRConfig } from "swr";
import { setToast, TOAST_TYPE } from "@pi-dash/propel/toast";
import { AssistantService } from "@pi-dash/services";
import type { IUserLLMConfig } from "@pi-dash/types";
import { AssistantSetupCard } from "@/components/assistant/setup-card";
import { ChatComposer } from "@/components/chat/composer";

const service = new AssistantService();

// Thread titles come from the first message; keep them list-friendly.
const TITLE_MAX_LENGTH = 80;

const AssistantIndexPage = observer(function AssistantIndexPage() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const navigate = useNavigate();
  const { mutate } = useSWRConfig();
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const { data: config } = useSWR<IUserLLMConfig>("assistant-llm-config", () => service.getLLMConfig());

  // `config` is undefined while loading — only treat as "needs setup" once known.
  const needsSetup = config ? !config.has_api_key : false;

  const startChat = async () => {
    const content = draft.trim();
    if (!content || !workspaceSlug || sending) return;
    setSending(true);
    try {
      const thread = await service.createThread(workspaceSlug, content.slice(0, TITLE_MAX_LENGTH));
      await service.sendMessage(workspaceSlug, thread.id, content);
      mutate(["assistant-threads", workspaceSlug]);
      navigate(`/${workspaceSlug}/assistant/${thread.id}`);
    } catch (e: unknown) {
      const err = e as { error?: string; detail?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Unable to start chat",
        message: err?.detail || err?.error || "Something went wrong. Please try again.",
      });
      setSending(false);
    }
  };

  return (
    <div className="flex h-full w-full items-center justify-center overflow-auto px-4">
      <div className="w-full max-w-2xl">
        {needsSetup ? (
          <AssistantSetupCard />
        ) : (
          <>
            <div className="mb-6 flex flex-col items-center gap-2 text-center">
              <Sparkles className="size-8 text-accent-primary" />
              <h1 className="text-20 font-semibold text-primary">How can I help you today?</h1>
              <p className="text-13 text-secondary">Ask about your issues, create work, or start a coding run.</p>
            </div>
            <ChatComposer
              draft={draft}
              onDraftChange={setDraft}
              onSend={startChat}
              sending={sending}
              className="w-full"
            />
          </>
        )}
      </div>
    </div>
  );
});

export default AssistantIndexPage;
