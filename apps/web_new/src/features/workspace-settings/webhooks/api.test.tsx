// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook reads and writes (parity/specs/workspace-settings.md §18-§24,
// WSET-019-024/041): the URLs and payloads, the list upsert, and the
// cache invariants the pages rely on after each write.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { ApiError, type Transport } from "@pidash/api-client";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  isMissingWebhookFailure,
  useCreateWebhook,
  useDeleteWebhook,
  useRegenerateWebhookSecret,
  useSetWebhookActive,
  useUpdateWebhook,
  useWebhookDetail,
  useWebhooksList,
  upsertListRow,
  webhookKeys,
  webhookRowOf,
  webhookDetailQueryOptions,
  webhooksListQueryOptions,
} from "./api.js";

const SLUG = "acme";
const ID = "223e4567-e89b-12d3-a456-426614174001";
const OTHER = "223e4567-e89b-12d3-a456-426614174002";

function row(overrides: Record<string, unknown> = {}) {
  return {
    id: ID,
    url: "https://example.com/post",
    is_active: true,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    project: true,
    issue: true,
    cycle: true,
    module: true,
    issue_comment: true,
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

function transport(handler: (call: Call) => { status: number; body?: unknown }): {
  calls: Call[];
  transport: Transport;
} {
  const calls: Call[] = [];
  return {
    calls,
    transport: async (url, init) => {
      const call: Call = {
        method: init?.method ?? "GET",
        url,
        body: typeof init?.body === "string" ? JSON.parse(init.body) : undefined,
      };
      calls.push(call);
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      const answer = handler(call);
      return {
        status: answer.status,
        url,
        headers: { get: () => null },
        text: async () => (answer.body === undefined ? "" : JSON.stringify(answer.body)),
      };
    },
  };
}

function harness(clientTransport: Transport) {
  const client = createAppClient("https://api.test", clientTransport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children?: unknown }): React.ReactElement => (
    <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>
  );
  return { client, queryClient, wrapper };
}

afterEach(() => {
  resetAppClient();
});

describe("webhook query options", () => {
  it("reads the list under the workspace scope", async () => {
    const stub = transport(() => ({ status: 200, body: [row()] }));
    const { client } = harness(stub.transport);
    await expect(webhooksListQueryOptions(client, SLUG).queryFn?.({} as never)).resolves.toEqual([row()]);
    expect(stub.calls.at(-1)).toMatchObject({
      method: "GET",
      url: `https://api.test/api/workspaces/${SLUG}/webhooks/`,
    });
    expect(webhookKeys.list(SLUG)).toEqual(["ws", SLUG, "workspace-settings", "webhooks", "list"]);
  });

  it("drops a read-only secret echo from the parsed row", async () => {
    const stub = transport(() => ({ status: 200, body: row({ secret_key: "leaked" }) }));
    const { client } = harness(stub.transport);
    const parsed = await webhookDetailQueryOptions(client, SLUG, ID).queryFn?.({} as never);
    expect(parsed).not.toHaveProperty("secret_key");
  });

  it("keeps a 404 detail read from retrying", async () => {
    const options = webhookDetailQueryOptions({} as never, SLUG, ID);
    const retry = options.retry;
    if (typeof retry !== "function") throw new Error("detail reads should decide retries per attempt");
    expect(retry(0, new ApiError({ status: 404, code: "missing", message: "gone" }))).toBe(false);
    expect(retry(0, new ApiError({ status: 500, code: "server", message: "boom" }))).toBe(true);
    expect(retry(1, new ApiError({ status: 500, code: "server", message: "boom" }))).toBe(false);
    expect(isMissingWebhookFailure({ status: 404 })).toBe(true);
    expect(isMissingWebhookFailure(new Error("x"))).toBe(false);
    expect(isMissingWebhookFailure(null)).toBe(false);
  });
});

describe("list cache", () => {
  it("replaces an existing row in place", () => {
    const rows = [row() as never, row({ id: OTHER }) as never];
    expect(upsertListRow(rows, row({ url: "https://example.com/new" }) as never).map((entry) => entry.url)).toEqual([
      "https://example.com/new",
      "https://example.com/post",
    ]);
  });

  it("prepends a row the list does not have", () => {
    const next = upsertListRow([row() as never], row({ id: OTHER }) as never);
    expect(next.map((entry) => entry.id)).toEqual([OTHER, ID]);
  });

  it("strips the secret from a create answer", () => {
    const stored = webhookRowOf(row({ secret_key: "fresh" }) as never);
    expect(stored).not.toHaveProperty("secret_key");
    expect(stored.id).toBe(ID);
  });
});

describe("webhook hooks", () => {
  it("reads the list and the detail through the client", async () => {
    const stub = transport((call) => ({ status: 200, body: call.url.endsWith("/webhooks/") ? [row()] : row() }));
    const { wrapper } = harness(stub.transport);
    const list = renderHook(() => useWebhooksList(SLUG), { wrapper });
    const detail = renderHook(() => useWebhookDetail(SLUG, ID), { wrapper });
    await waitFor(() => expect(list.result.current.isSuccess).toBe(true));
    await waitFor(() => expect(detail.result.current.isSuccess).toBe(true));
    expect(list.result.current.data).toEqual([row()]);
    expect(detail.result.current.data?.id).toBe(ID);
    expect(stub.calls.some((call) => call.url.endsWith(`/webhooks/${ID}/`))).toBe(true);
  });

  it("creates, then puts the new row at the head of the list cache", async () => {
    const issued = row({ id: OTHER, url: "https://example.com/new", secret_key: "whsec_fixture_new" });
    const stub = transport((call) => ({
      status: call.method === "POST" ? 201 : 200,
      body: call.method === "POST" ? issued : [],
    }));
    const { queryClient, wrapper } = harness(stub.transport);
    queryClient.setQueryData(webhookKeys.list(SLUG), [row()]);
    const keys: string[] = [];
    const create = renderHook(() => useCreateWebhook(SLUG, (answer) => keys.push(answer.secret_key)), { wrapper });
    await act(async () => {
      await create.result.current.mutateAsync({
        url: "https://example.com/new",
        project: true,
        issue: true,
        cycle: true,
        module: true,
        issue_comment: true,
      });
    });
    expect(stub.calls.at(-1)).toMatchObject({
      method: "POST",
      url: `https://api.test/api/workspaces/${SLUG}/webhooks/`,
      body: {
        url: "https://example.com/new",
        project: true,
        issue: true,
        cycle: true,
        module: true,
        issue_comment: true,
      },
    });
    const cached = queryClient.getQueryData<Array<{ id: string; secret_key?: string }>>(webhookKeys.list(SLUG));
    expect(cached?.map((entry) => entry.id)).toEqual([OTHER, ID]);
    expect(cached?.some((entry) => entry.secret_key !== undefined)).toBe(false);
    // The key reaches the caller out of band and never enters the cache.
    expect(keys).toEqual(["whsec_fixture_new"]);
    expect(
      JSON.stringify(
        queryClient
          .getMutationCache()
          .getAll()
          .map((m) => m.state.data)
      )
    ).not.toContain("whsec_fixture_new");
  });

  it("saves a detail edit into both caches", async () => {
    const saved = row({ url: "https://example.com/updated", is_active: false });
    const stub = transport(() => ({ status: 200, body: saved }));
    const { queryClient, wrapper } = harness(stub.transport);
    queryClient.setQueryData(webhookKeys.list(SLUG), [row()]);
    const update = renderHook(() => useUpdateWebhook(SLUG), { wrapper });
    await act(async () => {
      await update.result.current.mutateAsync({ webhookId: ID, input: { is_active: false } });
    });
    expect(stub.calls.at(-1)).toMatchObject({
      method: "PATCH",
      url: `https://api.test/api/workspaces/${SLUG}/webhooks/${ID}/`,
      body: { is_active: false },
    });
    expect(queryClient.getQueryData(webhookKeys.detail(SLUG, ID))).toEqual(saved);
    expect(queryClient.getQueryData(webhookKeys.list(SLUG))).toEqual([saved]);
  });

  it("sends only the active flag from the row switch", async () => {
    const stub = transport(() => ({ status: 200, body: row({ is_active: false }) }));
    const { wrapper } = harness(stub.transport);
    const toggle = renderHook(() => useSetWebhookActive(SLUG), { wrapper });
    await act(async () => {
      await toggle.result.current.mutateAsync({ webhookId: ID, input: { is_active: false } });
    });
    expect(stub.calls.at(-1)?.body).toEqual({ is_active: false });
  });

  it("drops a deleted row and its detail cache", async () => {
    const stub = transport(() => ({ status: 204 }));
    const { queryClient, wrapper } = harness(stub.transport);
    queryClient.setQueryData(webhookKeys.list(SLUG), [row(), row({ id: OTHER })]);
    queryClient.setQueryData(webhookKeys.detail(SLUG, ID), row());
    const remove = renderHook(() => useDeleteWebhook(SLUG), { wrapper });
    await act(async () => {
      await remove.result.current.mutateAsync(ID);
    });
    expect(stub.calls.at(-1)).toMatchObject({
      method: "DELETE",
      url: `https://api.test/api/workspaces/${SLUG}/webhooks/${ID}/`,
    });
    expect(queryClient.getQueryData(webhookKeys.detail(SLUG, ID))).toBeUndefined();
    expect(
      (queryClient.getQueryData<Array<{ id: string }>>(webhookKeys.list(SLUG)) ?? []).map((entry) => entry.id)
    ).toEqual([OTHER]);
  });

  it("keeps an uncached list uncached after a delete and a rotate", async () => {
    const stub = transport((call) => ({
      status: call.method === "DELETE" ? 204 : 200,
      ...(call.method === "DELETE" ? {} : { body: row({ secret_key: "fresh" }) }),
    }));
    const { queryClient, wrapper } = harness(stub.transport);
    const remove = renderHook(() => useDeleteWebhook(SLUG), { wrapper });
    await act(async () => {
      await remove.result.current.mutateAsync(ID);
    });
    expect(queryClient.getQueryData(webhookKeys.list(SLUG))).toBeUndefined();

    const keys: string[] = [];
    const rotate = renderHook(() => useRegenerateWebhookSecret(SLUG, (answer) => keys.push(answer.secret_key)), {
      wrapper,
    });
    let issued: { row: { id: string } } | undefined;
    await act(async () => {
      issued = await rotate.result.current.mutateAsync(ID);
    });
    expect(keys).toEqual(["fresh"]);
    expect(issued?.row.id).toBe(ID);
    expect(
      JSON.stringify(
        queryClient
          .getMutationCache()
          .getAll()
          .map((m) => m.state.data)
      )
    ).not.toContain("fresh");
    expect(stub.calls.at(-1)).toMatchObject({
      method: "POST",
      url: `https://api.test/api/workspaces/${SLUG}/webhooks/${ID}/regenerate/`,
    });
    expect(queryClient.getQueryData(webhookKeys.detail(SLUG, ID))).toEqual(row());
    expect(queryClient.getQueryState(webhookKeys.list(SLUG))).toBeUndefined();
  });
});
