// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline dependency writes (ISS-237): a committed drag-link posts the
// finish-to-start edge and a refused write leaves a notice.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { GANTT_DEPENDENCIES_FLAG } from "../edition/flags.js";
import { useGanttDependencies } from "./useGanttDependencies.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const A = "a23e4567-e89b-12d3-a456-426614174001";
const B = "b23e4567-e89b-12d3-a456-426614174002";

const calls: { method: string; url: string }[] = [];
let postStatus = 201;

function serve(): void {
  createAppClient("https://api.test", (async (url: string, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "c" }) };
    }
    calls.push({ method, url });
    if (method === "POST") {
      return {
        status: postStatus,
        url,
        headers: { get: () => null },
        text: async () => (postStatus < 400 ? JSON.stringify([]) : JSON.stringify({ error: "refused" })),
      };
    }
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () =>
        JSON.stringify({
          blocking: [],
          blocked_by: [],
          duplicate: [],
          relates_to: [],
          start_after: [],
          start_before: [],
          finish_after: [],
          finish_before: [],
        }),
    };
  }) as never);
}

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const { result } = renderHook(
    () => useGanttDependencies({ workspaceSlug: SLUG, projectId: PROJECT, rows: [{ id: A, unconfirmed: false }] }),
    { wrapper: wrapper(client) }
  );
  return { result, client };
}

beforeEach(() => {
  calls.length = 0;
  postStatus = 201;
  serve();
});
afterEach(() => {
  resetAppClient();
  delete edition.flags[GANTT_DEPENDENCIES_FLAG];
  vi.clearAllMocks();
});

describe("useGanttDependencies link", () => {
  it("posts the edge from the waiting item and notices the link", async () => {
    edition.flags[GANTT_DEPENDENCIES_FLAG] = true;
    const { result } = mount();
    result.current.link({ fromId: A, toId: B });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Dependency linked"));
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toContain(`/issues/${B}/issue-relation/`);
  });

  it("notices a refused link", async () => {
    postStatus = 500;
    const { result } = mount();
    result.current.link({ fromId: A, toId: B });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't link the dependency", expect.any(String)));
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("stays off inside the community build", () => {
    const { result } = mount();
    expect(result.current.enabled).toBe(false);
  });
});
