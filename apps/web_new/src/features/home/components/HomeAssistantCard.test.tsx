// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { AssistantThread, LlmConfig } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/index.js";
import { sessionKeys, setSessionClient } from "../../../core/session/queries.js";
import { homeKeys } from "../api/keys.js";
import { HomeAssistantCard } from "./HomeAssistantCard.js";

const THREAD_1: AssistantThread = {
  id: "11111111-1111-4111-8111-111111111111",
  title: "Sprint summary",
  is_archived: false,
  has_active_turn: false,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
};

const ARCHIVED_THREAD: AssistantThread = {
  ...THREAD_1,
  id: "22222222-2222-4222-8222-222222222222",
  title: "Old planning",
  is_archived: true,
};

const CONFIGURED: LlmConfig = {
  provider_kind: "openai",
  base_url: "https://api.test",
  model_name: "pilot",
  has_api_key: true,
  last_verified_at: null,
};

const UNCONFIGURED: LlmConfig = { ...CONFIGURED, model_name: "", has_api_key: false };

function workspaceRow(role: number): Record<string, unknown> {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    name: "Acme",
    slug: "acme",
    logo_url: null,
    total_members: 3,
    role,
  };
}

const requestedUrls: string[] = [];

function renderCard(options: {
  role?: number;
  seedWorkspaces?: boolean;
  threads?: AssistantThread[];
  config?: LlmConfig;
}): void {
  setSessionClient({
    getJson: () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  requestedUrls.length = 0;
  createAppClient("https://api.test", (async (url: string) => {
    requestedUrls.push(url);
    return {
      status: 500,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify({ error: "boom" }),
    };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.seedWorkspaces !== false) {
    queryClient.setQueryData(sessionKeys.workspaces, [workspaceRow(options.role ?? ROLE_MEMBER)]);
  }
  if (options.threads !== undefined) queryClient.setQueryData(homeKeys.threads("acme"), options.threads);
  if (options.config !== undefined) queryClient.setQueryData(homeKeys.assistantConfig("acme"), options.config);
  render(
    <QueryClientProvider client={queryClient}>
      <HomeAssistantCard workspaceSlug="acme" />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("HomeAssistantCard", () => {
  it("shows prompts and recent threads to configured members", async () => {
    renderCard({ threads: [THREAD_1, ARCHIVED_THREAD], config: CONFIGURED });
    const card = await screen.findByRole("region", { name: "Assistant" });
    expect(card).toHaveAttribute("data-assistant-state", "ready");
    expect(screen.getByRole("link", { name: "Summarize my open work" })).toHaveAttribute("href", "/acme/assistant");
    for (const suggestion of [
      "Summarize my open work",
      "Draft this week's status update",
      "What should I pick up next?",
      "Help me plan the week",
    ]) {
      expect(screen.getByRole("link", { name: suggestion })).toBeInTheDocument();
    }
    const threadLink = screen.getByRole("link", { name: "Sprint summary" });
    expect(threadLink).toHaveAttribute("href", `/acme/assistant/${THREAD_1.id}`);
    expect(screen.queryByRole("link", { name: "Old planning" })).not.toBeInTheDocument();
  });

  it("falls back to an untitled label for blank thread titles", async () => {
    renderCard({ threads: [{ ...THREAD_1, title: "" }], config: CONFIGURED });
    expect(await screen.findByRole("link", { name: "Untitled conversation" })).toBeInTheDocument();
  });

  it("shows prompts alone when no threads are recent", async () => {
    renderCard({ threads: [], config: CONFIGURED });
    const card = await screen.findByRole("region", { name: "Assistant" });
    expect(card).toHaveAttribute("data-assistant-state", "ready");
    expect(screen.queryByRole("heading", { name: "Recent conversations" })).not.toBeInTheDocument();
  });

  it("shows the setup reminder when no model is configured", async () => {
    renderCard({ threads: [], config: UNCONFIGURED });
    const card = await screen.findByRole("region", { name: "Assistant" });
    expect(card).toHaveAttribute("data-assistant-state", "setup");
    expect(screen.getByText(/connect a model/i)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Set up the assistant" })).toHaveAttribute("href", "/acme/assistant");
    expect(screen.queryByRole("link", { name: "Summarize my open work" })).not.toBeInTheDocument();
  });

  it("hides the card from guests without fetching", async () => {
    renderCard({ role: ROLE_GUEST, threads: [THREAD_1], config: CONFIGURED });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(requestedUrls).toEqual([]);
    expect(screen.queryByRole("region", { name: "Assistant" })).not.toBeInTheDocument();
  });

  it("renders nothing while permissions load", () => {
    renderCard({ seedWorkspaces: false });
    expect(screen.queryByRole("region", { name: "Assistant" })).not.toBeInTheDocument();
  });

  it("renders nothing when the threads read fails", async () => {
    renderCard({ config: CONFIGURED });
    await waitFor(() => {
      expect(requestedUrls.some((url) => url.includes("/ai-assistant/threads/"))).toBe(true);
    });
    expect(screen.queryByRole("region", { name: "Assistant" })).not.toBeInTheDocument();
  });

  it("renders nothing when the config read fails", async () => {
    renderCard({ threads: [THREAD_1] });
    await waitFor(() => {
      expect(requestedUrls.some((url) => url.includes("/ai-assistant/config/"))).toBe(true);
    });
    expect(screen.queryByRole("region", { name: "Assistant" })).not.toBeInTheDocument();
  });
});
