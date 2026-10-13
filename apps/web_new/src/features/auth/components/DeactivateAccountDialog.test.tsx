// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { useSessionStore } from "../../../core/session/store.js";

const { navigate } = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => navigate }));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { DEACTIVATE_FAILED_TITLE, DEACTIVATE_SUCCESS_MESSAGE, DEACTIVATE_SUCCESS_TITLE } from "../api.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { DeactivateAccountDialog } from "./DeactivateAccountDialog.js";

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

function stubTransport(handler: (url: string, init: TransportInit) => StubRoute | Promise<StubRoute>): Transport {
  return async (url, init) => {
    const response = await handler(url, init);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

function renderDialog(transport: Transport, onOpenChange: (open: boolean) => void = () => undefined) {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <DeactivateAccountDialog open={true} onOpenChange={onOpenChange} email="ada@example.com" />
    </QueryClientProvider>
  );
  return { queryClient };
}

afterEach(() => {
  resetAppClient();
  navigate.mockClear();
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
  useSessionStore.setState({ status: "authenticated", returnUrl: null });
});

describe("DeactivateAccountDialog", () => {
  it("explains the consequences and the way back", () => {
    renderDialog(stubTransport(async () => ({ status: 200, body: CSRF, url: "https://api.test/" })));
    expect(screen.getByRole("heading", { name: "Deactivate account" })).not.toBeNull();
    expect(screen.getByText(/assignments and billing standing/)).not.toBeNull();
    expect(screen.getByText(/fresh workspace invitation/)).not.toBeNull();
    expect(screen.getByText("ada@example.com")).not.toBeNull();
  });

  it("confirms deactivation, then signs out back at the entry", async () => {
    const seen: Array<{ method?: string; url: string }> = [];
    const onOpenChange = vi.fn();
    renderDialog(
      stubTransport(async (url, init) => {
        seen.push({ method: init.method, url });
        return { status: 200, body: url.endsWith("/auth/get-csrf-token/") ? CSRF : "{}", url };
      }),
      onOpenChange
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Deactivate account" }));

    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    const deactivation = seen.findIndex((request) => request.url.endsWith("/api/users/me/"));
    const signOut = seen.findIndex((request) => request.url.endsWith("/auth/sign-out/"));
    expect(deactivation).toBeGreaterThanOrEqual(0);
    expect(signOut).toBeGreaterThan(deactivation);
    expect(seen[deactivation]?.method).toBe("DELETE");
    expect(notifySuccess).toHaveBeenCalledWith(DEACTIVATE_SUCCESS_TITLE, DEACTIVATE_SUCCESS_MESSAGE);
    expect(useSessionStore.getState().status).toBe("signed-out");
  });

  it("toasts the server detail and stays signed in when deactivation fails", async () => {
    const seen: string[] = [];
    renderDialog(
      stubTransport(async (url, init) => {
        seen.push(url);
        if (init.method === "DELETE") {
          return { status: 400, body: JSON.stringify({ detail: "Deactivation is unavailable." }), url };
        }
        return { status: 200, body: CSRF, url };
      })
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Deactivate account" }));

    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(DEACTIVATE_FAILED_TITLE, "Deactivation is unavailable.")
    );
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(seen.some((url) => url.endsWith("/auth/sign-out/"))).toBe(false);
    expect(useSessionStore.getState().status).toBe("authenticated");
    expect(navigate).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Deactivate account" })).not.toBeNull();
  });

  it("shows a deactivating state while the request is in flight", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    renderDialog(
      stubTransport(async (url, init) => {
        if (init.method === "DELETE") await gate;
        return { status: 200, body: url.endsWith("/auth/get-csrf-token/") ? CSRF : "{}", url };
      })
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Deactivate account" }));

    const confirm = await screen.findByRole("button", { name: "Deactivate account" });
    await waitFor(() => expect(confirm).toBeDisabled());
    expect(confirm).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();

    release();
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/" }));
  });

  it("cancels without deactivating", async () => {
    const seen: Array<{ method?: string; url: string }> = [];
    const onOpenChange = vi.fn();
    renderDialog(
      stubTransport(async (url, init) => {
        seen.push({ method: init.method, url });
        return { status: 200, body: CSRF, url };
      }),
      onOpenChange
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(seen.some((request) => request.method === "DELETE")).toBe(false);
    expect(navigate).not.toHaveBeenCalled();
  });
});
