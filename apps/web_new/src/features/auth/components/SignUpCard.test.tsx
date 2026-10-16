// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { SignUpCard } from "../index.js";
import type { SignUpCardProps } from "../index.js";

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

function renderCard(transport: Transport, props: SignUpCardProps = {}, queryClient?: QueryClient) {
  createAppClient("https://api.test", transport);
  const client = queryClient ?? new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { container } = render(
    <QueryClientProvider client={client}>
      <SignUpCard {...props} />
    </QueryClientProvider>
  );
  return { queryClient: client, container };
}

async function renderReadyCard(transport: Transport, props: SignUpCardProps = {}) {
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

function hiddenNextPath(container: HTMLElement): string | null {
  return container.querySelector('input[type="hidden"][name="next_path"]')?.getAttribute("value") ?? null;
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

describe("SignUpCard", () => {
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

  it("renders the sign-up home", async () => {
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        throw new Error(`unexpected ${url}`);
      })
    );
    expect(screen.getByRole("region", { name: "Sign up" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create your account" })).toBeInTheDocument();
  });

  it("creates the account on the password path and lands on onboarding", async () => {
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
          return { body: "<html>", url: "https://app.test/onboarding/" };
        }
        throw new Error(`unexpected ${url}`);
      }),
      { next: "/acme/projects/p1/issues" }
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByLabelText("Password")).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm password")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Password"), "Tq7!mZ2@kP9#xR");
    await user.type(screen.getByLabelText("Confirm password"), "Tq7!mZ2@kP9#xR");
    await user.click(screen.getByRole("button", { name: "Create account" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith("https://app.test/onboarding/"));
    expect(posts).toHaveLength(1);
    expect(posts[0]).toContain("email=new%40example.com");
    expect(posts[0]).toContain("confirm_password=Tq7%21mZ2%40kP9%23xR");
    expect(posts[0]).toContain("next_path=%2Facme%2Fprojects%2Fp1%2Fissues");
  });

  it("creates the account on the code path", async () => {
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
          return { body: "<html>", url: "https://app.test/onboarding/" };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByLabelText("One-time code")).toBeInTheDocument();
    await user.type(screen.getByLabelText("One-time code"), "123456");
    await user.click(screen.getByRole("button", { name: "Verify and create account" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith("https://app.test/onboarding/"));
  });

  it("flips known addresses into sign-in mode", async () => {
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
    expect(screen.queryByLabelText("Confirm password")).toBeNull();
    expect(screen.getByRole("heading", { name: "Sign in to Pi Dash" })).toBeInTheDocument();
  });

  it("refuses a weak secret without posting", async () => {
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
          return { body: "<html>", url: "https://app.test/onboarding/" };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("Password");
    await user.type(screen.getByLabelText("Password"), "weakpass1");
    await user.type(screen.getByLabelText("Confirm password"), "weakpass1");
    expect(screen.queryByText(/don't match/i)).toBeNull();
    expect(screen.getByRole("button", { name: "Create account" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Create account" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/strong password/i);
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    expect(posts).toBe(0);
    expect(assign).not.toHaveBeenCalled();
  });

  it("blocks submit on a mismatched confirmation once it outgrows the secret", async () => {
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

  it("shows the mismatch after the confirmation loses focus even while shorter", async () => {
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
    await user.type(screen.getByLabelText("Confirm password"), "short");
    expect(screen.queryByText(/don't match/i)).toBeNull();
    await user.click(screen.getByLabelText("Password"));
    expect(await screen.findByText(/don't match/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create account" })).toBeDisabled();
  });

  it("disables submit while the secret field is empty", async () => {
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
    expect(screen.getByRole("button", { name: "Create account" })).toBeDisabled();
  });

  it("shows the strength hint while the secret is weak and hides it once strong", async () => {
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
    expect(screen.queryByRole("group", { name: "Password strength" })).toBeNull();
    await user.type(screen.getByLabelText("Password"), "weakpass1");
    expect(await screen.findByRole("group", { name: "Password strength" })).toBeInTheDocument();
    await user.clear(screen.getByLabelText("Password"));
    await user.type(screen.getByLabelText("Password"), "Tq7!mZ2@kP9#xR");
    await waitFor(() => expect(screen.queryByRole("group", { name: "Password strength" })).toBeNull());
  });

  it("carries the return path as a hidden field through the password step", async () => {
    const user = userEvent.setup();
    const { container } = await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      }),
      { next: "/acme/projects/p1/issues" }
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("Password");
    expect(hiddenNextPath(container)).toBe("/acme/projects/p1/issues");
  });

  it("carries the return path as a hidden field through the code step", async () => {
    const user = userEvent.setup();
    const { container } = await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          return { body: JSON.stringify({ key: "magic_new@example.com" }), url };
        }
        throw new Error(`unexpected ${url}`);
      }),
      { next: "/acme" }
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("One-time code");
    expect(hiddenNextPath(container)).toBe("/acme");
  });

  it("renders no hidden return path when none was supplied", async () => {
    const user = userEvent.setup();
    const { container } = await renderReadyCard(
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
    expect(hiddenNextPath(container)).toBeNull();
  });

  it("resets to the sign-up home when the address is cleared", async () => {
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
    await user.click(screen.getByRole("button", { name: "Clear email" }));
    expect(assign).toHaveBeenCalledWith("/sign-up");
    expect(screen.getByLabelText("Work email")).toHaveValue("");
    expect(screen.getByRole("heading", { name: "Create your account" })).toBeInTheDocument();
  });

  it("keeps the create-account label when mail is unconfigured", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: configBody({ is_smtp_configured: false }), url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "CREDENTIAL" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("Password");
    expect(screen.getByRole("button", { name: "Create account" })).toBeInTheDocument();
  });

  it("recovers a link-carried sign-up error in sign-up mode", async () => {
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        throw new Error(`unexpected ${url}`);
      }),
      { initialEmail: "new@example.com", initialErrorCode: "5015" }
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(/sign up disabled/i);
    expect(screen.getByRole("heading", { name: "Create your account" })).toBeInTheDocument();
  });

  it("stays in sign-up mode when the email check is unreachable", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) throw new Error("offline");
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/could not reach the server/i);
    expect(screen.getByRole("heading", { name: "Create your account" })).toBeInTheDocument();
  });

  it("stays in sign-up mode when code generation fails on email submit", async () => {
    const user = userEvent.setup();
    await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) throw new Error("offline");
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/could not send a code/i);
    expect(screen.getByLabelText("Work email")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create your account" })).toBeInTheDocument();
  });

  it("renders no hidden return path on the code step when none was supplied", async () => {
    const user = userEvent.setup();
    const { container } = await renderReadyCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return { body: CONFIG, url };
        if (url.endsWith("/auth/email-check/")) {
          return { body: JSON.stringify({ existing: false, status: "MAGIC_CODE" }), url };
        }
        if (url.endsWith("/auth/magic-generate/")) {
          return { body: JSON.stringify({ key: "magic_new@example.com" }), url };
        }
        throw new Error(`unexpected ${url}`);
      })
    );
    await user.type(screen.getByLabelText("Work email"), "new@example.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByLabelText("One-time code");
    expect(hiddenNextPath(container)).toBeNull();
  });

  it("shows the sign-up loading state while config resolves", async () => {
    const config = deferred<StubRoute>();
    renderCard(
      stubTransport((url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
        if (url.endsWith("/api/instances/")) return config.promise;
        throw new Error(`unexpected ${url}`);
      })
    );
    expect(await screen.findByText("Loading sign-up options")).toBeInTheDocument();
    config.resolve({ body: CONFIG, url: "https://api.test/x" });
    expect(await screen.findByLabelText("Work email")).toBeInTheDocument();
  });

  it("retries a failed sign-up config load", async () => {
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
    expect(screen.getByText(/could not load sign-up/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByLabelText("Work email")).toBeInTheDocument();
    expect(calls).toBe(2);
  });
});
