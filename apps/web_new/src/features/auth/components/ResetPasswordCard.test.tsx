// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ResetPasswordCard } from "./ResetPasswordCard.js";
import type { ResetPasswordCardProps } from "./ResetPasswordCard.js";

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

function stubTransport(handler: (url: string) => StubRoute): Transport {
  return async (url) => {
    const response = handler(url);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

function renderCard(transport: Transport, props: ResetPasswordCardProps) {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ResetPasswordCard {...props} />
    </QueryClientProvider>
  );
  return { queryClient };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });
const STRONG = "Tq7!mZ2@kP9#xR";

function csrfThen(handler: (url: string) => StubRoute): Transport {
  return stubTransport((url) => {
    if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
    return handler(url);
  });
}

async function fillAndSubmit(password: string, confirm: string) {
  const user = userEvent.setup();
  await user.type(screen.getByLabelText("New password"), password);
  await user.type(screen.getByLabelText("Confirm password"), confirm);
  await user.click(screen.getByRole("button", { name: "Reset password" }));
}

describe("ResetPasswordCard", () => {
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

  it("banners at once when the link has no params", () => {
    renderCard(
      csrfThen(() => {
        throw new Error("must not post without a link");
      }),
      { uid: "", token: "", email: "ada@example.com" }
    );
    expect(screen.getByRole("alert")).toHaveTextContent(/invalid password token/i);
    expect(screen.queryByLabelText("New password")).toBeNull();
  });

  it("banners at once when the link is missing its token", () => {
    renderCard(
      csrfThen(() => {
        throw new Error("must not post without a link");
      }),
      { uid: "uid", token: "", email: "ada@example.com" }
    );
    expect(screen.getByRole("alert")).toHaveTextContent(/invalid password token/i);
    expect(screen.queryByLabelText("New password")).toBeNull();
  });

  it("locks the address and returns to the entry on success", async () => {
    renderCard(
      csrfThen((url) => {
        if (url.includes("/auth/reset-password/")) {
          return { status: 200, body: "", url: "https://app.test/sign-in?success=True" };
        }
        throw new Error(`unexpected ${url}`);
      }),
      { uid: "uid", token: "token", email: "ada@example.com" }
    );
    expect(screen.getByLabelText("Work email")).toHaveValue("ada@example.com");
    await fillAndSubmit(STRONG, STRONG);
    await waitFor(() => expect(assign).toHaveBeenCalledWith("/"));
  });

  it.each([
    { code: "5125", expected: /invalid password token/i },
    { code: "5130", expected: /expired password token/i },
  ])("replaces the form with the banner for a dead link ($code)", async ({ code, expected }) => {
    renderCard(
      csrfThen((url) => {
        if (url.includes("/auth/reset-password/")) {
          return { status: 200, body: "", url: `https://app.test/accounts/reset-password?error_code=${code}` };
        }
        throw new Error(`unexpected ${url}`);
      }),
      { uid: "uid", token: "token", email: "ada@example.com" }
    );
    await fillAndSubmit(STRONG, STRONG);
    expect(await screen.findByRole("alert")).toHaveTextContent(expected);
    expect(screen.queryByLabelText("New password")).toBeNull();
    expect(assign).not.toHaveBeenCalled();
  });

  it("keeps the form for retryable server rejections", async () => {
    renderCard(
      csrfThen((url) => {
        if (url.includes("/auth/reset-password/")) {
          return { status: 200, body: "", url: "https://app.test/accounts/reset-password?error_code=5021" };
        }
        throw new Error(`unexpected ${url}`);
      }),
      { uid: "uid", token: "token" }
    );
    expect(screen.queryByLabelText("Work email")).toBeNull();
    await fillAndSubmit(STRONG, STRONG);
    expect(await screen.findByRole("alert")).toHaveTextContent(/That password is too weak/);
    expect(screen.getByLabelText("New password")).toBeInTheDocument();
  });

  it("banners a 500 as the invalid-token crash without the form", async () => {
    renderCard(
      csrfThen((url) => {
        if (url.includes("/auth/reset-password/")) {
          return { status: 500, body: "Internal Server Error", url };
        }
        throw new Error(`unexpected ${url}`);
      }),
      { uid: "AAAA", token: "bogus-token", email: "ada@example.com" }
    );
    await fillAndSubmit(STRONG, STRONG);
    expect(await screen.findByRole("alert")).toHaveTextContent(/invalid password token/i);
    expect(screen.queryByLabelText("New password")).toBeNull();
  });

  it("keeps the form with a generic banner when the post never answers", async () => {
    renderCard(
      csrfThen((url) => {
        if (url.includes("/auth/reset-password/")) {
          throw new Error("offline");
        }
        throw new Error(`unexpected ${url}`);
      }),
      { uid: "uid", token: "token", email: "ada@example.com" }
    );
    await fillAndSubmit(STRONG, STRONG);
    expect(await screen.findByRole("alert")).toHaveTextContent(/Could not reset your password/);
    expect(screen.getByLabelText("New password")).toBeInTheDocument();
  });
});
