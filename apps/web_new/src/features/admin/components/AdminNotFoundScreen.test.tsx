// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { createMemoryHistory, createRootRoute, createRouter, RouterProvider } from "@tanstack/react-router";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ADMIN_NOT_FOUND_ART, ADMIN_NOT_FOUND_TITLE, AdminNotFoundScreen } from "./AdminNotFoundScreen.js";

function renderScreen(): void {
  expect(ADMIN_NOT_FOUND_ART).toBe("/illustrations/not-found.svg");
  expect(ADMIN_NOT_FOUND_TITLE.length).toBeGreaterThan(0);
  const rootRoute = createRootRoute({ component: AdminNotFoundScreen });
  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory({ initialEntries: ["/god-mode/no-such-place/"] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

describe("AdminNotFoundScreen", () => {
  it("renders the illustration, message, and shortcut without any guard", async () => {
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("admin-not-found")).toBeInTheDocument());
    expect(screen.getByAltText("404 - Page not found")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Oops! Something went wrong." })).toBeInTheDocument();
    expect(screen.queryByTestId("admin-loader")).not.toBeInTheDocument();
  });

  it("links the shortcut to general settings", async () => {
    renderScreen();
    const shortcut = await screen.findByRole("link", { name: "Go to general settings" });
    expect(shortcut).toHaveAttribute("href", "/god-mode/general/");
  });

  it("owns the tab title and keeps crawlers out", async () => {
    renderScreen();
    await waitFor(() => expect(document.title).toBe(ADMIN_NOT_FOUND_TITLE));
    expect(document.querySelector('meta[name="robots"]')).toHaveAttribute("content", "noindex, nofollow");
  });
});
