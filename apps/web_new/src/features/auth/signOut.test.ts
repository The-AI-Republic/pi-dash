// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { registerRuntimeSignOutHooks } from "../../core/session/index.js";
import { registerStoreReset, useSessionStore } from "../../core/session/store.js";

vi.mock("../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

import { notifyError } from "../../shared/shell/index.js";
import { performSignOut, SIGN_OUT_FAILED_MESSAGE, SIGN_OUT_FAILED_TITLE, signOutLandingTarget } from "./signOut.js";

beforeEach(() => {
  useSessionStore.setState({ status: "authenticated", returnUrl: null });
  vi.mocked(notifyError).mockClear();
});

function stubClient(post: () => Promise<unknown>) {
  return {
    clearCsrfCache: vi.fn(),
    post: vi.fn(post),
  };
}

describe("signOutLandingTarget", () => {
  it("lands on the entry route on web and the login route on desktop", () => {
    expect(signOutLandingTarget("web")).toBe("/");
    expect(signOutLandingTarget("desktop")).toBe("/login");
  });
});

describe("performSignOut", () => {
  it("posts with a fresh token, then clears every store and the cache", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    queryClient.setQueryData(["session", "me"], { id: "user-1" });
    const reset = vi.fn();
    const unregister = registerStoreReset(reset);

    try {
      await expect(performSignOut({ client: client as never, queryClient })).resolves.toBe("/");
    } finally {
      unregister();
    }

    const clearOrder = vi.mocked(client.clearCsrfCache).mock.invocationCallOrder[0] ?? 0;
    const postOrder = vi.mocked(client.post).mock.invocationCallOrder[0] ?? 0;
    expect(clearOrder).toBeLessThan(postOrder);
    expect(client.post).toHaveBeenCalledWith("/auth/sign-out/", {});
    expect(reset).toHaveBeenCalledTimes(1);
    expect(queryClient.getQueryData(["session", "me"])).toBeUndefined();
    expect(useSessionStore.getState().status).toBe("signed-out");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts and rethrows a failed POST without clearing anything", async () => {
    const client = stubClient(async () => {
      throw new Error("boom");
    });
    const queryClient = new QueryClient();
    queryClient.setQueryData(["session", "me"], { id: "user-1" });

    await expect(performSignOut({ client: client as never, queryClient })).rejects.toThrow("boom");
    expect(notifyError).toHaveBeenCalledWith(SIGN_OUT_FAILED_TITLE, SIGN_OUT_FAILED_MESSAGE);
    expect(queryClient.getQueryData(["session", "me"])).toEqual({ id: "user-1" });
    expect(useSessionStore.getState().status).toBe("authenticated");
  });

  it("runs the desktop teardown after the POST and before landing", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    const clearLocalSession = vi.fn(async () => undefined);

    await expect(
      performSignOut({ client: client as never, queryClient, teardown: { clearLocalSession } })
    ).resolves.toBe("/");

    const postOrder = vi.mocked(client.post).mock.invocationCallOrder[0] ?? 0;
    const wipeOrder = clearLocalSession.mock.invocationCallOrder[0] ?? 0;
    expect(postOrder).toBeLessThan(wipeOrder);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("swallows a failed local teardown and still lands", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    const clearLocalSession = vi.fn(async () => {
      throw new Error("wipe failed");
    });

    await expect(
      performSignOut({ client: client as never, queryClient, teardown: { clearLocalSession } })
    ).resolves.toBe("/");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("disposes the runtime before the POST once confirmed", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    const choice = { deleteChatHistory: true };
    const confirmRuntimeSignOut = vi.fn(async () => choice);
    const disposeRuntimeForSignOut = vi.fn(async () => undefined);
    const unregister = registerRuntimeSignOutHooks({ confirmRuntimeSignOut, disposeRuntimeForSignOut });

    try {
      await expect(performSignOut({ client: client as never, queryClient })).resolves.toBe("/");
    } finally {
      unregister();
    }

    expect(confirmRuntimeSignOut).toHaveBeenCalledTimes(1);
    expect(disposeRuntimeForSignOut).toHaveBeenCalledWith(choice);
    const disposeOrder = disposeRuntimeForSignOut.mock.invocationCallOrder[0] ?? 0;
    const postOrder = vi.mocked(client.post).mock.invocationCallOrder[0] ?? 0;
    expect(disposeOrder).toBeGreaterThan(0);
    expect(disposeOrder).toBeLessThan(postOrder);
    expect(useSessionStore.getState().status).toBe("signed-out");
  });

  it("signs nothing out when the runtime confirmation is cancelled", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    queryClient.setQueryData(["session", "me"], { id: "user-1" });
    const confirmRuntimeSignOut = vi.fn(async () => null);
    const disposeRuntimeForSignOut = vi.fn(async () => undefined);
    const unregister = registerRuntimeSignOutHooks({ confirmRuntimeSignOut, disposeRuntimeForSignOut });

    try {
      await expect(performSignOut({ client: client as never, queryClient })).resolves.toBeNull();
    } finally {
      unregister();
    }

    expect(disposeRuntimeForSignOut).not.toHaveBeenCalled();
    expect(client.post).not.toHaveBeenCalled();
    expect(queryClient.getQueryData(["session", "me"])).toEqual({ id: "user-1" });
    expect(useSessionStore.getState().status).toBe("authenticated");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("disposes without asking for already-confirmed sign-outs", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    const confirmRuntimeSignOut = vi.fn(async () => ({ deleteChatHistory: true }));
    const disposeRuntimeForSignOut = vi.fn(async () => undefined);
    const unregister = registerRuntimeSignOutHooks({ confirmRuntimeSignOut, disposeRuntimeForSignOut });

    try {
      await expect(performSignOut({ client: client as never, queryClient, skipRuntimeConfirm: true })).resolves.toBe(
        "/"
      );
    } finally {
      unregister();
    }

    expect(confirmRuntimeSignOut).not.toHaveBeenCalled();
    expect(disposeRuntimeForSignOut).toHaveBeenCalledWith({ deleteChatHistory: false });
    expect(client.post).toHaveBeenCalledWith("/auth/sign-out/", {});
  });
});
