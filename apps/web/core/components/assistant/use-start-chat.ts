/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useState } from "react";
import { useNavigate } from "react-router";
import { useSWRConfig } from "swr";
import { setToast, TOAST_TYPE } from "@pi-dash/propel/toast";
import { AssistantService } from "@pi-dash/services";
import type { IAssistantThread } from "@pi-dash/types";

const service = new AssistantService();

/**
 * Starts a new conversation from a first message: creates the thread
 * (untitled — the backend titles it from the first message), then navigates
 * to it with the draft as router state so the thread view sends it through
 * useAssistantChat.send(). Sending from the thread view arms the busy/poll
 * fallback and keeps send failures (missing key, too long, throttled) inside
 * the thread instead of orphaning a new thread per retry.
 */
export function useStartAssistantChat(slug: string | undefined) {
  const navigate = useNavigate();
  const { mutate } = useSWRConfig();
  const [starting, setStarting] = useState(false);

  const start = useCallback(
    async (content: string) => {
      const text = content.trim();
      if (!text || !slug || starting) return;
      setStarting(true);
      try {
        const thread = await service.createThread(slug);
        // Show the thread in the history panel immediately; the revalidation
        // after the first send picks up the server-assigned title.
        await mutate(["assistant-threads", slug], (prev: IAssistantThread[] | undefined) => [thread, ...(prev ?? [])], {
          revalidate: false,
        });
        navigate(`/${slug}/assistant/${thread.id}`, { state: { pendingMessage: text } });
      } catch (e: unknown) {
        const err = e as { error?: string; detail?: string } | null;
        setToast({
          type: TOAST_TYPE.ERROR,
          title: "Unable to start chat",
          message: err?.detail || err?.error || "Something went wrong. Please try again.",
        });
      } finally {
        setStarting(false);
      }
    },
    [slug, starting, navigate, mutate]
  );

  return { start, starting };
}
