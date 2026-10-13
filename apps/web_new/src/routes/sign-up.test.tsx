// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApiClient, Transport, TransportInit } from "@pidash/api-client";

// Stand-in for the auth card: its internals belong to the auth area.
// What this suite owns is the search schema, the signed-out-only gate,
// plus prop passing. The real search primitives stay intact.
vi.mock("../features/auth/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../features/auth/index.js")>();
  return {
    ...actual,
    SignUpCard: (props: {
      next?: string;
      initialEmail?: string;
      initialErrorCode?: string | null;
      invitationId?: string;
      invitationSlug?: string;
    }) => (
      <div
        data-testid="sign-up-card"
        data-next={props.next ?? ""}
        data-email={props.initialEmail ?? ""}
        data-error={props.initialErrorCode ?? ""}
        data-invitation={props.invitationId ?? ""}
        data-slug={props.invitationSlug ?? ""}
      />
    ),
  };
});

import { createAppClient, resetAppClient } from "../core/api/client.js";
import { parseSignUpSearch, redirectSignedInUser, SignUpScreen } from "./sign-up.js";
import { routeTree } from "../routeTree.gen.js";

interface StubRoute {
  body: string;
  url: string;
}

function stubTransport(handler: (url: string, init: TransportInit) => StubRoute): Transport {
  return async (url, init) => {
    const response = handler(url, init);
    return {
      status: 200,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const ME = JSON.stringify({
  id: "11111111-1111-4111-8111-111111111111",
  email: "ada@example.com",
  username: "ada",
  first_name: "Ada",
  last_name: "Lovelace",
  display_name: "Ada Lovelace",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-01T00:00:00.000Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
});

const WORKSPACES = JSON.stringify([
  {
    id: "22222222-2222-4222-8222-222222222222",
    name: "Acme",
    slug: "acme",
    logo_url: null,
    total_members: 1,
    role: 20,
  },
]);

function guardContext(handler: (url: string) => StubRoute): { queryClient: QueryClient; apiClient: ApiClient } {
  const apiClient = createAppClient(
    "https://api.test",
    stubTransport((url) => handler(url))
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return { queryClient, apiClient };
}

interface ThrownRedirect extends Response {
  options: { to?: unknown; params?: unknown };
}

async function catchRedirect(promise: Promise<unknown>): Promise<ThrownRedirect> {
  try {
    await promise;
  } catch (error) {
    expect(error).toBeInstanceOf(Response);
    return error as ThrownRedirect;
  }
  throw new Error("expected the guard to redirect");
}

describe("parseSignUpSearch", () => {
  it("keeps every known param", () => {
    expect(
      parseSignUpSearch({
        next_path: "/acme",
        email: "ada@example.com",
        error_code: "5035",
        invitation_id: "invite-1",
        slug: "acme",
      })
    ).toEqual({
      next_path: "/acme",
      email: "ada@example.com",
      error_code: "5035",
      invitation_id: "invite-1",
      slug: "acme",
    });
  });

  it("drops unknown params and unsafe return paths", () => {
    expect(parseSignUpSearch({ next_path: "https://evil.test/", email: "", unknown: "x" })).toEqual({});
  });
});

describe("redirectSignedInUser", () => {
  afterEach(() => {
    resetAppClient();
  });

  it("lets signed-out visitors through to the card", async () => {
    const context = guardContext((url) => {
      if (url.endsWith("/api/users/me/")) throw new Error("signed out");
      throw new Error(`unexpected ${url}`);
    });
    await expect(redirectSignedInUser({ context, search: {} })).resolves.toEqual({});
  });

  it("sends signed-in visitors to their return path first", async () => {
    const context = guardContext((url) => {
      if (url.endsWith("/api/users/me/")) return { body: ME, url };
      throw new Error(`unexpected ${url}`);
    });
    const redirect = await catchRedirect(
      redirectSignedInUser({ context, search: { next_path: "/acme/projects/p1/issues" } })
    );
    expect(redirect.headers.get("Location")).toBe("/acme/projects/p1/issues");
  });

  it("sends signed-in visitors without a return path to their first workspace", async () => {
    const context = guardContext((url) => {
      if (url.endsWith("/api/users/me/")) return { body: ME, url };
      if (url.endsWith("/api/users/me/workspaces/")) return { body: WORKSPACES, url };
      throw new Error(`unexpected ${url}`);
    });
    const redirect = await catchRedirect(redirectSignedInUser({ context, search: {} }));
    expect(redirect.options.to).toBe("/$ws");
    expect(redirect.options.params).toEqual({ ws: "acme" });
  });

  it("renders the card for signed-in visitors with nowhere to go", async () => {
    const context = guardContext((url) => {
      if (url.endsWith("/api/users/me/")) return { body: ME, url };
      if (url.endsWith("/api/users/me/workspaces/")) return { body: "[]", url };
      throw new Error(`unexpected ${url}`);
    });
    await expect(redirectSignedInUser({ context, search: {} })).resolves.toEqual({});
  });

  it("renders the card when the workspace list fails to load", async () => {
    const context = guardContext((url) => {
      if (url.endsWith("/api/users/me/")) return { body: ME, url };
      throw new Error("workspaces unavailable");
    });
    await expect(redirectSignedInUser({ context, search: {} })).resolves.toEqual({});
  });
});

describe("SignUpScreen", () => {
  it("passes the search params to the card", () => {
    render(
      <SignUpScreen
        search={{
          next_path: "/acme",
          email: "ada@example.com",
          error_code: "5035",
          invitation_id: "invite-1",
          slug: "acme",
        }}
      />
    );
    const card = screen.getByTestId("sign-up-card");
    expect(card).toHaveAttribute("data-next", "/acme");
    expect(card).toHaveAttribute("data-email", "ada@example.com");
    expect(card).toHaveAttribute("data-error", "5035");
    expect(card).toHaveAttribute("data-invitation", "invite-1");
    expect(card).toHaveAttribute("data-slug", "acme");
  });

  it("falls back to empty email and null error", () => {
    render(<SignUpScreen search={{}} />);
    const card = screen.getByTestId("sign-up-card");
    expect(card).toHaveAttribute("data-next", "");
    expect(card).toHaveAttribute("data-email", "");
    expect(card).toHaveAttribute("data-error", "");
  });
});

describe("Route", () => {
  afterEach(() => {
    resetAppClient();
  });

  it("renders the card through the search schema", async () => {
    const context = guardContext((url) => {
      if (url.endsWith("/api/users/me/")) throw new Error("signed out");
      throw new Error(`unexpected ${url}`);
    });
    const router = createRouter({
      routeTree,
      history: createMemoryHistory({
        initialEntries: ["/sign-up?email=ada%40example.com&next_path=%2Facme"],
      }),
      context,
    });
    render(<RouterProvider router={router} />);
    const card = await screen.findByTestId("sign-up-card");
    expect(card).toHaveAttribute("data-email", "ada@example.com");
    expect(card).toHaveAttribute("data-next", "/acme");
  });
});
