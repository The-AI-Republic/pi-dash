// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

// Toast rendering belongs to kit; what this suite owns is the toast calls.
vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { ForgotPasswordCard } from "./ForgotPasswordCard.js";

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

function stubTransport(handler: (url: string) => StubRoute | Promise<StubRoute>): Transport {
  return async (url) => {
    const response = await handler(url);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

function renderCard(transport: Transport, initialEmail = "") {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { container } = render(
    <QueryClientProvider client={queryClient}>
      <ForgotPasswordCard initialEmail={initialEmail} />
    </QueryClientProvider>
  );
  return { queryClient, container };
}

function submitForm(container: HTMLElement) {
  const form = container.querySelector("form");
  if (!form) throw new Error("missing form");
  fireEvent.submit(form);
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });
const FORGOT_OK = JSON.stringify({ message: "Check your email to reset your password" });

beforeEach(() => {
  vi.mocked(notifySuccess).mockClear();
  vi.mocked(notifyError).mockClear();
});

afterEach(() => {
  vi.useRealTimers();
  resetAppClient();
});

describe("ForgotPasswordCard", () => {
  it("prefills the address and links back to sign-in", () => {
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        throw new Error(`unexpected ${url}`);
      }),
      "  ada@example.com  "
    );
    expect(screen.getByLabelText("Work email")).toHaveValue("ada@example.com");
    expect(screen.getByRole("link", { name: "Back to sign in" })).toHaveAttribute("href", "/");
  });

  it("gates submit on a valid address and clears on demand", async () => {
    const user = userEvent.setup();
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        throw new Error(`unexpected ${url}`);
      })
    );
    const submit = screen.getByRole("button", { name: "Send reset link" });
    expect(submit).toBeDisabled();

    const field = screen.getByLabelText("Work email");
    await user.type(field, "not-an-email");
    expect(submit).toBeDisabled();
    expect(screen.queryByText("Enter a valid email address.")).toBeNull();
    fireEvent.blur(field);
    expect(screen.getByText("Enter a valid email address.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(field).toHaveValue("");
    expect(screen.queryByRole("button", { name: "Clear" })).toBeNull();
  });

  it("toasts the inbox check and throttles resends for thirty seconds", async () => {
    vi.useFakeTimers();
    try {
      const gate = deferred<StubRoute>();
      let posts = 0;
      const { container } = renderCard(
        stubTransport((url) => {
          if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
          if (url.endsWith("/auth/forgot-password/")) {
            posts += 1;
            return gate.promise;
          }
          throw new Error(`unexpected ${url}`);
        })
      );
      const field = screen.getByLabelText("Work email");
      fireEvent.change(field, { target: { value: "ada@example.com" } });
      // jsdom does not submit forms on button click; submit the form itself.
      // (The pending render lands a tick later, so the countdown below is
      // the assertion; the pending branch still executes while gated.)
      submitForm(container);
      gate.resolve({ status: 200, body: FORGOT_OK, url: "https://api.test/auth/forgot-password/" });
      await act(async () => {});

      expect(notifySuccess).toHaveBeenCalledWith(
        "Check your inbox",
        "A password reset link is on its way to ada@example.com."
      );
      expect(vi.mocked(notifySuccess).mock.calls[0]?.[0]).toMatch(/check your inbox/i);
      expect(screen.getByText("Resend in 30 seconds")).toBeInTheDocument();
      expect(field).toBeDisabled();
      expect(screen.getByRole("button", { name: /resend in/i })).toBeDisabled();
      expect(screen.queryByRole("button", { name: "Clear" })).toBeNull();

      await act(async () => {
        vi.advanceTimersByTime(29_000);
      });
      expect(screen.getByText("Resend in 1 second")).toBeInTheDocument();
      await act(async () => {
        vi.advanceTimersByTime(1000);
      });
      expect(screen.getByRole("button", { name: "Resend link" })).toBeEnabled();
      expect(field).toBeEnabled();
      // The ticker stops once the countdown elapses instead of re-rendering forever.
      expect(vi.getTimerCount()).toBe(0);

      submitForm(container);
      await act(async () => {});
      expect(posts).toBe(2);
      expect(screen.getByText("Resend in 30 seconds")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("toasts failures without starting the countdown", async () => {
    const user = userEvent.setup();
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/auth/forgot-password/")) {
          return {
            status: 400,
            body: JSON.stringify({ error_code: 5060, error_message: "USER_DOES_NOT_EXIST" }),
            url,
          };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ghost@example.com");
    await user.click(screen.getByRole("button", { name: "Send reset link" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith("Could not send a reset link", "Try again in a moment.");
    expect(screen.getByRole("button", { name: "Send reset link" })).toBeEnabled();
    const toastCalls = vi.mocked(notifyError).mock.calls;
    const toast = `${toastCalls[0]?.[0] ?? ""} ${toastCalls[0]?.[1] ?? ""}`;
    expect(toast.length).toBeGreaterThan(0);
    expect(toast).not.toMatch(/no account|does not exist|not found|unknown user/i);
    expect(screen.getByLabelText("Work email")).toBeEnabled();
  });
});
