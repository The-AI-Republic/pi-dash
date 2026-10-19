// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../core/api/client.js";

vi.mock("../../../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../../../shared/shell/index.js";
import { CodeReviewsWidget } from "./CodeReviewsWidget.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "323e4567-e89b-12d3-a456-426614174002";
const LINK_ONE = "423e4567-e89b-12d3-a456-426614174003";
const LINK_TWO = "523e4567-e89b-12d3-a456-426614174004";

function reviewRow(overrides: Record<string, unknown> = {}) {
  return {
    id: LINK_ONE,
    issue: ISSUE,
    provider: "github",
    host_url: "https://github.com",
    namespace: "octocat",
    repo_name: "hello",
    repo_external_id: "1",
    external_id: "2",
    external_iid: "1",
    url: "https://github.com/octocat/hello/pull/1",
    title: "octocat/hello#1",
    state: "open",
    merged: false,
    draft: false,
    remote_updated_at: null,
    metadata: {},
    created_at: "2026-10-18T00:00:00Z",
    updated_at: "2026-10-18T00:00:00Z",
    created_by: null,
    created_by_detail: null,
    ...overrides,
  };
}

interface StubOptions {
  rows?: Record<string, unknown>[];
  postStatus?: number;
  deleteStatus?: number;
}

function stubTransport(options: StubOptions = {}) {
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
    if (url.includes(`/issues/${ISSUE}/code-reviews/`)) {
      const method = init.method ?? "GET";
      if (method === "GET") return json(200, options.rows ?? []);
      if (method === "POST") {
        if (options.postStatus !== undefined) return json(options.postStatus, { error: "bad url" });
        const body = JSON.parse(init.body ?? "{}") as { url: string };
        return json(201, reviewRow({ id: LINK_TWO, url: body.url, title: "octocat/hello#2" }));
      }
      if (method === "DELETE") return json(options.deleteStatus ?? 204, {});
    }
    throw new Error(`unexpected ${init.method ?? "GET"} ${url}`);
  }) as never);
  return { calls };
}

function setup(options: StubOptions = {}, overrides: Partial<React.ComponentProps<typeof CodeReviewsWidget>> = {}) {
  const stub = stubTransport(options);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <CodeReviewsWidget workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} editable {...overrides} />
    </QueryClientProvider>
  );
  return stub;
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => resetAppClient());

describe("CodeReviewsWidget", () => {
  it("stays unmounted with no linked reviews", async () => {
    const { calls } = setup();
    await waitFor(() => expect(calls.some((call) => call.url.includes("/code-reviews/"))).toBe(true));
    expect(screen.queryByText("Code reviews")).not.toBeInTheDocument();
    expect(screen.queryByPlaceholderText("Paste a pull request or merge request URL")).not.toBeInTheDocument();
  });

  it("renders the badge, new-tab title and detach control", async () => {
    setup({
      rows: [reviewRow(), reviewRow({ id: LINK_TWO, title: "octocat/hello#2", merged: true, state: "merged" })],
    });
    expect(await screen.findByText("Code reviews")).toBeInTheDocument();
    const link = screen.getByRole("link", { name: "octocat/hello#1" });
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("href", "https://github.com/octocat/hello/pull/1");
    expect(screen.getByText("Open")).toBeInTheDocument();
    expect(screen.getByText("Merged")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Detach octocat/hello#1" })).toBeInTheDocument();
  });

  it("trims, submits and clears the pasted URL on success", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ rows: [reviewRow()] });
    const input = await screen.findByPlaceholderText("Paste a pull request or merge request URL");
    await user.type(input, "  https://github.com/octocat/hello/pull/2  ");
    await user.click(screen.getByRole("button", { name: "Attach" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Code review attached"));
    const post = calls.find((call) => call.method === "POST");
    expect(JSON.parse(post?.body ?? "{}")).toEqual({ url: "https://github.com/octocat/hello/pull/2" });
    await waitFor(() => expect(input).toHaveValue(""));
  });

  it("keeps the value and surfaces the server error when refused", async () => {
    const user = userEvent.setup();
    setup({ rows: [reviewRow()], postStatus: 400 });
    const input = await screen.findByPlaceholderText("Paste a pull request or merge request URL");
    await user.type(input, "https://example.com/not-a-review");
    await user.click(screen.getByRole("button", { name: "Attach" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Code review not attached"));
    expect(input).toHaveValue("https://example.com/not-a-review");
  });

  it("blocks an empty submit without a request", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ rows: [reviewRow()] });
    await screen.findByPlaceholderText("Paste a pull request or merge request URL");
    await user.click(screen.getByRole("button", { name: "Attach" }));
    expect(calls.some((call) => call.method === "POST")).toBe(false);
  });

  it("detaches a row and reports a refusal", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ rows: [reviewRow()], deleteStatus: 403 });
    await user.click(await screen.findByRole("button", { name: "Detach octocat/hello#1" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't detach the code review"));
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes(`/code-reviews/${LINK_ONE}/`))).toBe(
      true
    );
  });

  it("hides attach and detach for read-only viewers", async () => {
    setup({ rows: [reviewRow()] }, { editable: false });
    expect(await screen.findByRole("link", { name: "octocat/hello#1" })).toBeInTheDocument();
    expect(screen.queryByPlaceholderText("Paste a pull request or merge request URL")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Detach/ })).not.toBeInTheDocument();
  });

  it("collapses the section body from its header", async () => {
    const user = userEvent.setup();
    setup({ rows: [reviewRow()] });
    await screen.findByRole("link", { name: "octocat/hello#1" });
    await user.click(screen.getByRole("button", { name: /Code reviews/ }));
    expect(screen.queryByRole("link", { name: "octocat/hello#1" })).not.toBeInTheDocument();
  });
});
