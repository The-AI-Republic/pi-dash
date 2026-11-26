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

function makeThreadId(tag: string): string {
  // The fetch path validates ids as UUIDs; the cache path does not.
  const suffix = tag
    .replaceAll(/[^0-9a-f]/gi, "1")
    .padEnd(12, "0")
    .slice(0, 12);
  return `11111111-1111-4111-8111-${suffix}`;
}

function makeThread(tag: string, overrides: Partial<AssistantThread> = {}): AssistantThread {
  return {
    id: makeThreadId(tag),
    title: `Thread ${tag}`,
    is_archived: false,
    has_active_turn: false,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
    ...overrides,
  };
}

function renderDialog(options: {
  threads?: AssistantThread[];
  fail?: boolean;
  failRef?: { current: boolean };
  hang?: boolean;
  preset?: boolean;
  initialPath?: string;
}): QueryClient {
  const shouldFail = () => (options.failRef ? options.failRef.current : options.fail === true);
  createAppClient("https://api.test", ((url: string) => {
    if (options.hang === true) return new Promise(() => {}) as never;
    return (async () => ({
      status: shouldFail() ? 403 : 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(shouldFail() ? { error: "role_not_allowed" } : (options.threads ?? [])),
    }))() as never;
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.threads !== undefined && options.preset !== false)
    queryClient.setQueryData(assistantKeys.threads("acme"), options.threads);
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
  return queryClient;
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
      initialPath: `/acme/assistant/${makeThreadId("b")}`,
    });
    await openDialog();
    const dialog = screen.getByRole("dialog", { name: "Chat History" });
    expect(within(dialog).getByRole("link", { name: "Beta" })).toHaveAttribute(
      "href",
      `/acme/assistant/${makeThreadId("b")}`
    );
    const untitled = within(dialog).getByRole("link", { name: "Untitled conversation" });
    expect(untitled).toHaveAttribute("href", `/acme/assistant/${makeThreadId("a")}`);
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

  it("moves focus to the first row once the read settles", async () => {
    renderDialog({
      threads: [makeThread("b", { title: "Beta" }), makeThread("a", { title: "Alpha" })],
      initialPath: "/acme/assistant",
    });
    await openDialog();
    const dialog = screen.getByRole("dialog", { name: "Chat History" });
    const links = within(dialog).getAllByRole("link");
    await waitFor(() => expect(document.activeElement).toBe(links[0]));
  });

  it("falls back to the list itself when there is no row to land on", async () => {
    renderDialog({ threads: [] });
    await openDialog();
    const dialog = screen.getByRole("dialog", { name: "Chat History" });
    await waitFor(() => expect(document.activeElement).toBe(within(dialog).getByRole("navigation")));
  });

  it("leaves focus on the opener while the read is pending", async () => {
    renderDialog({ hang: true });
    await openDialog();
    expect(document.activeElement).toHaveTextContent("Chat History");
    expect(document.activeElement?.tagName).toBe("BUTTON");
  });

  it("keeps focus put when the read recovers while the dialog is open", async () => {
    const failRef = { current: true };
    const queryClient = renderDialog({
      threads: [makeThread("b", { title: "Beta" }), makeThread("a", { title: "Alpha" })],
      failRef,
      preset: false,
      initialPath: "/acme/assistant",
    });
    await openDialog();
    const dialog = screen.getByRole("dialog", { name: "Chat History" });
    const nav = within(dialog).getByRole("navigation");
    await waitFor(() => expect(document.activeElement).toBe(nav));
    failRef.current = false;
    await queryClient.invalidateQueries({ queryKey: assistantKeys.threads("acme") });
    await waitFor(() => expect(within(dialog).getByRole("link", { name: "Beta" })).toBeInTheDocument());
    expect(document.activeElement).toBe(nav);
  });
});
