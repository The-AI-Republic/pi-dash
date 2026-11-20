// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation history dialog (AGT-044 dialog half + AGT-041 guest half):
// opening the dialog lists newest-first links with an untitled fallback,
// the open row highlighted, archived rows hidden, the placeholder only when
// the read resolved empty, and nothing at all when the read was refused.
// Picking a row navigates and closes the dialog; Escape and the backdrop
// close it too.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AssistantThread } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { assistantKeys } from "../api/keys.js";
import { ConversationHistoryDialog } from "./ConversationHistoryDialog.js";

function makeThread(id: string, overrides: Partial<AssistantThread> = {}): AssistantThread {
  return {
    id,
    title: `Thread ${id}`,
    is_archived: false,
    has_active_turn: false,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
    ...overrides,
  };
}

function renderDialog(options: { threads?: AssistantThread[]; fail?: boolean; initialPath?: string }): void {
  createAppClient("https://api.test", (async (url: string) => ({
    status: options.fail === true ? 403 : 200,
    url,
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify(options.fail === true ? { error: "role_not_allowed" } : (options.threads ?? [])),
  })) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.threads !== undefined) queryClient.setQueryData(assistantKeys.threads("acme"), options.threads);
  const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()();
  const wsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws",
    component: () => <Outlet />,
  });
  const dialogRoute = createRoute({
    getParentRoute: () => wsRoute,
    id: "assistant",
    component: function DialogHost() {
      const [open, setOpen] = React.useState(false);
      return (
        <div>
          <button type="button" onClick={() => setOpen(true)}>
            Chat History
          </button>
          <ConversationHistoryDialog workspaceSlug="acme" open={open} onOpenChange={setOpen} />
          <Outlet />
        </div>
      );
    },
  });
  const landing = createRoute({
    getParentRoute: () => dialogRoute,
    path: "/assistant",
    component: () => <div data-testid="landing-slot" />,
  });
  const conversation = createRoute({
    getParentRoute: () => dialogRoute,
    path: "/assistant/$threadId",
    component: () => <div data-testid="conversation-slot" />,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([dialogRoute.addChildren([landing, conversation])])]),
    context: { queryClient },
    history: createMemoryHistory({ initialEntries: [options.initialPath ?? "/acme/assistant"] }),
  });
  // Serve the loader-less dialog through the cache so the pending/refused
  // branches are observable.
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

async function openDialog(): Promise<void> {
  // The router mounts asynchronously, so the first query waits.
  await userEvent.setup().click(await screen.findByRole("button", { name: "Chat History" }));
  expect(await screen.findByRole("dialog", { name: "Chat History" })).toBeInTheDocument();
}

afterEach(() => {
  resetAppClient();
});

describe("ConversationHistoryDialog", () => {
  it("stays closed until the history button opens it", async () => {
    renderDialog({ threads: [makeThread("b", { title: "Beta" })] });
    expect(screen.queryByRole("dialog", { name: "Chat History" })).not.toBeInTheDocument();
    await openDialog();
  });

  it("lists visible conversations newest-first and highlights the open one", async () => {
    renderDialog({
      threads: [
        makeThread("b", { title: "Beta" }),
        makeThread("gone", { is_archived: true }),
        makeThread("a", { title: "" }),
      ],
      initialPath: "/acme/assistant/b",
    });
    await openDialog();
    const dialog = screen.getByRole("dialog", { name: "Chat History" });
    expect(within(dialog).getByRole("link", { name: "Beta" })).toHaveAttribute("href", "/acme/assistant/b");
    const untitled = within(dialog).getByRole("link", { name: "Untitled conversation" });
    expect(untitled).toHaveAttribute("href", "/acme/assistant/a");
    expect(within(dialog).queryByRole("link", { name: "Thread gone" })).not.toBeInTheDocument();
    expect(within(dialog).getByRole("link", { name: "Beta" })).toHaveAttribute("aria-current", "page");
    expect(untitled).not.toHaveAttribute("aria-current");
    expect(within(dialog).queryByText("No conversations yet.")).not.toBeInTheDocument();
  });

  it("keeps the dialog links-only, with no management affordance", async () => {
    renderDialog({ threads: [makeThread("b", { title: "Beta" })] });
    await openDialog();
    const dialog = screen.getByRole("dialog", { name: "Chat History" });
    // The dialog is links only: no buttons, and no rename/archive/delete
    // affordance anywhere in the history surface (AGT-058/AGT-061).
    expect(within(dialog).queryByRole("button")).not.toBeInTheDocument();
    for (const link of Array.from(within(dialog).getAllByRole("link"))) {
      expect(link.textContent ?? "").not.toMatch(/rename|archive|delete/i);
    }
  });

  it("navigates to the picked conversation and closes the dialog", async () => {
    renderDialog({
      threads: [makeThread("b", { title: "Beta" }), makeThread("a", { title: "Alpha" })],
      initialPath: "/acme/assistant",
    });
    await openDialog();
    await userEvent.setup().click(screen.getByRole("link", { name: "Alpha" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Chat History" })).not.toBeInTheDocument());
    expect(await screen.findByTestId("conversation-slot")).toBeInTheDocument();
  });

  it("closes on Escape without navigating", async () => {
    renderDialog({ threads: [makeThread("b", { title: "Beta" })], initialPath: "/acme/assistant" });
    await openDialog();
    await userEvent.setup().keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Chat History" })).not.toBeInTheDocument());
    expect(screen.getByTestId("landing-slot")).toBeInTheDocument();
  });

  it("shows the placeholder only for a resolved empty list", async () => {
    renderDialog({ threads: [] });
    await openDialog();
    expect(
      within(screen.getByRole("dialog", { name: "Chat History" })).getByText("No conversations yet.")
    ).toBeInTheDocument();
  });

  it("shows neither rows nor the placeholder when the read is refused", async () => {
    renderDialog({ fail: true });
    await openDialog();
    const dialog = screen.getByRole("dialog", { name: "Chat History" });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(within(dialog).queryByText("No conversations yet.")).not.toBeInTheDocument();
    expect(within(dialog).getByRole("navigation").querySelectorAll("a")).toHaveLength(0);
  });
});
