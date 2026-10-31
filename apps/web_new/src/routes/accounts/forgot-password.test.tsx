// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import { createClient, type ApiClient, type Transport } from "@pidash/api-client";
import { isRedirect } from "@tanstack/react-router";
import { showToast } from "@pidash/kit";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the auth card: its internals belong to the auth area.
// What this suite owns is the search schema, the guard, plus prop passing.
vi.mock("../../features/auth/components/ForgotPasswordCard.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../features/auth/components/ForgotPasswordCard.js")>()),
  ForgotPasswordCard: (props: { initialEmail?: string }) => (
    <div data-testid="forgot-card" data-email={props.initialEmail ?? ""} />
  ),
}));

import { meQueryOptions } from "../../core/session/queries.js";
import { getToastManager } from "../../shared/shell/index.js";
import { ForgotPasswordScreen, parseForgotPasswordSearch, Route } from "./forgot-password.js";

function throwingTransport(): Transport {
  return (url) => {
    throw new Error(`unexpected fetch ${url}`);
  };
}

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

function queryClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function beforeLoadOf(route: typeof Route) {
  const beforeLoad = route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  return beforeLoad;
}

describe("parseForgotPasswordSearch", () => {
  it("keeps the prefill address and drops anything else", () => {
    expect(parseForgotPasswordSearch({ email: "ada@example.com" })).toEqual({ email: "ada@example.com" });
    expect(parseForgotPasswordSearch({})).toEqual({});
    expect(parseForgotPasswordSearch({ email: "" })).toEqual({});
    expect(parseForgotPasswordSearch({ email: 42 })).toEqual({});
  });
});

describe("forgot-password guard", () => {
  it("renders for signed-out visitors", async () => {
    const client = queryClient();
    const result = await beforeLoadOf(Route)({
      context: { queryClient: client, apiClient: signedOutClient() },
      location: { href: "/accounts/forgot-password" },
    } as never);
    expect(result).toEqual({});
  });

  it("sends signed-in visitors to the entry route", async () => {
    const client = queryClient();
    const apiClient = createClient({ baseUrl: "https://api.test", transport: throwingTransport() });
    client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1" } as never);
    const thrown = await beforeLoadOf(Route)({
      context: { queryClient: client, apiClient },
      location: { href: "/accounts/forgot-password" },
    } as never).then(
      () => null,
      (error: unknown) => error
    );
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: unknown }).options).toMatchObject({ to: "/" });
  });
});

describe("ForgotPasswordScreen", () => {
  it("passes the prefill through to the card", () => {
    render(<ForgotPasswordScreen email="ada@example.com" />);
    expect(screen.getByTestId("forgot-card")).toHaveAttribute("data-email", "ada@example.com");
  });

  it("defaults a bare search to an empty prefill", () => {
    render(<ForgotPasswordScreen />);
    expect(screen.getByTestId("forgot-card")).toHaveAttribute("data-email", "");
  });

  it("renders the route component from the search", () => {
    const component = Route.options.component;
    if (!component) throw new Error("missing component");
    const useSearch = vi.spyOn(Route, "useSearch").mockReturnValue({ email: "ada@example.com" });
    try {
      render(React.createElement(component as () => React.ReactElement));
      expect(screen.getByTestId("forgot-card")).toHaveAttribute("data-email", "ada@example.com");
    } finally {
      useSearch.mockRestore();
    }
  });

  // Last: leaves a toast behind in the (per-file) manager on purpose.
  it("mounts the shared toast host", async () => {
    render(<ForgotPasswordScreen />);
    act(() => {
      showToast(getToastManager(), { title: "Host is live" });
    });
    expect(await screen.findByText("Host is live")).toBeInTheDocument();
  });
});
