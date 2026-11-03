// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { createMemoryHistory, createRouter, isRedirect, RouterProvider } from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApiClient, Transport, TransportInit } from "@pidash/api-client";
import { ApiError } from "@pidash/api-client";

// Stand-in for the auth card: its internals belong to the auth area.
// What this suite owns is the search schema, the signed-out-only gate,
// plus prop passing. The real search primitives stay intact.
vi.mock("../features/auth/components/SignUpCard.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../features/auth/components/SignUpCard.js")>();
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

function guardContext(handler: (url: string) => StubRoute): { queryClient: QueryClient; apiClient: ApiClient } {
  const apiClient = createAppClient(
    "https://api.test",
    stubTransport((url) => handler(url))
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return { queryClient, apiClient };
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
  const me = { is_password_autoset: false };
  const onboarded = { is_onboarded: true };

  function stubInput(values: Record<string, unknown>, search: Record<string, unknown> = {}) {
    return {
      context: {
        apiClient: {},
        queryClient: {
          ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
            const value = values[options.queryKey[1] as string];
            if (value instanceof Error) throw value;
            if (value === undefined) throw new Error("no fixture");
            return value;
          },
        },
      },
      search,
    } as never;
  }

  async function landingOf(args: never): Promise<string> {
    try {
      await redirectSignedInUser(args);
    } catch (error) {
      expect(isRedirect(error)).toBe(true);
      return (error as unknown as { options: { href: string } }).options.href;
    }
    expect.unreachable("expected a redirect");
  }

  it("lets signed-out visitors through to the card", async () => {
    await expect(
      redirectSignedInUser(stubInput({ me: new ApiError({ status: 401, code: "http", message: "signed out" }) }))
    ).resolves.toEqual({});
  });

  it("sends finished members to their return path first", async () => {
    await expect(
      landingOf(stubInput({ me, profile: onboarded }, { next_path: "/acme/projects/p1/issues" }))
    ).resolves.toBe("/acme/projects/p1/issues");
  });

  it("sends finished members to their last workspace, not the first in the list", async () => {
    const settings = { workspace: { last_workspace_slug: "beta", fallback_workspace_slug: null } };
    const workspaces = [{ slug: "acme" }, { slug: "beta" }];
    await expect(landingOf(stubInput({ me, profile: onboarded, settings, workspaces }))).resolves.toBe("/beta");
  });

  it("falls back to the fallback workspace when the last one is gone", async () => {
    const settings = { workspace: { last_workspace_slug: "gone", fallback_workspace_slug: "acme" } };
    const workspaces = [{ slug: "acme" }];
    await expect(landingOf(stubInput({ me, profile: onboarded, settings, workspaces }))).resolves.toBe("/acme");
  });

  it("sends finished members with nowhere to go to workspace creation", async () => {
    const settings = { workspace: { last_workspace_slug: null, fallback_workspace_slug: null } };
    await expect(landingOf(stubInput({ me, profile: onboarded, settings, workspaces: [] }))).resolves.toBe(
      "/create-workspace"
    );
  });

  it("sends finished members to workspace creation when the landing reads fail", async () => {
    const failed = new Error("landing reads unavailable");
    await expect(landingOf(stubInput({ me, profile: onboarded, settings: failed, workspaces: failed }))).resolves.toBe(
      "/create-workspace"
    );
  });

  it("sends unfinished members to onboarding", async () => {
    const profile = { is_onboarded: false, onboarding_step: {} };
    await expect(landingOf(stubInput({ me, profile }))).resolves.toBe("/onboarding");
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
      if (url.endsWith("/api/users/me/")) throw new ApiError({ status: 401, code: "http", message: "signed out" });
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
