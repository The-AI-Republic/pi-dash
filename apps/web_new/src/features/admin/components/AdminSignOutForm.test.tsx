// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { adminKeys } from "../queries.js";
import { resetAdminStores, useFirstWorkspaceStore } from "../stores.js";
import { ADMIN_THEME_KEY, useAdminThemeStore } from "../theme.js";
import { AdminSignOutForm, SIGN_OUT_POST_PATH } from "./AdminSignOutForm.js";

function stubTransport(): Transport {
  return (url) => {
    const path = new URL(url, "https://api.test").pathname;
    if (path === "/auth/get-csrf-token/") {
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf-9" })),
      });
    }
    return new Promise(() => {});
  };
}

function renderForm(queryClient: QueryClient): void {
  render(
    <QueryClientProvider client={queryClient}>
      <div role="menu">
        <AdminSignOutForm />
      </div>
    </QueryClientProvider>
  );
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

afterEach(() => {
  resetAppClient();
});

describe("AdminSignOutForm", () => {
  it("posts natively to the sign-out endpoint with the forgery token", async () => {
    expect(SIGN_OUT_POST_PATH).toBe("/api/instances/admins/sign-out/");
    createAppClient("https://api.test", stubTransport());
    renderForm(new QueryClient({ defaultOptions: { queries: { retry: false } } }));

    const form = screen.getByTestId("admin-sign-out-form");
    expect(form).toHaveAttribute("method", "post");
    expect(form).toHaveAttribute("action", SIGN_OUT_POST_PATH);
    await waitFor(() =>
      expect(form.querySelector('input[name="csrfmiddlewaretoken"]')).toHaveAttribute("value", "csrf-9")
    );

    const signOut = screen.getByRole("menuitem", { name: "Sign out" });
    expect(signOut).toHaveAttribute("type", "submit");
    expect(signOut).toHaveAttribute("data-testid", "admin-sign-out");
  });

  it("resets admin client state on submit", async () => {
    createAppClient("https://api.test", stubTransport());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(adminKeys.session, { kind: "admin" });
    useAdminThemeStore.getState().setMode("dark");
    useFirstWorkspaceStore.getState().dismiss();
    renderForm(queryClient);

    // jsdom dispatches the submit without navigating, so the handler's
    // reset is what this asserts; the browser performs the native POST.
    fireEvent.submit(screen.getByTestId("admin-sign-out-form"));

    expect(window.localStorage.getItem(ADMIN_THEME_KEY)).toBe("system");
    expect(useAdminThemeStore.getState().mode).toBe("system");
    expect(useFirstWorkspaceStore.getState().dismissed).toBe(false);
    expect(queryClient.getQueryData(adminKeys.session)).toBeUndefined();
  });
});
