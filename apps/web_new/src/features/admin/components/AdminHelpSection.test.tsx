// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar help footer: main-app redirect, external help popover with
// the instance version, and the collapse toggle.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ADMIN_SIDEBAR_COLLAPSED_KEY } from "../adminShell.js";
import { adminKeys } from "../queries.js";
import { resetAdminStores, useAdminSidebarStore } from "../stores.js";
import { AdminHelpSection } from "./AdminHelpSection.js";

function failingTransport(): Transport {
  return (url) =>
    Promise.resolve({
      status: 500,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
    });
}

function renderSection(version: string | "pending"): void {
  createAppClient("https://api.test", failingTransport());
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (version !== "pending") {
    queryClient.setQueryData(adminKeys.bootstrap, {
      instance: { current_version: version, workspaces_exist: true },
    });
  }
  render(
    <QueryClientProvider client={queryClient}>
      <AdminHelpSection />
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

describe("help footer layout", () => {
  it("lays out flat with a labeled redirect when expanded", () => {
    renderSection("9.9.9");
    const footer = screen.getByTestId("admin-help-footer");
    expect(footer.className).not.toContain("flex-col");
    const redirect = screen.getByTestId("admin-help-redirect");
    expect(redirect).toHaveAttribute("href", "/");
    expect(redirect).toHaveTextContent("Redirect to Pi Dash");
    expect(screen.getByRole("button", { name: "Help" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Toggle sidebar" }).querySelector("svg")).toBeInTheDocument();
  });

  it("restacks vertically with an icon-only redirect when collapsed", () => {
    useAdminSidebarStore.getState().setCollapsed(true);
    renderSection("9.9.9");
    const footer = screen.getByTestId("admin-help-footer");
    expect(footer.className).toContain("flex-col");
    const redirect = screen.getByTestId("admin-help-redirect");
    expect(redirect).toHaveAttribute("href", "/");
    expect(redirect.textContent).toBe("");
    expect(screen.getByRole("button", { name: "Toggle sidebar" }).querySelector("svg")).toBeInTheDocument();
  });
});

describe("help popover", () => {
  it("lists the external links and the running version", () => {
    renderSection("9.9.9");
    fireEvent.click(screen.getByRole("button", { name: "Help" }));
    const links = screen.getAllByRole("link", { name: /Documentation|Join our Forum|Report a bug/ });
    expect(links.map((link) => [link.textContent, link.getAttribute("href"), link.getAttribute("target")])).toEqual([
      ["Documentation", "https://github.com/The-AI-Republic/pi-dash/#readme", "_blank"],
      ["Join our Forum", "https://github.com/The-AI-Republic/pi-dash/discussions/", "_blank"],
      ["Report a bug", "https://github.com/The-AI-Republic/pi-dash/issues/new/choose/", "_blank"],
    ]);
    expect(screen.getByTestId("admin-help-version")).toHaveTextContent("Version: v9.9.9");
  });

  it("opens collapsed with the same links", () => {
    useAdminSidebarStore.getState().setCollapsed(true);
    renderSection("9.9.9");
    fireEvent.click(screen.getByRole("button", { name: "Help" }));
    expect(screen.getByRole("link", { name: "Documentation" })).toHaveAttribute(
      "href",
      "https://github.com/The-AI-Republic/pi-dash/#readme"
    );
    expect(screen.getByTestId("admin-help-version")).toHaveTextContent("Version: v9.9.9");
  });

  it("leaves the version blank while the bootstrap is unresolved", () => {
    renderSection("pending");
    fireEvent.click(screen.getByRole("button", { name: "Help" }));
    expect(screen.getByTestId("admin-help-version")).toHaveTextContent("Version: v");
  });
});

describe("collapse toggle", () => {
  it("flips the rail and persists the choice", () => {
    renderSection("9.9.9");
    const toggle = screen.getByRole("button", { name: "Toggle sidebar" });
    fireEvent.click(toggle);
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBe("true");
    fireEvent.click(toggle);
    expect(useAdminSidebarStore.getState().collapsed).toBe(false);
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBe("false");
  });
});
