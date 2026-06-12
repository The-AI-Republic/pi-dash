/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { Sparkles } from "lucide-react";
import { Link, useNavigate } from "react-router";
import useSWR from "swr";
import { setToast, TOAST_TYPE } from "@pi-dash/propel/toast";
import { AssistantService } from "@pi-dash/services";
import { EUserWorkspaceRoles, type IAssistantThread, type IUserLLMConfig } from "@pi-dash/types";
import { Button } from "@pi-dash/ui";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new AssistantService();

const SUGGESTIONS = ["What's assigned to me?", "Create an issue in…", "Summarize open issues in…"];
const API_KEY_REMINDER = "Please set your API key in Settings first to start using AI Assistant.";

export const AssistantHomeWidget = observer(function AssistantHomeWidget() {
  const { currentWorkspace } = useWorkspace();
  const navigate = useNavigate();
  const [draft, setDraft] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const slug = currentWorkspace?.slug;
  const role = currentWorkspace?.role ?? 0;

  const enabled = !!slug && role >= EUserWorkspaceRoles.MEMBER;

  const { data: threads } = useSWR<IAssistantThread[]>(enabled ? ["assistant-threads", slug] : null, () =>
    service.listThreads(slug!)
  );
  const { data: config } = useSWR<IUserLLMConfig>(enabled ? "assistant-llm-config" : null, () =>
    service.getLLMConfig()
  );

  // Guests do not get the assistant (parity with the backend 403).
  if (!enabled || !slug) return null;

  const start = async (text: string) => {
    const content = text.trim();
    if (!content || submitting) return;
    // Remind the user to configure a key before starting a conversation.
    if (config && !config.has_api_key) {
      setToast({ type: TOAST_TYPE.ERROR, title: "API key required", message: API_KEY_REMINDER });
      return;
    }
    setSubmitting(true);
    try {
      const thread = await service.createThread(slug);
      await service.sendMessage(slug, thread.id, content);
      navigate(`/${slug}/assistant/${thread.id}`);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="rounded-lg border border-subtle bg-surface-1 p-4">
      <div className="mb-2 flex items-center gap-2 text-13 font-semibold text-primary">
        <Sparkles className="size-4 text-accent-primary" /> Pi Dash AI
      </div>
      <div className="flex items-end gap-2">
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") start(draft);
          }}
          placeholder="Ask Pi to do something…"
          className="flex-1 rounded-md border border-subtle bg-canvas px-3 py-2 text-13 outline-none focus:border-accent-strong"
        />
        <Button onClick={() => start(draft)} disabled={!draft.trim()} loading={submitting}>
          Ask
        </Button>
      </div>
      <div className="mt-2 flex flex-wrap gap-2">
        {SUGGESTIONS.map((s) => (
          <button
            key={s}
            onClick={() => setDraft(s)}
            className="rounded-full border border-subtle px-3 py-1 text-11 text-secondary hover:border-accent-strong hover:text-primary"
          >
            {s}
          </button>
        ))}
      </div>
      {threads && threads.length > 0 && (
        <div className="mt-3 border-t border-subtle pt-2">
          <div className="mb-1 text-11 tracking-wide text-secondary uppercase">Recent</div>
          <div className="flex flex-col gap-1">
            {threads.slice(0, 5).map((t) => (
              <Link
                key={t.id}
                to={`/${slug}/assistant/${t.id}`}
                className="truncate text-12 text-primary hover:text-accent-primary"
              >
                {t.title || "Untitled conversation"}
              </Link>
            ))}
          </div>
        </div>
      )}
    </div>
  );
});
