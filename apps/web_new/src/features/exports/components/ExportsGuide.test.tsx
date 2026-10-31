// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Exports guide (EXP-001): the registered body composes the real request
// form above the real previous-exports history, both wired to the same
// workspace slug.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { expectContentKeysInert, expectKeyboardReachable, pressEnter, tabTo } from "../../../testSupport/keyboard.js";
import { ExportsGuide } from "./ExportsGuide.js";

const USER_ID = "11111111-1111-4111-8111-111111111111";

function projectRow(id: string, name: string, identifier: string) {
  return {
    id,
    name,
    identifier,
    member_role: 15,
    archived_at: null,
    workspace: "44444444-4444-4444-8444-444444444444",
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
  };
}

function historyRecord() {
  return {
    id: "12345678-1234-4234-8234-000000000001",
    created_at: new Date(Date.now() - 3_600_000).toISOString(),
    updated_at: new Date(Date.now() - 3_600_000).toISOString(),
    project: ["12345678-1234-4234-8234-000000000901"],
    provider: "csv",
    status: "completed",
    url: "https://files.test/export-1.csv",
    initiated_by: USER_ID,
    initiated_by_detail: {
      id: USER_ID,
      first_name: "Ana",
      last_name: "Siva",
      avatar: "",
      avatar_url: null,
      display_name: "Ana Siva",
      is_bot: false,
    },
    token: "token",
    created_by: null,
    updated_by: null,
  };
}

function historyPage() {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 1,
    next_cursor: "10:1:0",
    prev_cursor: "10:-1:1",
    next_page_results: false,
    prev_page_results: false,
    count: 1,
    total_pages: 1,
    total_results: 1,
    extra_stats: null,
    results: [historyRecord()],
  };
}

function answer(url: string, body: unknown) {
  return {
    status: 200,
    url,
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify(body),
  };
}

/** Stub transport answering the reads the form and history make. */
function stubTransport(requested: string[], posts: Array<{ url: string; body: unknown }> = []): void {
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string } = {}) => {
    requested.push(url);
    if (url.endsWith("/export-issues/") && init.method === "POST") {
      posts.push({ url, body: JSON.parse(init.body ?? "{}") });
      return answer(url, { message: "queued" });
    }
    if (url.endsWith("/auth/get-csrf-token/")) return answer(url, { csrf_token: "csrf" });
    if (url.endsWith("/projects/")) {
      return answer(url, [
        projectRow("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "Exp Alpha", "EA"),
        projectRow("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "Exp Beta", "EB"),
      ]);
    }
    if (url.includes("/export-issues")) return answer(url, historyPage());
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
}

function renderGuide(): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ExportsGuide workspaceSlug="acme" />
    </QueryClientProvider>
  );
}

afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("ExportsGuide", () => {
  it("renders the request form above the previous exports for the workspace", async () => {
    const requested: string[] = [];
    stubTransport(requested);
    renderGuide();

    expect(await screen.findByRole("heading", { level: 3, name: "Exporting project" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Export" })).toBeInTheDocument();
    expect(await screen.findByRole("heading", { level: 3, name: "Previous exports" })).toBeInTheDocument();
    expect(await screen.findByText("Ana Siva")).toBeInTheDocument();

    expect(requested.some((url) => url.endsWith("/projects/"))).toBe(true);
    expect(requested.some((url) => url.includes("/api/workspaces/acme/export-issues"))).toBe(true);
  });

  it("keeps the assembled page keyboard-operable end to end", async () => {
    const user = userEvent.setup();
    const requested: string[] = [];
    const posts: Array<{ url: string; body: unknown }> = [];
    stubTransport(requested, posts);
    renderGuide();
    await screen.findByRole("heading", { level: 3, name: "Previous exports" });

    // Both halves of the guide sit in one tab order, with visible focus.
    const guide = screen.getByRole("heading", { level: 3, name: "Exporting project" }).parentElement?.parentElement
      ?.parentElement as HTMLElement;
    expectKeyboardReachable(guide);

    await tabTo(user, screen.getByRole("button", { name: "All projects" }));
    await tabTo(user, screen.getByRole("button", { name: "CSV" }));
    const submit = screen.getByRole("button", { name: "Export" });
    await tabTo(user, submit);
    expect(submit).toHaveFocus();
    await pressEnter(user);
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]?.body).toEqual({ provider: "csv", project: [], multiple: false });

    await expectContentKeysInert(user, () => `${guide.textContent} ${posts.length}`);
    expect(posts).toHaveLength(1);
  });
});
