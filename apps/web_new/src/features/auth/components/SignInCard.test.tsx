// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { authKeys, isValidEmail, SignInCard } from "../index.js";
import type { SignInCardProps } from "../index.js";

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

function renderCard(transport: Transport, props: SignInCardProps = {}, queryClient?: QueryClient) {
  createAppClient("https://api.test", transport);
  const client = queryClient ?? new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { container } = render(
    <QueryClientProvider client={client}>
      <SignInCard {...props} />
    </QueryClientProvider>
  );
  return { queryClient: client, container };
}

async function renderReadyCard(transport: Transport, props: SignInCardProps = {}) {
  const result = renderCard(transport, props);
  await screen.findByLabelText("Work email");
  return result;
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

function configBody(overrides: Record<string, boolean> = {}): string {
  return JSON.stringify({
    config: {
      enable_signup: true,
      is_workspace_creation_disabled: false,
      is_google_enabled: false,
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

const CONFIG = configBody();

function invitationFixture(): Record<string, unknown> {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    workspace: {
      id: "22222222-2222-4222-8222-222222222222",
      name: "Acme",
      slug: "acme",
      logo_url: "https://cdn.test/acme.png",
    },
    email: "ada@example.com",
    accepted: false,
    token: "tok",
    message: null,
    responded_at: null,
    role: 15,
    invite_link: "https://app.test/invite",
    created_by: null,
    updated_by: null,
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
  };
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
    vi.useRealTimers();
    resetAppClient();
  });

  it("validates email addresses", () => {
    expect(isValidEmail("ada@example.com")).toBe(true);
    expect(isValidEmail("  ada@example.com  ")).toBe(true);
    expect(isValidEmail("not-an-email")).toBe(false);
    expect(isValidEmail("ada@")).toBe(false);
    expect(isValidEmail("")).toBe(false);
  });

  it("routes a password address to the password step", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByLabelText("Password")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Sign in to Pi Dash" })).toBeInTheDocument();
    expect(screen.getByText(/Welcome back to Pi Dash/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /with google/i })).toBeNull();
  });

  it("routes a code address to the code step after generating", async () => {
    const user = userEvent.setup();
    const seen: string[] = [];
    await renderReadyCard(
      stubTransport((url) => {
        seen.push(url);
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
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

  it("gates Continue on a valid address and hints only after blur", async () => {
    const user = userEvent.setup();
    let checks = 0;
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          checks += 1;
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    const field = screen.getByLabelText("Work email");
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
    await user.type(field, "not-an-email");
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
    expect(screen.queryByText("Enter a valid email address.")).toBeNull();
    fireEvent.blur(field);
    expect(screen.getByText("Enter a valid email address.")).toBeInTheDocument();
    await user.clear(field);
    expect(screen.queryByText("Enter a valid email address.")).toBeNull();
    await user.type(field, "ada@example.com");
    fireEvent.blur(field);
    expect(screen.queryByText("Enter a valid email address.")).toBeNull();
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled();
    expect(checks).toBe(0);
  });

  it("disables Continue while the check is in flight", async () => {
    const user = userEvent.setup();
    const check = deferred<StubRoute>();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) return check.promise;
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
    check.resolve({ body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url: "https://api.test/x" });
    expect(await screen.findByLabelText("Password")).toBeInTheDocument();
  });

  it("clears the field without losing focus", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        throw new Error(`unexpected ${url}`);
      })
    );
    const field = screen.getByLabelText("Work email");
    expect(screen.queryByRole("button", { name: "Clear" })).toBeNull();
    await user.click(field);
    await user.type(field, "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(field).toHaveValue("");
    expect(field).toHaveFocus();
    expect(screen.queryByRole("button", { name: "Clear" })).toBeNull();
  });

  it("stays on email with a banner when the check fails", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) throw new Error("offline");
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the server");
    expect(screen.getByLabelText("Work email")).toBeInTheDocument();
  });

  it("stays on email with a banner when entry generation fails", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) throw new Error("offline");
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not send a code");
    expect(screen.getByLabelText("Work email")).toBeInTheDocument();
  });

  it("flips unknown addresses into sign-up mode and creates the account", async () => {
    const user = userEvent.setup();
    const posts: string[] = [];
    await renderReadyCard(
      stubTransport((url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "CREDENTIAL" }), url };
        }
        if (url.endsWith("/auth/sign-up/")) {
          posts.push(init.body ?? "");
          return { body: "<html>", url: "https://app.test/onboarding" };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByLabelText("Password")).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm password")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create your account" })).toBeInTheDocument();
    await user.type(screen.getByLabelText("Password"), "Tq7!mZ2@kP9#xR");
    await user.type(screen.getByLabelText("Confirm password"), "Tq7!mZ2@kP9#xR");
    await user.click(screen.getByRole("button", { name: "Create account" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith("https://app.test/onboarding"));
    expect(posts).toHaveLength(1);
    expect(posts[0]).toContain("confirm_password=Tq7%21mZ2%40kP9%23xR");
  });

  it("routes unknown code addresses to the sign-up code step", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          return { body: JSON.stringify({ key: "magic_new@example.com" }), url };
        }
        if (url.endsWith("/auth/magic-sign-up/")) {
          return { body: "<html>", url: "https://app.test/onboarding" };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByLabelText("One-time code")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create your account" })).toBeInTheDocument();
    await user.type(screen.getByLabelText("One-time code"), "123456");
    await user.click(screen.getByRole("button", { name: "Verify and create account" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith("https://app.test/onboarding"));
  });

  it("falls back to the password step when mail is off despite a magic status", async () => {
    const user = userEvent.setup();
    let generates = 0;
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: configBody({ is_smtp_configured: false }), url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          generates += 1;
          return { body: JSON.stringify({ key: "k" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByLabelText("Password")).toBeInTheDocument();
    expect(generates).toBe(0);
  });

  it("resets the card when the address is cleared", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("Password"), "secret");
    await user.click(screen.getByRole("button", { name: "Clear email" }));
    expect(assign).toHaveBeenCalledWith("/");
    expect(screen.getByLabelText("Work email")).toHaveValue("");
    expect(screen.getByRole("heading", { name: "Sign in to Pi Dash" })).toBeInTheDocument();
  });

  it("completes password sign-in through the server landing URL", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
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
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        return {
          body: "<html>",
          url: "https://app.test/sign-in?error_code=5065&error_message=x",
        };
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("Password"), "wrong");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/authentication failed/i);
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    expect(assign).not.toHaveBeenCalled();
  });

  it("toggles password visibility", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    const field = await screen.findByLabelText("Password");
    expect(field).toHaveAttribute("type", "password");
    await user.click(screen.getByRole("button", { name: "Show password" }));
    expect(field).toHaveAttribute("type", "text");
    await user.click(screen.getByRole("button", { name: "Hide password" }));
    expect(field).toHaveAttribute("type", "password");
  });

  it("links the forgot entry with the address when mail is on", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("Password");
    expect(screen.getByRole("link", { name: "Forgot your password?" })).toHaveAttribute(
      "href",
      "/accounts/forgot-password?email=ada%40example.com"
    );
  });

  it("forwards the return path on password submit", async () => {
    const user = userEvent.setup();
    const posts: string[] = [];
    await renderReadyCard(
      stubTransport((url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        if (url.endsWith("/auth/sign-in/")) {
          posts.push(init.body ?? "");
          return { body: "<html>", url: "https://app.test/acme" };
        }
        throw new Error(`unexpected ${url}`);
      }),
      { next: "/acme" }
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("Password"), "secret");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith("https://app.test/acme"));
    expect(posts).toHaveLength(1);
    expect(posts[0]).toContain("next_path=%2Facme");
  });

  it("forwards an unsafe return path raw so the server sanitizes it", async () => {
    const user = userEvent.setup();
    const posts: string[] = [];
    await renderReadyCard(
      stubTransport((url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        if (url.endsWith("/auth/sign-in/")) {
          posts.push(init.body ?? "");
          return { body: "<html>", url: "https://app.test/phish" };
        }
        throw new Error(`unexpected ${url}`);
      }),
      { unsafeNext: "https://evil.test/phish" }
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("Password"), "secret");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith("https://app.test/phish"));
    expect(posts).toHaveLength(1);
    expect(posts[0]).toContain("next_path=https%3A%2F%2Fevil.test%2Fphish");
  });

  it("keeps the password step with a banner when sign-in is unreachable", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        if (url.endsWith("/auth/sign-in/")) throw new Error("offline");
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("Password"), "secret");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the server");
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
  });

  it("completes code sign-in through the server landing URL", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          return { body: JSON.stringify({ key: "magic_ada@example.com" }), url };
        }
        if (url.endsWith("/auth/magic-sign-in/")) {
          return { body: "<html>", url: "https://app.test/contract-acme" };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("One-time code"), "123456");
    await user.click(screen.getByRole("button", { name: "Verify and sign in" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith("https://app.test/contract-acme"));
  });

  it("maps a wrong code back to the code step with a banner", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          return { body: JSON.stringify({ key: "magic_ada@example.com" }), url };
        }
        if (url.endsWith("/auth/magic-sign-in/")) {
          return { body: "<html>", url: "https://app.test/?error_code=5090&error_message=x" };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("One-time code"), "000000");
    await user.click(screen.getByRole("button", { name: "Verify and sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/invalid magic code/i);
    expect(screen.getByLabelText("One-time code")).toBeInTheDocument();
    expect(assign).not.toHaveBeenCalled();
  });

  it("throttles resend with a five-second cooldown", async () => {
    const user = userEvent.setup();
    let generates = 0;
    const resend = deferred<StubRoute>();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          generates += 1;
          if (generates === 1) return { body: JSON.stringify({ key: "k1" }), url };
          return resend.promise;
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("One-time code"), "123456");
    expect(screen.getByRole("button", { name: "Resend code" })).toBeEnabled();
    vi.useFakeTimers();
    try {
      fireEvent.click(screen.getByRole("button", { name: "Resend code" }));
      expect(screen.getByText("Requesting code…")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Verify and sign in" })).toBeDisabled();
      resend.resolve({ body: JSON.stringify({ key: "k2" }), url: "https://api.test/x" });
      await act(async () => {});
      expect(screen.getByText("Resend in 5 seconds")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /resend in/i })).toBeDisabled();
      expect(screen.getByRole("button", { name: "Verify and sign in" })).toBeEnabled();
      fireEvent.click(screen.getByRole("button", { name: /resend in/i }));
      expect(generates).toBe(2);
      await act(async () => {
        vi.advanceTimersByTime(4000);
      });
      expect(screen.getByText("Resend in 1 second")).toBeInTheDocument();
      await act(async () => {
        vi.advanceTimersByTime(1000);
      });
      expect(screen.getByRole("button", { name: "Resend code" })).toBeEnabled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("allows immediate retry when resend fails", async () => {
    const user = userEvent.setup();
    let generates = 0;
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          generates += 1;
          if (generates > 1) throw new Error("offline");
          return { body: JSON.stringify({ key: "k1" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("One-time code");
    await user.click(screen.getByRole("button", { name: "Resend code" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not send a code");
    expect(screen.getByRole("button", { name: "Resend code" })).toBeEnabled();
  });

  it("disables code submit while a submit is in flight", async () => {
    const user = userEvent.setup();
    const submit = deferred<StubRoute>();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          return { body: JSON.stringify({ key: "k" }), url };
        }
        if (url.endsWith("/auth/magic-sign-in/")) return submit.promise;
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("One-time code"), "123456");
    await user.click(screen.getByRole("button", { name: "Verify and sign in" }));
    expect(screen.getByRole("button", { name: "Verify and sign in" })).toBeDisabled();
    submit.resolve({ body: "<html>", url: "https://app.test/contract-acme" });
    await waitFor(() => expect(assign).toHaveBeenCalledWith("https://app.test/contract-acme"));
  });

  it("keeps the code step with a banner when code sign-in is unreachable", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          return { body: JSON.stringify({ key: "k" }), url };
        }
        if (url.endsWith("/auth/magic-sign-in/")) throw new Error("offline");
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("One-time code"), "123456");
    await user.click(screen.getByRole("button", { name: "Verify and sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not reach the server");
    expect(screen.getByLabelText("One-time code")).toBeInTheDocument();
  });

  it("offers the unique-code action on the password step when mail is on", async () => {
    const user = userEvent.setup();
    let generates = 0;
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          generates += 1;
          return { body: JSON.stringify({ key: "k" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("Password");
    await user.click(screen.getByRole("button", { name: "Sign in with unique code" }));
    expect(await screen.findByLabelText("One-time code")).toBeInTheDocument();
    expect(generates).toBe(1);
    expect(screen.getByRole("button", { name: "Resend code" })).toBeEnabled();
  });

  it("reflects the mail-less mode on the password step", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: configBody({ is_smtp_configured: false }), url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("Password");
    expect(screen.getByRole("button", { name: "Go to workspace" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Forgot your password?" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Sign in with unique code" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Forgot your password?" }));
    expect(await screen.findByText(/no SMTP mail configured/i)).toBeInTheDocument();
  });

  it("stays on password with a banner when unique-code generation fails", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) throw new Error("offline");
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("Password");
    await user.click(screen.getByRole("button", { name: "Sign in with unique code" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not send a code");
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
  });

  it("names the workspace when the invitation matches the typed address", async () => {
    const { container } = renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/join/")) return { body: JSON.stringify(invitationFixture()), url };
        throw new Error(`unexpected ${url}`);
      }),
      { invitationId: "invite-1", invitationSlug: "acme", initialEmail: "ada@example.com" }
    );
    expect(await screen.findByRole("heading", { name: "Join Acme" })).toBeInTheDocument();
    expect(screen.getByText("Log in to work with the team.")).toBeInTheDocument();
    expect(container.querySelector("img")).toHaveAttribute("src", "https://cdn.test/acme.png");
    expect(screen.getByLabelText("Work email")).toHaveValue("ada@example.com");
  });

  it("tracks the typed address for the invitation header", async () => {
    const user = userEvent.setup();
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/join/")) return { body: JSON.stringify(invitationFixture()), url };
        throw new Error(`unexpected ${url}`);
      }),
      { invitationId: "invite-1", invitationSlug: "acme", initialEmail: "someone-else@example.com" }
    );
    expect(await screen.findByRole("heading", { name: "Sign in to Pi Dash" })).toBeInTheDocument();
    const field = screen.getByLabelText("Work email");
    await user.clear(field);
    await user.type(field, "ada@example.com");
    expect(await screen.findByRole("heading", { name: "Join Acme" })).toBeInTheDocument();
  });

  it("names the workspace without a logo", async () => {
    const fixture = invitationFixture();
    const workspace = fixture["workspace"] as Record<string, unknown>;
    workspace["logo_url"] = null;
    const { container } = renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/join/")) return { body: JSON.stringify(fixture), url };
        throw new Error(`unexpected ${url}`);
      }),
      { invitationId: "invite-1", invitationSlug: "acme", initialEmail: "ada@example.com" }
    );
    expect(await screen.findByRole("heading", { name: "Join Acme" })).toBeInTheDocument();
    expect(container.querySelector("img")).toBeNull();
  });

  it("names the workspace in sign-up mode after an error deep link", async () => {
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/join/")) return { body: JSON.stringify(invitationFixture()), url };
        throw new Error(`unexpected ${url}`);
      }),
      {
        invitationId: "invite-1",
        invitationSlug: "acme",
        initialEmail: "ada@example.com",
        initialErrorCode: "5092",
      }
    );
    expect(await screen.findByRole("heading", { name: "Join Acme" })).toBeInTheDocument();
    expect(screen.getByText("Create an account to work with the team.")).toBeInTheDocument();
    expect(screen.getByLabelText("One-time code")).toBeInTheDocument();
  });

  it("keeps the generic header when the invitation fetch fails", async () => {
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/join/")) throw new Error("gone");
        throw new Error(`unexpected ${url}`);
      }),
      { invitationId: "invite-1", invitationSlug: "acme", initialEmail: "ada@example.com" }
    );
    expect(await screen.findByRole("heading", { name: "Sign in to Pi Dash" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows a loading marker while the invitation resolves", async () => {
    const fetch = deferred<StubRoute>();
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/join/")) return fetch.promise;
        throw new Error(`unexpected ${url}`);
      }),
      { invitationId: "invite-1", invitationSlug: "acme", initialEmail: "ada@example.com" }
    );
    expect(await screen.findByText("Loading invitation")).toBeInTheDocument();
    expect(screen.getByLabelText("Work email")).toBeInTheDocument();
    fetch.resolve({ body: JSON.stringify(invitationFixture()), url: "https://api.test/x" });
    expect(await screen.findByRole("heading", { name: "Join Acme" })).toBeInTheDocument();
  });

  it("keeps the generic header when the invitation carries no workspace", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(authKeys.invitation("acme", "invite-1"), {
      ...invitationFixture(),
      workspace: null,
    });
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/join/")) throw new Error("must use the cache");
        throw new Error(`unexpected ${url}`);
      }),
      { invitationId: "invite-1", invitationSlug: "acme", initialEmail: "ada@example.com" },
      queryClient
    );
    expect(await screen.findByRole("heading", { name: "Sign in to Pi Dash" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /join acme/i })).toBeNull();
  });

  it("skips the invitation fetch without both invitation params", async () => {
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/join/")) throw new Error("must not fetch");
        throw new Error(`unexpected ${url}`);
      }),
      { invitationSlug: "acme" }
    );
    expect(await screen.findByRole("heading", { name: "Sign in to Pi Dash" })).toBeInTheDocument();
  });

  it("preloads a server error code from the URL on the fixing step", async () => {
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        throw new Error(`unexpected ${url}`);
      }),
      {
        initialErrorCode: "5065",
      }
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(/authentication failed/i);
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
  });

  it("preloads an unknown-account error with a create-account link", async () => {
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        throw new Error(`unexpected ${url}`);
      }),
      {
        initialErrorCode: "5060",
        initialEmail: "ada@example.com",
      }
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(/no account found/i);
    expect(screen.getByLabelText("Work email")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Create an account" })).toHaveAttribute(
      "href",
      "/sign-up?email=ada%40example.com"
    );
  });

  it("raises no banner for an unrecognized preloaded code", async () => {
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        throw new Error(`unexpected ${url}`);
      }),
      {
        initialErrorCode: "9999",
      }
    );
    expect(await screen.findByLabelText("Work email")).toBeInTheDocument();
    expect(screen.queryAllByRole("alert")).toHaveLength(0);
  });

  it("preloads a sign-up code in sign-up mode", async () => {
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        throw new Error(`unexpected ${url}`);
      }),
      {
        initialErrorCode: "5092",
      }
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(/invalid magic code/i);
    expect(screen.getByLabelText("One-time code")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create your account" })).toBeInTheDocument();
  });

  it("recovers a flipped submit through the matrix with a link", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "CREDENTIAL" }), url };
        }
        if (url.endsWith("/auth/sign-up/")) {
          return { body: "<html>", url: "https://app.test/?error_code=5030&error_message=x" };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    // Strong secrets: AUTH-010 refuses weak ones before anything posts, so
    // the server-side 5030 recovery below needs a postable secret.
    await user.type(await screen.findByLabelText("Password"), "Tq7!mZ2@kP9#xR");
    await user.type(screen.getByLabelText("Confirm password"), "Tq7!mZ2@kP9#xR");
    await user.click(screen.getByRole("button", { name: "Create account" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/already exists/i);
    expect(screen.getByLabelText("Work email")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Sign in to Pi Dash" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/?email=ada%40example.com");
  });

  it("surfaces the server message for unrecognized submit failures", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        return { body: "<html>", url: "https://app.test/?error_code=9999&error_message=Custom%20server%20words" };
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("Password"), "secret");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Custom server words");
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
  });

  it("dismissing the banner keeps the step", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: true, status: "CREDENTIAL" }), url };
        }
        return {
          body: "<html>",
          url: "https://app.test/sign-in?error_code=5065&error_message=x",
        };
      })
    );
    await user.type(screen.getByLabelText("Work email"), "ada@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.type(await screen.findByLabelText("Password"), "wrong");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await screen.findByRole("alert");
    await user.click(screen.getByRole("button", { name: "Dismiss error" }));
    expect(screen.queryAllByRole("alert")).toHaveLength(0);
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
  });

  it("replaces the form with the no-methods notice", async () => {
    renderCard(
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
    expect(await screen.findByRole("heading", { name: "Sign-in is unavailable" })).toBeInTheDocument();
    expect(screen.getByText(/contact your administrator/i)).toBeInTheDocument();
    expect(screen.queryByLabelText("Work email")).toBeNull();
    expect(screen.queryByRole("button", { name: /with google/i })).toBeNull();
  });

  it("shows the loading state while config resolves", async () => {
    const config = deferred<StubRoute>();
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return config.promise;
        throw new Error(`unexpected ${url}`);
      })
    );
    expect(await screen.findByText("Loading sign-in options")).toBeInTheDocument();
    config.resolve({ body: CONFIG, url: "https://api.test/x" });
    expect(await screen.findByLabelText("Work email")).toBeInTheDocument();
  });

  it("retries a failed config load", async () => {
    let calls = 0;
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) {
          calls += 1;
          if (calls === 1) throw new Error("offline");
          return { body: CONFIG, url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    expect(await screen.findByRole("button", { name: "Try again" })).toBeInTheDocument();
    expect(screen.getByText(/could not load sign-in/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByLabelText("Work email")).toBeInTheDocument();
    expect(calls).toBe(2);
  });

  it("shows the email form when only code login is enabled", async () => {
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) {
          return { body: configBody({ is_email_password_enabled: false }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    expect(await screen.findByLabelText("Work email")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Sign-in is unavailable" })).toBeNull();
  });

  it("gates weak secrets in flipped sign-up mode without posting", async () => {
    const user = userEvent.setup();
    let posts = 0;
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "CREDENTIAL" }), url };
        }
        if (url.endsWith("/auth/sign-up/")) {
          posts += 1;
          return { body: "<html>", url: "https://app.test/onboarding" };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("Password");
    await user.type(screen.getByLabelText("Password"), "weakpass1");
    await user.type(screen.getByLabelText("Confirm password"), "weakpass1");
    await user.click(screen.getByRole("button", { name: "Create account" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/strong password/i);
    expect(posts).toBe(0);
    expect(assign).not.toHaveBeenCalled();
  });

  it("disables submit on a mismatched confirmation in flipped sign-up mode", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("Password");
    await user.type(screen.getByLabelText("Password"), "Tq7!mZ2@kP9#xR");
    await user.type(screen.getByLabelText("Confirm password"), "Tq7!mZ2@kP9#xR-other");
    expect(await screen.findByText(/don't match/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create account" })).toBeDisabled();
  });
});
