/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { Plus, Sparkles } from "lucide-react";
import { Link, useNavigate, useParams } from "react-router";
import useSWR from "swr";
import { AssistantService } from "@pi-dash/services";
import type { IAssistantThread } from "@pi-dash/types";
import { Button } from "@pi-dash/ui";

const service = new AssistantService();

const AssistantIndexPage = observer(function AssistantIndexPage() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const navigate = useNavigate();
  const { data: threads } = useSWR<IAssistantThread[]>(
    workspaceSlug ? ["assistant-threads", workspaceSlug] : null,
    () => service.listThreads(workspaceSlug!)
  );

  const newChat = async () => {
    if (!workspaceSlug) return;
    const thread = await service.createThread(workspaceSlug);
    navigate(`/${workspaceSlug}/assistant/${thread.id}`);
  };

  return (
    <div className="mx-auto w-full max-w-3xl px-4 py-8">
      <div className="mb-4 flex items-center justify-between">
        <div className="flex items-center gap-2 text-18 font-semibold text-primary">
          <Sparkles className="size-5 text-accent-primary" /> Pi Assistant
        </div>
        <Button onClick={newChat}>
          <Plus className="size-4" /> New chat
        </Button>
      </div>
      <div className="flex flex-col gap-1">
        {(threads ?? []).map((t) => (
          <Link
            key={t.id}
            to={`/${workspaceSlug}/assistant/${t.id}`}
            className="flex items-center justify-between rounded-md border border-subtle px-3 py-2 text-13 text-primary hover:border-accent-strong"
          >
            <span className="truncate">{t.title || "Untitled conversation"}</span>
            <span className="ml-3 shrink-0 text-11 text-secondary">{new Date(t.updated_at).toLocaleDateString()}</span>
          </Link>
        ))}
        {threads && threads.length === 0 && (
          <div className="py-12 text-center text-13 text-secondary">No conversations yet. Start a new chat above.</div>
        )}
      </div>
    </div>
  );
});

export default AssistantIndexPage;
