// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { createClient, type ApiClient, type Transport } from "@pidash/api-client";
import { isRedirect } from "@tanstack/react-router";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the auth card: its internals belong to the auth area.
// What this suite owns is the search schema, the guard, plus prop passing.
vi.mock("../../features/auth/components/ResetPasswordCard.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../features/auth/components/ResetPasswordCard.js")>()),
  ResetPasswordCard: (props: { uid?: string; token?: string; email?: string }) => (
    <div
      data-testid="reset-card"
      data-uid={props.uid ?? ""}
      data-token={props.token ?? ""}
      data-email={props.email ?? ""}
    />
  ),
}));

import { meQueryOptions } from "../../core/session/queries.js";
import { parseResetPasswordSearch, ResetPasswordScreen, Route } from "./reset-password.js";

function signedOutClient(): ApiClient {
  return createClient({
    baseUrl: "https://api.test",
    transport: async (url) => ({
      status: 401,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify({ detail: "signed out" }),
    }),
  });
}

function throwingClient(): ApiClient {
  const transport: Transport = (url) => {
    throw new Error(`unexpected fetch ${url}`);
  };
  return createClient({ baseUrl: "https://api.test", transport });
}

function queryClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function beforeLoadOf(route: typeof Route) {
  const beforeLoad = route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  return beforeLoad;
}

describe("parseResetPasswordSearch", () => {
  it("keeps the link params and drops anything else", () => {
    expect(parseResetPasswordSearch({ uidb64: "uid", token: "tok", email: "ada@example.com" })).toEqual({
      uidb64: "uid",
      token: "tok",
      email: "ada@example.com",
    });
    expect(parseResetPasswordSearch({})).toEqual({});
    expect(parseResetPasswordSearch({ uidb64: "", token: null, email: 42 })).toEqual({});
  });
});

describe("reset-password guard", () => {
  it("renders for signed-out visitors", async () => {
    const client = queryClient();
    const result = await beforeLoadOf(Route)({
      context: { queryClient: client, apiClient: signedOutClient() },
      location: { href: "/accounts/reset-password" },
    } as never);
    expect(result).toEqual({});
  });

  it("sends signed-in visitors to the entry route", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1" } as never);
    const thrown = await beforeLoadOf(Route)({
      context: { queryClient: client, apiClient },
      location: { href: "/accounts/reset-password" },
    } as never).then(
      () => null,
      (error: unknown) => error
    );
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: unknown }).options).toMatchObject({ to: "/" });
  });
});

describe("ResetPasswordScreen", () => {
  it("passes the link through to the card", () => {
    render(<ResetPasswordScreen uidb64="uid" token="tok" email="ada@example.com" />);
    const card = screen.getByTestId("reset-card");
    expect(card).toHaveAttribute("data-uid", "uid");
    expect(card).toHaveAttribute("data-token", "tok");
    expect(card).toHaveAttribute("data-email", "ada@example.com");
  });

  it("defaults a bare search to an empty link", () => {
    render(<ResetPasswordScreen />);
    const card = screen.getByTestId("reset-card");
    expect(card).toHaveAttribute("data-uid", "");
    expect(card).toHaveAttribute("data-token", "");
    expect(card).toHaveAttribute("data-email", "");
  });

  it("renders the route component from the search", () => {
    const component = Route.options.component;
    if (!component) throw new Error("missing component");
    const useSearch = vi.spyOn(Route, "useSearch").mockReturnValue({ uidb64: "uid", token: "tok" });
    try {
      render(React.createElement(component as () => React.ReactElement));
      const card = screen.getByTestId("reset-card");
      expect(card).toHaveAttribute("data-uid", "uid");
      expect(card).toHaveAttribute("data-token", "tok");
    } finally {
      useSearch.mockRestore();
    }
  });
});
