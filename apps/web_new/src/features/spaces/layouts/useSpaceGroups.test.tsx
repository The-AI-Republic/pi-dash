// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { useSpaceGroups } from "./useSpaceGroups.js";
import { boardIssue, DONE_ID, firstPage, STATES, TODO_ID } from "./testFixtures.js";

function reply(body: unknown, status = 200): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

function harness(handler: (url: string) => Promise<Awaited<ReturnType<Transport>>> | Awaited<ReturnType<Transport>>) {
  const urls: string[] = [];
  const client = createClient({
    baseUrl: "https://api.test",
    transport: async (url) => {
      urls.push(url);
      return await handler(url);
    },
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { client, urls, wrapper };
}

const FIRST = firstPage(
  {
    [TODO_ID]: { results: [boardIssue(1, TODO_ID, "First")], total_results: 3 },
    [DONE_ID]: { results: [boardIssue(2, DONE_ID, "Done one")], total_results: 1 },
  },
  { next_cursor: "50:1:0", next_page_results: true }
);

function nextPage(stateId: string, names: string[], total: number, overrides: Record<string, unknown> = {}) {
  return firstPage(
    { [stateId]: { results: names.map((name, index) => boardIssue(10 + index, stateId, name)), total_results: total } },
    { next_cursor: "50:2:0", next_page_results: false, ...overrides }
  );
}

describe("useSpaceGroups (SPC-026)", () => {
  it("builds the groups from the first page without fetching", () => {
    const { client, urls, wrapper } = harness(() => reply(FIRST));
    const { result } = renderHook(() => useSpaceGroups(client, "anchor-1", {}, STATES, FIRST), { wrapper });
    expect(result.current.groups.map((group) => group.state.name)).toEqual(["Todo", "Empty", "Done"]);
    expect(result.current.groups.map((group) => group.total)).toEqual([3, 0, 1]);
    expect(urls).toEqual([]);
    expect([...result.current.paging]).toEqual([]);
  });

  it("pages one group with its own cursor and leaves the others untouched", async () => {
    const { client, urls, wrapper } = harness((url) =>
      url.includes("state=") ? reply(nextPage(TODO_ID, ["Second", "Third"], 3)) : reply(FIRST)
    );
    const { result } = renderHook(() => useSpaceGroups(client, "anchor-1", {}, STATES, FIRST), { wrapper });
    act(() => result.current.loadMore(TODO_ID));
    expect([...result.current.paging]).toEqual([TODO_ID]);
    await waitFor(() => expect([...result.current.paging]).toEqual([]));
    const todo = result.current.groups.find((group) => group.stateId === TODO_ID);
    const done = result.current.groups.find((group) => group.stateId === DONE_ID);
    expect(todo?.issues.map((issue) => issue.name)).toEqual(["First", "Second", "Third"]);
    expect(todo?.total).toBe(3);
    expect(todo?.nextCursor).toBeNull();
    expect(done?.issues.map((issue) => issue.name)).toEqual(["Done one"]);
    expect(done?.total).toBe(1);
    const request = new URL(urls[0] ?? "", "https://api.test");
    expect(request.searchParams.get("cursor")).toBe("50:1:0");
    expect(request.searchParams.get("state")).toBe(TODO_ID);
  });

  it("never asks a group without a next page", async () => {
    const { client, urls, wrapper } = harness(() => reply(FIRST));
    const { result } = renderHook(() => useSpaceGroups(client, "anchor-1", {}, STATES, FIRST), { wrapper });
    act(() => result.current.loadMore(DONE_ID));
    act(() => result.current.loadMore("99999999-9999-4999-8999-999999999999"));
    await act(async () => undefined);
    expect(urls).toEqual([]);
    expect([...result.current.paging]).toEqual([]);
  });

  it("ignores a second trigger while the same page is in flight", async () => {
    let release: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { client, urls, wrapper } = harness(async (url) => {
      if (url.includes("state=")) {
        await gate;
        return reply(nextPage(TODO_ID, ["Second"], 3));
      }
      return reply(FIRST);
    });
    const { result } = renderHook(() => useSpaceGroups(client, "anchor-1", {}, STATES, FIRST), { wrapper });
    act(() => result.current.loadMore(TODO_ID));
    act(() => result.current.loadMore(TODO_ID));
    expect(urls).toHaveLength(1);
    release?.();
    await waitFor(() => expect([...result.current.paging]).toEqual([]));
    expect(urls).toHaveLength(1);
  });

  it("marks a failed group and clears the mark when it retries", async () => {
    let fail = true;
    const { client, urls, wrapper } = harness((url) => {
      if (!url.includes("state=")) return reply(FIRST);
      return fail ? reply({ detail: "down" }, 500) : reply(nextPage(TODO_ID, ["Second"], 3));
    });
    const { result } = renderHook(() => useSpaceGroups(client, "anchor-1", {}, STATES, FIRST), { wrapper });
    act(() => result.current.loadMore(TODO_ID));
    await waitFor(() => expect([...result.current.failed]).toEqual([TODO_ID]));
    expect([...result.current.paging]).toEqual([]);
    // The group keeps its cursor so the visitor can try again.
    expect(result.current.groups.find((group) => group.stateId === TODO_ID)?.nextCursor).toBe("50:1:0");
    fail = false;
    act(() => result.current.loadMore(TODO_ID));
    await waitFor(() =>
      expect(result.current.groups.find((group) => group.stateId === TODO_ID)?.issues).toHaveLength(2)
    );
    expect([...result.current.failed]).toEqual([]);
    expect(urls).toHaveLength(2);
  });

  it("drops loaded later pages when the first page changes under it", async () => {
    const { client, wrapper } = harness((url) =>
      url.includes("state=") ? reply(nextPage(TODO_ID, ["Second"], 3)) : reply(FIRST)
    );
    const { result, rerender } = renderHook(({ page }) => useSpaceGroups(client, "anchor-1", {}, STATES, page), {
      wrapper,
      initialProps: { page: FIRST },
    });
    act(() => result.current.loadMore(TODO_ID));
    await waitFor(() =>
      expect(result.current.groups.find((group) => group.stateId === TODO_ID)?.issues).toHaveLength(2)
    );
    const refetched = firstPage({ [TODO_ID]: { results: [boardIssue(20, TODO_ID, "Fresh")], total_results: 3 } });
    rerender({ page: refetched });
    expect(result.current.groups.find((group) => group.stateId === TODO_ID)?.issues.map((issue) => issue.name)).toEqual(
      ["Fresh"]
    );
    expect([...result.current.paging]).toEqual([]);
    expect([...result.current.failed]).toEqual([]);
  });

  it("carries the address filters onto a later page", async () => {
    const { client, urls, wrapper } = harness((url) =>
      url.includes("state=") ? reply(nextPage(TODO_ID, ["Second"], 3)) : reply(FIRST)
    );
    const { result } = renderHook(() => useSpaceGroups(client, "anchor-1", { priority: ["high"] }, STATES, FIRST), {
      wrapper,
    });
    act(() => result.current.loadMore(TODO_ID));
    await waitFor(() => expect(urls).toHaveLength(1));
    expect(new URL(urls[0] ?? "", "https://api.test").searchParams.get("priority")).toBe("high");
  });

  it("keeps the newest filters when the trigger re-renders", async () => {
    let release: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { client, urls, wrapper } = harness(async (url) => {
      if (url.includes("state=")) {
        await gate;
        return reply(nextPage(TODO_ID, ["Second"], 3));
      }
      return reply(FIRST);
    });
    const { result, rerender } = renderHook(
      ({ filters }) => useSpaceGroups(client, "anchor-1", filters, STATES, FIRST),
      { wrapper, initialProps: { filters: {} as { priority?: string[] } } }
    );
    act(() => result.current.loadMore(TODO_ID));
    rerender({ filters: { priority: ["low"] } });
    release?.();
    await waitFor(() => expect(urls).toHaveLength(1));
    // The in-flight page was requested with the filters that were live when
    // it started; the rerender must not start a second one.
    expect(urls).toHaveLength(1);
  });

  it("does not apply a page that lands after the first page was replaced", async () => {
    let release: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { client, wrapper } = harness(async (url) => {
      if (url.includes("state=")) {
        await gate;
        return reply(nextPage(TODO_ID, ["Stale"], 3));
      }
      return reply(FIRST);
    });
    const { result, rerender } = renderHook(({ page }) => useSpaceGroups(client, "anchor-1", {}, STATES, page), {
      wrapper,
      initialProps: { page: FIRST },
    });
    act(() => result.current.loadMore(TODO_ID));
    const refetched = firstPage({ [TODO_ID]: { results: [boardIssue(30, TODO_ID, "Fresh")], total_results: 3 } });
    rerender({ page: refetched });
    release?.();
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(result.current.groups.find((group) => group.stateId === TODO_ID)?.issues.map((issue) => issue.name)).toEqual(
      ["Fresh"]
    );
  });

  it("survives a failure that lands after the first page was replaced", async () => {
    let release: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { client, wrapper } = harness(async (url) => {
      if (url.includes("state=")) {
        await gate;
        return reply({ detail: "down" }, 500);
      }
      return reply(FIRST);
    });
    const { result, rerender } = renderHook(({ page }) => useSpaceGroups(client, "anchor-1", {}, STATES, page), {
      wrapper,
      initialProps: { page: FIRST },
    });
    act(() => result.current.loadMore(TODO_ID));
    const refetched = firstPage(
      { [TODO_ID]: { results: [boardIssue(31, TODO_ID, "Fresh")], total_results: 3 } },
      { next_cursor: "50:1:0", next_page_results: true }
    );
    rerender({ page: refetched });
    release?.();
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect([...result.current.failed]).toEqual([]);
    expect([...result.current.paging]).toEqual([]);
    // The fresh page's own cursor is intact, so a later trigger still works.
    expect(result.current.groups.find((group) => group.stateId === TODO_ID)?.nextCursor).toBe("50:1:0");
  });

  it("keeps another group's failure mark when this group retries", async () => {
    const page = firstPage(
      {
        [TODO_ID]: { results: [boardIssue(1, TODO_ID, "First")], total_results: 3 },
        [DONE_ID]: { results: [boardIssue(2, DONE_ID, "Done one")], total_results: 60 },
      },
      { next_cursor: "50:1:0", next_page_results: true }
    );
    let calls = 0;
    const { client, wrapper } = harness((url) => {
      if (!url.includes("state=")) return reply(page);
      calls += 1;
      if (calls === 1) return reply({ detail: "down" }, 500);
      return reply(nextPage(TODO_ID, ["Second"], 3));
    });
    const { result } = renderHook(() => useSpaceGroups(client, "anchor-1", {}, STATES, page), { wrapper });
    act(() => result.current.loadMore(DONE_ID));
    await waitFor(() => expect([...result.current.failed]).toEqual([DONE_ID]));
    act(() => result.current.loadMore(TODO_ID));
    await waitFor(() => expect([...result.current.paging]).toEqual([]));
    expect([...result.current.failed]).toEqual([DONE_ID]);
  });

  it("resets to the new page when a stale trigger fires after a reload", async () => {
    const { client, urls, wrapper } = harness(() => reply(nextPage(TODO_ID, ["Late"], 3)));
    const { result, rerender } = renderHook(({ page }) => useSpaceGroups(client, "anchor-1", {}, STATES, page), {
      wrapper,
      initialProps: { page: FIRST },
    });
    const stale = result.current.loadMore;
    const refetched = firstPage(
      { [TODO_ID]: { results: [boardIssue(50, TODO_ID, "Fresh")], total_results: 3 } },
      { next_cursor: "50:1:0", next_page_results: true }
    );
    rerender({ page: refetched });
    act(() => stale(TODO_ID));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    // The stale trigger still asks the server (the group has a next page),
    // but it runs against the reloaded page: the fresh rows survive and the
    // in-flight mark belongs to the new page's own state.
    expect(urls).toHaveLength(1);
    const todo = result.current.groups.find((group) => group.stateId === TODO_ID);
    expect(todo?.issues[0]?.name).toBe("Fresh");
    expect(todo?.issues.length).toBeLessThanOrEqual(3);
  });

  it("marks a failure that lands for the current page", async () => {
    const { client, wrapper } = harness((url) =>
      url.includes("state=") ? reply({ detail: "down" }, 500) : reply(FIRST)
    );
    const { result } = renderHook(() => useSpaceGroups(client, "anchor-1", {}, STATES, FIRST), { wrapper });
    act(() => result.current.loadMore(TODO_ID));
    await waitFor(() => expect([...result.current.failed]).toEqual([TODO_ID]));
  });
});
