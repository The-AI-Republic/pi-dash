// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page assistant task call (parity/specs/pages.md §9.3, PAGE-075): the
// composed instruction rides `task`, the selection rides `prompt`, and
// the answer keeps the trimmed text plus the trimmed markup.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { jsonResponse } from "../testing.js";
import { requestPageAssistantAnswer, usePageAssistantAnswer } from "./assistantMutations.js";

let posts: { url: string; body: unknown }[] = [];

beforeEach(() => {
  posts = [];
  createAppClient("https://api.test", (async (url: string, init: { method: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (init.method === "POST") {
      posts.push({ url, body: init.body === undefined ? null : JSON.parse(init.body) });
      return jsonResponse(url, 200, { response: "  done  ", response_html: "  <p>done</p>  " });
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
});

describe("requestPageAssistantAnswer", () => {
  it("posts the task plus the selection and trims both answer forms", async () => {
    const { getAppClient } = await import("../../../core/api/client.js");
    const answer = await requestPageAssistantAnswer(getAppClient(), "acme", "Summarize", "chosen text");
    expect(posts).toEqual([
      {
        url: "https://api.test/api/workspaces/acme/ai-assistant/",
        body: { task: "Summarize", prompt: "chosen text" },
      },
    ]);
    expect(answer).toEqual({ text: "done", html: "<p>done</p>" });
  });
});

describe("usePageAssistantAnswer", () => {
  it("runs the task through the app client", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => usePageAssistantAnswer("acme"), { wrapper });
    result.current.mutate({ task: "Summarize", prompt: "chosen text" });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual({ text: "done", html: "<p>done</p>" });
    expect(posts).toHaveLength(1);
  });
});
