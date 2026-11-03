// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar account menu: identity, theme switch, and sign-out, hanging
// off the avatar trigger expanded and collapsed.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { AdminSession, Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { adminKeys } from "../queries.js";
import { resetAdminStores, useAdminSidebarStore } from "../stores.js";
import { ADMIN_THEME_KEY, useAdminThemeStore } from "../theme.js";
import { AdminAccountMenu } from "./AdminAccountMenu.js";

const ADMIN: AdminSession = {
  kind: "admin",
  admin: { email: "admin@example.com", display_name: "Ada Admin", avatar_url: null } as never,
};

function failingTransport(): Transport {
  return (url) =>
    Promise.resolve({
      status: 500,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
    });
}

function renderMenu(session: AdminSession | "pending"): void {
  createAppClient("https://api.test", failingTransport());
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (session !== "pending") {
    queryClient.setQueryData(adminKeys.session, session);
  }
  queryClient.setQueryData(adminKeys.csrf, { csrf_token: "csrf-9" });
  render(
    <QueryClientProvider client={queryClient}>
      <AdminAccountMenu />
    </QueryClientProvider>
  );
}

function openMenu(): void {
  fireEvent.click(screen.getByRole("button", { name: "Admin account" }));
  expect(screen.getByRole("menu", { name: "Admin account" })).toBeInTheDocument();
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
});

describe("admin account trigger", () => {
  it("shows the admin label and avatar trigger when expanded", () => {
    renderMenu(ADMIN);
    expect(screen.getByTestId("admin-account-label")).toHaveTextContent("Instance admin");
    expect(screen.getByRole("button", { name: "Admin account" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Ada Admin" })).toBeInTheDocument();
  });

  it("hides the label but keeps the trigger when collapsed", () => {
    useAdminSidebarStore.getState().setCollapsed(true);
    renderMenu(ADMIN);
    expect(screen.queryByTestId("admin-account-label")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Admin account" })).toBeInTheDocument();
  });

  it("names the avatar from the email when no display name is set", () => {
    renderMenu({ kind: "admin", admin: { email: "admin@example.com", display_name: "", avatar_url: null } } as never);
    expect(screen.getByRole("img", { name: "admin@example.com" })).toBeInTheDocument();
  });

  it("falls back to the admin label with no session to name", () => {
    renderMenu({ kind: "anonymous" });
    expect(screen.getByRole("img", { name: "Instance admin" })).toBeInTheDocument();
  });

  it("renders the avatar image when the session carries one", () => {
    renderMenu({
      kind: "admin",
      admin: { email: "admin@example.com", display_name: "Ada", avatar_url: "https://cdn.test/a.png" },
    } as never);
    expect(screen.getByRole("img", { name: "Ada" }).querySelector("img")).toHaveAttribute(
      "src",
      "https://cdn.test/a.png"
    );
  });
});

describe("admin account menu", () => {
  it("shows the account email, theme item, and sign-out with its CSRF token", () => {
    renderMenu(ADMIN);
    openMenu();
    expect(screen.getByTestId("admin-account-email")).toHaveTextContent("admin@example.com");
    expect(screen.getByRole("menuitem", { name: "Switch to dark mode" })).toBeInTheDocument();
    const signOut = screen.getAllByRole("menuitem", { name: "Sign out" });
    expect(signOut).toHaveLength(1);
    const form = screen.getByTestId("admin-sign-out-form");
    expect(form).toHaveAttribute("action", "/api/instances/admins/sign-out/");
    expect(form.querySelector('input[name="csrfmiddlewaretoken"]')).toHaveAttribute("value", "csrf-9");
  });

  it("renders the same menu off the collapsed trigger", () => {
    useAdminSidebarStore.getState().setCollapsed(true);
    renderMenu(ADMIN);
    openMenu();
    expect(screen.getByTestId("admin-account-email")).toHaveTextContent("admin@example.com");
    expect(screen.getByRole("menuitem", { name: "Switch to dark mode" })).toBeInTheDocument();
    expect(screen.getAllByRole("menuitem", { name: "Sign out" })).toHaveLength(1);
  });

  it("renders an empty identity while the session is unresolved", () => {
    renderMenu("pending");
    openMenu();
    expect(screen.getByTestId("admin-account-email")).toHaveTextContent("");
    expect(screen.getByRole("menuitem", { name: "Switch to dark mode" })).toBeInTheDocument();
  });
});

describe("admin theme switch", () => {
  it("flips to dark and persists it", () => {
    useAdminThemeStore.getState().setMode("light");
    renderMenu(ADMIN);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Switch to dark mode" }));
    expect(window.localStorage.getItem(ADMIN_THEME_KEY)).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    openMenu();
    expect(screen.getByRole("menuitem", { name: "Switch to light mode" })).toBeInTheDocument();
  });

  it("flips to light and persists it", () => {
    useAdminThemeStore.getState().setMode("dark");
    renderMenu(ADMIN);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Switch to light mode" }));
    expect(window.localStorage.getItem(ADMIN_THEME_KEY)).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("names the target off the resolved mode when following the system", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((query: string) => ({ matches: query === "(prefers-color-scheme: dark)", media: query }))
    );
    useAdminThemeStore.getState().setMode("system");
    renderMenu(ADMIN);
    openMenu();
    expect(screen.getByRole("menuitem", { name: "Switch to light mode" })).toBeInTheDocument();
  });
});
