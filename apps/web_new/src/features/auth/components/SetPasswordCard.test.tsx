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

// Toast rendering belongs to kit; what this suite owns is the toast calls.
vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";
import { SetPasswordCard } from "./SetPasswordCard.js";

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

function renderCard(transport: Transport) {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <SetPasswordCard />
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

describe("SetPasswordCard", () => {
  const assign = vi.fn();

  beforeEach(() => {
    assign.mockClear();
    vi.mocked(notifyError).mockClear();
    Object.defineProperty(globalThis.window, "location", {
      value: { assign },
      writable: true,
      configurable: true,
    });
  });

  afterEach(() => {
    resetAppClient();
  });

  it("lands on the entry route once the first secret is set", async () => {
    const user = userEvent.setup();
    renderCard(
      csrfThen((url) => {
        if (url.endsWith("/auth/set-password/")) {
          return {
            status: 200,
            body: JSON.stringify({
              id: "11111111-1111-4111-8111-111111111111",
              email: "ada@example.com",
              is_password_autoset: false,
            }),
            url,
          };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    expect(screen.queryByLabelText("Work email")).toBeNull();
    await user.type(screen.getByLabelText("New password"), STRONG);
    await user.type(screen.getByLabelText("Confirm password"), STRONG);
    await user.click(screen.getByRole("button", { name: "Set password" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith("/"));
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts server detail when setting fails", async () => {
    const user = userEvent.setup();
    renderCard(
      csrfThen((url) => {
        if (url.endsWith("/auth/set-password/")) {
          return {
            status: 400,
            body: JSON.stringify({ error_code: 5145, error_message: "PASSWORD_ALREADY_SET" }),
            url,
          };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("New password"), STRONG);
    await user.type(screen.getByLabelText("Confirm password"), STRONG);
    await user.click(screen.getByRole("button", { name: "Set password" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith("Could not set a password", "A password is already set on this account.");
    expect(assign).not.toHaveBeenCalled();
  });

  it("toasts generic copy when the post never answers", async () => {
    const user = userEvent.setup();
    renderCard(
      csrfThen((url) => {
        if (url.endsWith("/auth/set-password/")) {
          throw new Error("offline");
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("New password"), STRONG);
    await user.type(screen.getByLabelText("Confirm password"), STRONG);
    await user.click(screen.getByRole("button", { name: "Set password" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith(
      "Could not set a password",
      "Could not set your password. Try again in a moment."
    );
  });
});
