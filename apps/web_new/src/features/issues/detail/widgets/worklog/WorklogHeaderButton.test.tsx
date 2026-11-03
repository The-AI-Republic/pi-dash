// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../core/api/client.js";

vi.mock("../../../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../../../shared/shell/index.js";
import { WorklogHeaderButton } from "./WorklogHeaderButton.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "323e4567-e89b-12d3-a456-426614174002";
const ENTRY = "423e4567-e89b-12d3-a456-426614174003";

function entryRow(overrides: Record<string, unknown> = {}) {
  return {
    id: ENTRY,
    issue: ISSUE,
    project: PROJECT,
    workspace: PROJECT,
    actor: null,
    actor_detail: null,
    minutes: 90,
    logged_on: "2026-10-20",
    note: "",
    created_at: "2026-10-20T22:00:00Z",
    updated_at: "2026-10-20T22:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

function stubTransport(options: { postStatus?: number } = {}) {
  const calls: { url: string; method: string; body: string }[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (url.includes(`/issues/${ISSUE}/worklogs/`) && (init.method ?? "GET") === "POST") {
      if (options.postStatus !== undefined) return json(options.postStatus, { error: "refused" });
      const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      return json(201, entryRow(body));
    }
    throw new Error(`unexpected ${init.method ?? "GET"} ${url}`);
  }) as never);
  return { calls };
}

function setup() {
  const stub = stubTransport();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  render(
    <QueryClientProvider client={queryClient}>
      <WorklogHeaderButton workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} />
    </QueryClientProvider>
  );
  return { ...stub, invalidate };
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => resetAppClient());

describe("WorklogHeaderButton", () => {
  it("logs time through the dialog and closes on success", async () => {
    const user = userEvent.setup();
    const { calls, invalidate } = setup();

    await user.click(screen.getByRole("button", { name: "Log time" }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText("Minutes"), "90");
    await user.clear(within(dialog).getByLabelText("Date"));
    await user.type(within(dialog).getByLabelText("Date"), "2026-10-20");
    await user.click(within(dialog).getByRole("button", { name: "Log time" }));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Time logged"));
    expect(screen.queryByRole("dialog")).toBeNull();
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/${ISSUE}/worklogs/`);
    expect(JSON.parse(post?.body ?? "{}")).toEqual({ minutes: 90, logged_on: "2026-10-20", note: "" });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["ws", SLUG, "issues", "activity", PROJECT, ISSUE],
    });
  });

  it("keeps the dialog open with its values when the server refuses", async () => {
    const user = userEvent.setup();
    stubTransport({ postStatus: 400 });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <WorklogHeaderButton workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} />
      </QueryClientProvider>
    );

    await user.click(screen.getByRole("button", { name: "Log time" }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText("Minutes"), "30");
    await user.click(within(dialog).getByRole("button", { name: "Log time" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't log time"));
    expect(screen.getByRole("dialog")).toBeDefined();
    expect((within(dialog).getByLabelText("Minutes") as HTMLInputElement).value).toBe("30");
  });

  it("dismisses the dialog with Escape", async () => {
    const user = userEvent.setup();
    setup();

    await user.click(screen.getByRole("button", { name: "Log time" }));
    await screen.findByRole("dialog");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
