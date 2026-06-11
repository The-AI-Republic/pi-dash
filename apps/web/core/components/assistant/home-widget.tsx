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
import { AssistantService } from "@pi-dash/services";
import { EUserWorkspaceRoles, type IAssistantThread } from "@pi-dash/types";
import { Button } from "@pi-dash/ui";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new AssistantService();

const SUGGESTIONS = ["What's assigned to me?", "Create an issue in…", "Summarize open issues in…"];

export const AssistantHomeWidget = observer(function AssistantHomeWidget() {
  const { currentWorkspace } = useWorkspace();
  const navigate = useNavigate();
  const [draft, setDraft] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const slug = currentWorkspace?.slug;
  const role = currentWorkspace?.role ?? 0;

  const { data: threads } = useSWR<IAssistantThread[]>(
    slug && role >= EUserWorkspaceRoles.MEMBER ? ["assistant-threads", slug] : null,
    () => service.listThreads(slug!)
  );

  // Guests do not get the assistant (parity with the backend 403).
  if (!slug || role < EUserWorkspaceRoles.MEMBER) return null;

  const start = async (text: string) => {
    const content = text.trim();
    if (!content || submitting) return;
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
        <Sparkles className="size-4 text-accent-primary" /> Pi Assistant
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
