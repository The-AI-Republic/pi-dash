// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-views jump sub-page (parity/specs/views.md §12, VIEW-042):
// loading, order, filtering, empty and failed-read states, and pick
// navigation.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { useCommandPalette, type PalettePage, type PalettePageState } from "../../../shared/commands/index.js";
import { jsonResponse, makeView } from "../testing.js";
import { projectViewsJumpPage, VIEW_JUMP_EMPTY_TEXT, VIEW_JUMP_PAGE_PLACEHOLDER } from "./viewJumpPage.js";
import { VIEW_JUMP_PAGE_ID, VIEW_JUMP_TITLE } from "./viewCommands.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

const harness = vi.hoisted(() => ({ views: [] as ReturnType<typeof makeView>[], status: 200, delayMs: 0 }));

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

let captured: PalettePageState | null = null;

function PageHarness({ page, query }: { page: PalettePage; query: string }): React.ReactElement {
  const state = page.usePageState(query);
  captured = state;
  return (
    <div>
      <span data-testid="jump-status">{state.status}</span>
      <span data-testid="jump-empty">{state.emptyText}</span>
      {state.options.map((option) => (
        <button
          key={option.id}
          type="button"
          onClick={() => {
            void option.run();
          }}
        >
          {option.title}
        </button>
      ))}
    </div>
  );
}

function renderPage(page: PalettePage, query = ""): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PageHarness page={page} query={query} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  captured = null;
  harness.views = [];
  harness.status = 200;
  harness.delayMs = 0;
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/views/`)) {
      if (harness.delayMs > 0) await sleep(harness.delayMs);
      return jsonResponse(url, harness.status, harness.views);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  useCommandPalette.getState().closePalette();
});

describe("projectViewsJumpPage", () => {
  it("carries the jump id and prompt", () => {
    const page = projectViewsJumpPage({ workspaceSlug: "acme", projectId: PROJECT_ID, onPick: () => undefined });
    expect(page.id).toBe(VIEW_JUMP_PAGE_ID);
    expect(page.placeholder).toBe(VIEW_JUMP_PAGE_PLACEHOLDER);
    expect(VIEW_JUMP_PAGE_PLACEHOLDER).toBe(VIEW_JUMP_TITLE);
  });

  it("spins while the list loads", async () => {
    harness.delayMs = 50;
    harness.views = [makeView({ name: "Triage" })];
    renderPage(projectViewsJumpPage({ workspaceSlug: "acme", projectId: PROJECT_ID, onPick: () => undefined }));
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("loading"));
    expect(captured?.options).toEqual([]);
    expect(captured?.emptyText).toBe(VIEW_JUMP_EMPTY_TEXT);
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("ready"));
  });

  it("lists views newest-first and filters by the query", async () => {
    harness.views = [
      makeView({ id: "223e4567-e89b-12d3-a456-426614174011", name: "Alpha board", updated_at: "2026-01-01T00:00:00Z" }),
      makeView({ id: "223e4567-e89b-12d3-a456-426614174012", name: "Beta triage", updated_at: "2026-03-01T00:00:00Z" }),
    ];
    renderPage(projectViewsJumpPage({ workspaceSlug: "acme", projectId: PROJECT_ID, onPick: () => undefined }), "beta");
    await waitFor(() => expect(screen.getByRole("button", { name: "Beta triage" })).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Alpha board" })).not.toBeInTheDocument();
    expect(captured?.status).toBe("ready");
  });

  it("keeps the list order unfiltered on a blank query", async () => {
    harness.views = [
      makeView({ id: "223e4567-e89b-12d3-a456-426614174011", name: "Alpha board", updated_at: "2026-01-01T00:00:00Z" }),
      makeView({ id: "223e4567-e89b-12d3-a456-426614174012", name: "Beta triage", updated_at: "2026-03-01T00:00:00Z" }),
    ];
    renderPage(projectViewsJumpPage({ workspaceSlug: "acme", projectId: PROJECT_ID, onPick: () => undefined }), "   ");
    await waitFor(() => expect(screen.getAllByRole("button")).toHaveLength(2));
    const buttons = screen.getAllByRole("button");
    expect(buttons[0]).toHaveTextContent("Beta triage");
    expect(buttons[1]).toHaveTextContent("Alpha board");
  });

  it("shows the plain empty line when the project holds no views", async () => {
    renderPage(projectViewsJumpPage({ workspaceSlug: "acme", projectId: PROJECT_ID, onPick: () => undefined }));
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("ready"));
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByTestId("jump-empty")).toHaveTextContent(VIEW_JUMP_EMPTY_TEXT);
  });

  it("renders a failed read as empty with no error UI", async () => {
    harness.status = 500;
    harness.views = { detail: "boom" } as unknown as ReturnType<typeof makeView>[];
    renderPage(projectViewsJumpPage({ workspaceSlug: "acme", projectId: PROJECT_ID, onPick: () => undefined }));
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("ready"));
    expect(captured?.options).toEqual([]);
    expect(screen.getByTestId("jump-empty")).toHaveTextContent(VIEW_JUMP_EMPTY_TEXT);
  });

  it("closes the palette and picks the view on activation", async () => {
    const onPick = vi.fn();
    harness.views = [makeView({ id: "223e4567-e89b-12d3-a456-426614174013", name: "Triage" })];
    useCommandPalette.getState().openPalette();
    renderPage(projectViewsJumpPage({ workspaceSlug: "acme", projectId: PROJECT_ID, onPick }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Triage" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Triage" }));
    expect(onPick).toHaveBeenCalledWith("223e4567-e89b-12d3-a456-426614174013");
    expect(useCommandPalette.getState().open).toBe(false);
  });
});
