// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Duplicate-target picker (INT-023): recent rows on open, search as
// the viewer types, the current request excluded, pick-to-apply, and
// the empty, failed and pending states.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { IssueSearchHit, TransportResponse } from "@pidash/api-client";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { DUPLICATE_DIALOG_TITLE, IntakeDuplicateDialog } from "./IntakeDuplicateDialog.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CURRENT = "11111111-1111-4111-8111-111111111111";
const TARGET = "22222222-2222-4222-8222-222222222222";
const SEARCH_PATH = `/api/workspaces/${WS}/projects/${PROJECT}/search-issues/`;

function hit(overrides: Partial<IssueSearchHit> = {}): IssueSearchHit {
  return {
    name: "Original work",
    id: TARGET,
    start_date: null,
    sequence_id: 3,
    project__name: "Atlas",
    project__identifier: "APO",
    project_id: PROJECT,
    workspace__slug: WS,
    state__name: "Todo",
    state__group: "unstarted",
    state__color: "#444444",
    ...overrides,
  };
}

interface SearchServer {
  urls: string[];
  status: number;
  hits: IssueSearchHit[];
}

function stubTransport(server: SearchServer): void {
  createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
    const method = init?.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    if (url.includes(SEARCH_PATH) && method === "GET") {
      server.urls.push(url);
      if (server.status >= 400) {
        return { status: server.status, url, headers: { get: () => null }, text: async () => '{"error":"boom"}' };
      }
      return {
        status: server.status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(server.hits),
      };
    }
    throw new Error(`unexpected ${method} ${url}`);
  }) as never as (url: string) => Promise<TransportResponse>);
}

function setup(server: SearchServer, props: Partial<React.ComponentProps<typeof IntakeDuplicateDialog>> = {}) {
  stubTransport(server);
  const onCancel = vi.fn();
  const onPick = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const renderWith = (next: Partial<React.ComponentProps<typeof IntakeDuplicateDialog>>) => (
    <QueryClientProvider client={queryClient}>
      <IntakeDuplicateDialog
        open
        pending={false}
        workspaceSlug={WS}
        projectId={PROJECT}
        currentIssueId={CURRENT}
        onCancel={onCancel}
        onPick={onPick}
        {...props}
        {...next}
      />
    </QueryClientProvider>
  );
  const view = render(renderWith({}));
  return { onCancel, onPick, view, renderWith };
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("IntakeDuplicateDialog", () => {
  it("lists recent rows on open and excludes the current request", async () => {
    const server: SearchServer = { urls: [], status: 200, hits: [hit({ id: CURRENT, name: "Own request" }), hit()] };
    setup(server);
    await waitFor(() => expect(screen.getAllByTestId("intake-duplicate-option")).toHaveLength(1));
    expect(screen.getByTestId("intake-duplicate-option")).toHaveTextContent("APO-3");
    expect(screen.getByTestId("intake-duplicate-option")).toHaveTextContent("Original work");
    expect(screen.queryByText("Own request")).not.toBeInTheDocument();
    expect(server.urls).toHaveLength(1);
    expect(server.urls[0]).not.toMatch(/[?&]search=/);
    expect(server.urls[0]).toContain("workspace_search=false");
  });

  it("searches as the viewer types", async () => {
    const user = userEvent.setup();
    const server: SearchServer = { urls: [], status: 200, hits: [hit()] };
    setup(server);
    await waitFor(() => expect(screen.getAllByTestId("intake-duplicate-option")).toHaveLength(1));

    server.hits = [hit({ name: "Typed match" })];
    await user.type(screen.getByLabelText("Search work items"), "match");
    await waitFor(() => expect(screen.getByText("Typed match")).toBeInTheDocument(), { timeout: 5000 });
    expect(server.urls.at(-1)).toContain("search=match");
  });

  it("picks a row at once", async () => {
    const user = userEvent.setup();
    const server: SearchServer = { urls: [], status: 200, hits: [hit()] };
    const h = setup(server);
    await waitFor(() => expect(screen.getAllByTestId("intake-duplicate-option")).toHaveLength(1));
    await user.click(screen.getByTestId("intake-duplicate-option"));
    expect(h.onPick).toHaveBeenCalledWith(TARGET);
    expect(h.onCancel).not.toHaveBeenCalled();
  });

  it("treats cancel, the close control and Escape as cancel", async () => {
    const user = userEvent.setup();
    const server: SearchServer = { urls: [], status: 200, hits: [hit()] };
    const h = setup(server);
    await waitFor(() => expect(screen.getByRole("dialog", { name: DUPLICATE_DIALOG_TITLE })).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(h.onCancel).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(h.onCancel).toHaveBeenCalledTimes(2);
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(h.onCancel).toHaveBeenCalledTimes(3);
    expect(h.onPick).not.toHaveBeenCalled();
  });

  it("says so when nothing matches", async () => {
    const server: SearchServer = { urls: [], status: 200, hits: [] };
    setup(server);
    await waitFor(() => expect(screen.getByText("No work items found.")).toBeInTheDocument());
  });

  it("retries a failed search", async () => {
    const user = userEvent.setup();
    const server: SearchServer = { urls: [], status: 500, hits: [hit()] };
    setup(server);
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Couldn't search work items."));
    server.status = 200;
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.getAllByTestId("intake-duplicate-option")).toHaveLength(1));
  });

  it("clears the query when reopened", async () => {
    const user = userEvent.setup();
    const server: SearchServer = { urls: [], status: 200, hits: [hit()] };
    const { view, renderWith } = setup(server);
    await waitFor(() => expect(screen.getAllByTestId("intake-duplicate-option")).toHaveLength(1));
    await user.type(screen.getByLabelText("Search work items"), "typed");
    expect(screen.getByLabelText("Search work items")).toHaveValue("typed");
    view.rerender(renderWith({ open: false }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    view.rerender(renderWith({ open: true }));
    await waitFor(() => expect(screen.getByRole("dialog", { name: DUPLICATE_DIALOG_TITLE })).toBeInTheDocument());
    expect(screen.getByLabelText("Search work items")).toHaveValue("");
  });

  it("locks rows and cancel while a pick writes", async () => {
    const server: SearchServer = { urls: [], status: 200, hits: [hit()] };
    setup(server, { pending: true });
    await waitFor(() => expect(screen.getAllByTestId("intake-duplicate-option")).toHaveLength(1));
    expect(screen.getByTestId("intake-duplicate-option")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("stays closed and reads nothing when shut", async () => {
    const server: SearchServer = { urls: [], status: 200, hits: [hit()] };
    setup(server, { open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await new Promise((resolve) => globalThis.setTimeout(resolve, 50));
    expect(server.urls).toHaveLength(0);
  });
});
