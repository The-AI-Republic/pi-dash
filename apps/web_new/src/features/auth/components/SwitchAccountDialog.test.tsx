// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { applyTheme, resolveTheme } from "@pidash/kit";
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

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { SIGN_OUT_FAILED_MESSAGE, SIGN_OUT_FAILED_TITLE } from "../signOut.js";
import { SwitchAccountDialog } from "./SwitchAccountDialog.js";

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
      <SwitchAccountDialog open={true} onOpenChange={onOpenChange} email="ada@example.com" />
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
  document.documentElement.removeAttribute("data-theme");
});

describe("SwitchAccountDialog", () => {
  it("names the active account and explains the switch", () => {
    renderDialog(stubTransport(async () => ({ status: 200, body: CSRF, url: "https://api.test/" })));
    expect(screen.getByRole("heading", { name: "Switch accounts" })).not.toBeNull();
    expect(screen.getByText("ada@example.com")).not.toBeNull();
    expect(screen.getByText(/wrong identity/)).not.toBeNull();
  });

  it("confirms through sign-out into a fresh login with the theme reset", async () => {
    const seen: Array<{ method?: string; url: string }> = [];
    renderDialog(
      stubTransport(async (url, init) => {
        seen.push({ method: init.method, url });
        return { status: 200, body: url.endsWith("/auth/get-csrf-token/") ? CSRF : "{}", url };
      })
    );
    applyTheme("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Switch account" }));

    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/" }));
    expect(seen.some((request) => request.url.endsWith("/auth/sign-out/"))).toBe(true);
    expect(document.documentElement.getAttribute("data-theme")).toBe(resolveTheme("system"));
    expect(useSessionStore.getState().status).toBe("signed-out");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("closes the dialog on a confirmed switch", async () => {
    const onOpenChange = vi.fn();
    renderDialog(
      stubTransport(async (url) => ({
        status: 200,
        body: url.endsWith("/auth/get-csrf-token/") ? CSRF : "{}",
        url,
      })),
      onOpenChange
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Switch account" }));

    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });

  it("keeps the dialog open when sign-out fails", async () => {
    renderDialog(
      stubTransport(async (url) => {
        if (url.endsWith("/auth/sign-out/")) return { status: 500, body: "{}", url };
        return { status: 200, body: CSRF, url };
      })
    );
    applyTheme("dark");

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Switch account" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(SIGN_OUT_FAILED_TITLE, SIGN_OUT_FAILED_MESSAGE));
    expect(navigate).not.toHaveBeenCalled();
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(screen.getByRole("heading", { name: "Switch accounts" })).not.toBeNull();
  });

  it("blocks dismissal while the switch is in flight", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    renderDialog(
      stubTransport(async (url) => {
        if (url.endsWith("/auth/sign-out/")) await gate;
        return { status: 200, body: url.endsWith("/auth/get-csrf-token/") ? CSRF : "{}", url };
      })
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Switch account" }));

    const confirm = await screen.findByRole("button", { name: "Switch account" });
    await waitFor(() => expect(confirm).toBeDisabled());
    expect(confirm).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();

    release();
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/" }));
  });

  it("cancels without signing out", async () => {
    const seen: string[] = [];
    const onOpenChange = vi.fn();
    renderDialog(
      stubTransport(async (url) => {
        seen.push(url);
        return { status: 200, body: CSRF, url };
      }),
      onOpenChange
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(seen.some((url) => url.endsWith("/auth/sign-out/"))).toBe(false);
    expect(navigate).not.toHaveBeenCalled();
  });
});
