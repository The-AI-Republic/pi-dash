// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Warm-up on open (NEWFRONT-549, RUN-028): warm an open session, create
// then warm when nothing is selected, never replace picked history, never
// touch offline/revoked runners, and de-duplicate per runner.
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ApiClient } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({ createChatSession: vi.fn(), warmChatSession: vi.fn() }));
vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { chatKeys } = await import("./keys.js");
const { makeRunner, makeSession, testClient, wrapper } = await import("./harness.js");
const { useWarmUp } = await import("./useWarmUp.js");

beforeEach(() => {
  createAppClient("https://api.test", {} as never);
  contracts.createChatSession.mockResolvedValue(makeSession({ id: "created" }));
  contracts.warmChatSession.mockResolvedValue({ ok: true });
});

afterEach(() => {
  contracts.createChatSession.mockReset();
  contracts.warmChatSession.mockReset();
  resetAppClient();
  void ({} as ApiClient);
});

function base(overrides: Record<string, unknown> = {}) {
  return {
    workspaceSlug: "acme",
    workspaceId: "ws-id",
    runnerId: "r-1",
    runner: makeRunner(),
    sessions: [],
    session: null,
    pendingId: null,
    requestedId: null,
    enabled: true,
    onCreated: vi.fn(),
    ...overrides,
  };
}

describe("useWarmUp", () => {
  it("warms an open session exactly once", async () => {
    const client = testClient();
    const input = base({ session: makeSession({ id: "open-1" }) });
    const { rerender } = renderHook(() => useWarmUp(input), { wrapper: wrapper(client) });
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledTimes(1));
    expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "open-1");
    rerender();
    await act(async () => undefined);
    expect(contracts.warmChatSession).toHaveBeenCalledTimes(1);
    expect(contracts.createChatSession).not.toHaveBeenCalled();
  });

  it("creates a session when none is selected, publishes it, then warms it", async () => {
    const client = testClient();
    const onCreated = vi.fn();
    const { result } = renderHook(() => useWarmUp(base({ onCreated })), { wrapper: wrapper(client) });
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "created"));
    expect(contracts.createChatSession).toHaveBeenCalledWith(expect.anything(), { workspace: "ws-id", runner: "r-1" });
    expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ id: "created" }));
    expect(client.getQueryData(chatKeys.sessions("acme", "r-1"))).toEqual([expect.objectContaining({ id: "created" })]);
    expect(result.current).toBe(false);
  });

  it("creates once per runner even across re-renders", async () => {
    const client = testClient();
    const input = base();
    const { rerender } = renderHook(() => useWarmUp(input), { wrapper: wrapper(client) });
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledTimes(1));
    rerender();
    rerender();
    await act(async () => undefined);
    expect(contracts.createChatSession).toHaveBeenCalledTimes(1);
  });

  it("starts fresh when the contact changes", async () => {
    const client = testClient();
    const first = base();
    const { rerender } = renderHook((props: Record<string, unknown>) => useWarmUp(props as never), {
      wrapper: wrapper(client),
      initialProps: first,
    });
    await waitFor(() => expect(contracts.createChatSession).toHaveBeenCalledTimes(1));
    rerender(base({ runnerId: "r-2", runner: makeRunner({ id: "r-2" }) }));
    await waitFor(() => expect(contracts.createChatSession).toHaveBeenCalledTimes(2));
    expect(contracts.createChatSession.mock.calls[1]?.[1]).toMatchObject({ runner: "r-2" });
  });

  it("never warms or creates for offline and revoked runners", async () => {
    const client = testClient();
    const offline = base({ runner: makeRunner({ status: "offline" }), session: makeSession() });
    const { rerender } = renderHook((props: Record<string, unknown>) => useWarmUp(props as never), {
      wrapper: wrapper(client),
      initialProps: offline,
    });
    await act(async () => undefined);
    expect(contracts.warmChatSession).not.toHaveBeenCalled();
    expect(contracts.createChatSession).not.toHaveBeenCalled();
    rerender(base({ runner: makeRunner({ status: "revoked" }) }));
    await act(async () => undefined);
    expect(contracts.createChatSession).not.toHaveBeenCalled();
  });

  it("leaves a picked closed history session alone", async () => {
    const client = testClient();
    renderHook(() => useWarmUp(base({ session: makeSession({ id: "old", status: "closed" }), pendingId: "old" })), {
      wrapper: wrapper(client),
    });
    await act(async () => undefined);
    expect(contracts.warmChatSession).not.toHaveBeenCalled();
    expect(contracts.createChatSession).not.toHaveBeenCalled();
  });

  it("does nothing while the session list is still unknown or access is denied", async () => {
    const client = testClient();
    const { rerender } = renderHook((props: Record<string, unknown>) => useWarmUp(props as never), {
      wrapper: wrapper(client),
      initialProps: base({ sessions: undefined }),
    });
    await act(async () => undefined);
    rerender(base({ enabled: false }));
    await act(async () => undefined);
    expect(contracts.createChatSession).not.toHaveBeenCalled();
  });

  it("swallows create and warm failures and clears the working state", async () => {
    contracts.createChatSession.mockRejectedValueOnce(new Error("offline"));
    const client = testClient();
    const { result } = renderHook(() => useWarmUp(base()), { wrapper: wrapper(client) });
    await waitFor(() => expect(result.current).toBe(false));
    expect(contracts.warmChatSession).not.toHaveBeenCalled();

    contracts.createChatSession.mockResolvedValueOnce(makeSession({ id: "created-2" }));
    contracts.warmChatSession.mockRejectedValueOnce(new Error("cold"));
    const second = renderHook(() => useWarmUp(base({ runnerId: "r-3", runner: makeRunner({ id: "r-3" }) })), {
      wrapper: wrapper(testClient()),
    });
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenLastCalledWith(expect.anything(), "created-2"));
    await waitFor(() => expect(second.result.current).toBe(false));
  });

  it("drops a create that resolves after the contact changed", async () => {
    const client = testClient();
    let resolveCreate: (value: ReturnType<typeof makeSession>) => void = () => undefined;
    contracts.createChatSession.mockImplementationOnce(
      () => new Promise((resolve) => (resolveCreate = resolve as never))
    );
    const onCreated = vi.fn();
    const { rerender, unmount } = renderHook((props: Record<string, unknown>) => useWarmUp(props as never), {
      wrapper: wrapper(client),
      initialProps: base({ onCreated }),
    });
    rerender(base({ onCreated, runnerId: "r-9", runner: makeRunner({ id: "r-9" }) }));
    await act(async () => {
      resolveCreate(makeSession({ id: "late" }));
    });
    expect(onCreated).not.toHaveBeenCalledWith(expect.objectContaining({ id: "late" }));
    unmount();
  });

  it("treats an unknown runner and a status-less session as not yet resolvable", async () => {
    const client = testClient();
    const input = base({
      runner: undefined,
      session: { ...makeSession({ id: "open-2" }), status: undefined },
    });
    renderHook(() => useWarmUp(input as never), { wrapper: wrapper(client) });
    await act(async () => undefined);
    expect(contracts.warmChatSession).not.toHaveBeenCalled();
    expect(contracts.createChatSession).not.toHaveBeenCalled();
  });

  it("leaves the cache alone when the created session is already listed", async () => {
    const client = testClient();
    const listed = makeSession({ id: "created", status: "open" });
    client.setQueryData(chatKeys.sessions("acme", "r-1"), [listed]);
    renderHook(() => useWarmUp(base()), { wrapper: wrapper(client) });
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "created"));
    expect(client.getQueryData(chatKeys.sessions("acme", "r-1"))).toEqual([listed]);
  });

  it("re-warms a different open session and does not repeat it on re-render", async () => {
    const client = testClient();
    const first = base({ session: makeSession({ id: "open-a" }), requestedId: "open-a", pendingId: "open-a" });
    const { rerender } = renderHook((props: Record<string, unknown>) => useWarmUp(props as never), {
      wrapper: wrapper(client),
      initialProps: first,
    });
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "open-a"));
    rerender(base({ session: makeSession({ id: "open-b" }), requestedId: "open-b", pendingId: "open-b" }));
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "open-b"));
    rerender(base({ session: makeSession({ id: "open-b" }), requestedId: "open-b", pendingId: "open-b" }));
    await act(async () => undefined);
    expect(contracts.warmChatSession).toHaveBeenCalledTimes(2);
  });

  it("skips the history refetch when the contact changes while the new session warms", async () => {
    const client = testClient();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    let settleWarm: () => void = () => undefined;
    contracts.warmChatSession.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          settleWarm = () => resolve({ ok: true });
        })
    );
    const onCreated = vi.fn();
    const { rerender } = renderHook((props: Record<string, unknown>) => useWarmUp(props as never), {
      wrapper: wrapper(client),
      initialProps: base({ onCreated }),
    });
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "created"));
    rerender(base({ onCreated, runnerId: "r-6", runner: makeRunner({ id: "r-6" }), sessions: undefined }));
    // The second contact never creates, so the only wait left is the first.
    await act(async () => {
      settleWarm();
    });
    expect(onCreated).toHaveBeenCalledTimes(1);
    expect(invalidate).not.toHaveBeenCalled();
  });
  it("warms a newly opened history session without creating one", async () => {
    const client = testClient();
    const first = base({ sessions: [makeSession({ id: "open-e" })], pendingId: null, requestedId: null });
    const { rerender } = renderHook((props: Record<string, unknown>) => useWarmUp(props as never), {
      wrapper: wrapper(client),
      initialProps: first,
    });
    await waitFor(() => expect(contracts.createChatSession).toHaveBeenCalledTimes(1));
    const second = base({
      sessions: [makeSession({ id: "open-e" })],
      session: makeSession({ id: "open-e" }),
      requestedId: "open-e",
      pendingId: "open-e",
    });
    rerender(second);
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "open-e"));
    expect(contracts.warmChatSession).toHaveBeenCalledTimes(2);
    expect(contracts.createChatSession).toHaveBeenCalledTimes(1);
  });

  it("creates a fresh session when the address is stale", async () => {
    const client = testClient();
    const input = base({
      sessions: [makeSession({ id: "other", status: "closed" })],
      session: makeSession({ id: "other", status: "closed" }),
      requestedId: "stale",
      pendingId: "stale",
    });
    renderHook(() => useWarmUp(input), { wrapper: wrapper(client) });
    await waitFor(() => expect(contracts.createChatSession).toHaveBeenCalledTimes(1));
    expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "created");
  });
  it("re-decides from a fresh render and never warms the same session twice", async () => {
    const client = testClient();
    const onCreated = vi.fn();
    const first = base({
      session: makeSession({ id: "open-h" }),
      requestedId: "open-h",
      pendingId: "open-h",
      onCreated,
    });
    const { rerender } = renderHook((props: Record<string, unknown>) => useWarmUp(props as never), {
      wrapper: wrapper(client),
      initialProps: first,
    });
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledTimes(1));
    await act(async () => undefined);
    // The runner goes busy: the decision re-evaluates, but the session it
    // names is the one already warmed.
    rerender({ ...first, runner: makeRunner({ status: "busy" }) });
    await act(async () => undefined);
    expect(contracts.warmChatSession).toHaveBeenCalledTimes(1);
  });

  it("re-decides to create without creating a second session", async () => {
    const client = testClient();
    const onCreated = vi.fn();
    const first = base({ onCreated, pendingId: null });
    const { rerender } = renderHook((props: Record<string, unknown>) => useWarmUp(props as never), {
      wrapper: wrapper(client),
      initialProps: first,
    });
    await waitFor(() => expect(contracts.createChatSession).toHaveBeenCalledTimes(1));
    await act(async () => undefined);
    rerender(base({ onCreated, pendingId: "some-other-pick" }));
    await act(async () => undefined);
    expect(contracts.createChatSession).toHaveBeenCalledTimes(1);
  });
});
