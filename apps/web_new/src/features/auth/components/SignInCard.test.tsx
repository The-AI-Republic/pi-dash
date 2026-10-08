// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { SignInCard } from "./SignInCard.js";

function stubTransport(handler: (url: string) => { body: string; url: string }): Transport {
  return async (url) => {
    const response = handler(url);
    return {
      status: 200,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

function renderCard(transport: Transport, props: { initialErrorCode?: string | null } = {}) {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <SignInCard initialErrorCode={props.initialErrorCode ?? null} />
    </QueryClientProvider>
  );
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

describe("SignInCard", () => {
  const assign = vi.fn();

  beforeEach(() => {
    assign.mockClear();
    Object.defineProperty(globalThis.window, "location", {
      value: { assign },
      writable: true,
      configurable: true,
    });
  });

  afterEach(() => {
    resetAppClient();
  });

  it("routes a password address to the password step", async () => {
    const user = userEvent.setup();
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByLabelText("Password")).toBeInTheDocument();
  });

  it("routes a code address to the code step after generating", async () => {
    const user = userEvent.setup();
    const seen: string[] = [];
    renderCard(
      stubTransport((url) => {
        seen.push(url);
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          return { body: JSON.stringify({ key: "magic_ada@example.com" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByLabelText("One-time code")).toBeInTheDocument();
    expect(seen.some((url) => url.endsWith("/auth/magic-generate/"))).toBe(true);
  });

  it("stays on email with a banner for unknown addresses", async () => {
    const user = userEvent.setup();
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        return { body: JSON.stringify({ existing: false, status: "CREDENTIAL" }), url };
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("No account uses this address yet.");
    expect(screen.getByLabelText("Work email")).toBeInTheDocument();
  });

  it("completes password sign-in through the server landing URL", async () => {
    const user = userEvent.setup();
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        return { body: "<html>", url: "https://app.test/contract-acme" };
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("Password"), "secret");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith("https://app.test/contract-acme"));
  });

  it("maps a wrong password back to the password step with a banner", async () => {
    const user = userEvent.setup();
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        return {
          body: "<html>",
          url: "https://app.test/sign-in?error_code=AUTHENTICATION_FAILED_SIGN_IN&error_message=x",
        };
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("Password"), "wrong");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("password");
    expect(assign).not.toHaveBeenCalled();
  });

  it("preloads a server error code from the URL on the fixing step", async () => {
    renderCard(
      stubTransport(() => ({ body: "{}", url: "https://api.test/" })),
      {
        initialErrorCode: "AUTHENTICATION_FAILED_SIGN_IN",
      }
    );
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
  });
});
