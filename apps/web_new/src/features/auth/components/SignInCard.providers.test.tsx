// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { SignInCard } from "../index.js";
import type { SignInCardProps } from "../index.js";

vi.mock("../../../core/edition/index.js", () => ({
  edition: {
    id: "cloud",
    flags: {},
    auth: [
      { id: "google", label: "Google" },
      { id: "github", label: "GitHub" },
      { id: "saml", label: "SAML" },
    ],
  },
}));

interface StubRoute {
  body: string;
  url: string;
}

function stubTransport(handler: (url: string, init: TransportInit) => StubRoute | Promise<StubRoute>): Transport {
  return async (url, init) => {
    const response = await handler(url, init);
    return {
      status: 200,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

async function renderReadyCard(transport: Transport, props: SignInCardProps = {}) {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <SignInCard {...props} />
    </QueryClientProvider>
  );
  await screen.findByRole("button", { name: /sign in with google/i });
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

function configBody(overrides: Record<string, boolean> = {}): string {
  return JSON.stringify({
    config: {
      enable_signup: true,
      is_workspace_creation_disabled: false,
      is_google_enabled: true,
      is_github_enabled: false,
      is_gitlab_enabled: false,
      is_gitea_enabled: false,
      is_magic_login_enabled: true,
      is_email_password_enabled: true,
      is_smtp_configured: true,
      is_self_managed: false,
      instance_changelog_url: "",
      ...overrides,
    },
  });
}

describe("SignInCard providers", () => {
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

  it("offers only the instance-enabled providers and starts their flow", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: configBody(), url };
        throw new Error(`unexpected ${url}`);
      }),
      { next: "/acme" }
    );
    expect(screen.getByRole("button", { name: "Sign in with Google" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /github/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /saml/i })).toBeNull();
    expect(screen.getByLabelText("Work email")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Sign in with Google" }));
    expect(assign).toHaveBeenCalledWith("https://api.test/auth/google/?next_path=%2Facme");
  });

  it("carries an unsafe return path on provider starts for the server to sanitize", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: configBody(), url };
        throw new Error(`unexpected ${url}`);
      }),
      { unsafeNext: "https://evil.test/phish" }
    );
    await user.click(screen.getByRole("button", { name: "Sign in with Google" }));
    expect(assign).toHaveBeenCalledWith("https://api.test/auth/google/?next_path=https%3A%2F%2Fevil.test%2Fphish");
  });

  it("reads compact on the password step", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: configBody(), url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    expect(screen.getByRole("button", { name: "Sign in with Google" }).className).toContain("w-full");
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("Password");
    expect(screen.getByRole("button", { name: "Sign in with Google" }).className).not.toContain("w-full");
  });

  it("names the sign-up mode on flipped cards", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: configBody(), url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("button", { name: "Sign up with Google" })).toBeInTheDocument();
  });

  it("shows providers without the email form when email login is off", async () => {
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) {
          return {
            body: configBody({ is_email_password_enabled: false, is_magic_login_enabled: false }),
            url,
          };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    expect(screen.getByRole("button", { name: "Sign in with Google" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Work email")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Sign-in is unavailable" })).toBeNull();
  });

  it("completes password sign-in behind the provider section", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: configBody(), url };
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
});
