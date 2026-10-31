// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module jump sub-page (parity/specs/modules.md §10.2, MOD-037): one row per
// project module carrying its status mark and no selection check, filtering
// by the query, the plain empty line, a failed read read as empty, and a
// pick that closes the palette and navigates.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { useCommandPalette, type PalettePage, type PalettePageState } from "../../../shared/commands/index.js";
import { jsonResponse, makeModuleRow, MODULES_PROJECT_ID } from "../testing.js";
import { MODULE_JUMP_PAGE_ID, MODULE_JUMP_TITLE } from "./moduleCommands.js";
import { MODULE_JUMP_EMPTY_TEXT, MODULE_JUMP_PLACEHOLDER, projectModulesJumpPage } from "./moduleJumpPage.js";

const ALPHA_ID = "323e4567-e89b-12d3-a456-426614174011";
const BETA_ID = "323e4567-e89b-12d3-a456-426614174012";

const harness = vi.hoisted(() => ({ rows: [] as unknown, status: 200, delayMs: 0 }));
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

let captured: PalettePageState | null = null;
let page: PalettePage | null = null;

function PageHarness({ query }: { query: string }): React.ReactElement {
  if (page === null) throw new Error("page missing");
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
          aria-label={option.title}
          onClick={() => {
            void option.run();
          }}
        >
          <span data-testid={`icon-${option.id}`}>{option.icon}</span>
          <span data-testid={`title-${option.id}`}>{option.title}</span>
          <span data-testid={`hint-${option.id}`}>{option.hint}</span>
        </button>
      ))}
    </div>
  );
}

function renderPage(query = ""): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PageHarness query={query} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  captured = null;
  page = projectModulesJumpPage({ workspaceSlug: "acme", projectId: MODULES_PROJECT_ID, onPick: () => undefined });
  harness.rows = [];
  harness.status = 200;
  harness.delayMs = 0;
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/modules/`)) {
      if (harness.delayMs > 0) await sleep(harness.delayMs);
      return jsonResponse(url, harness.status, harness.rows);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  useCommandPalette.getState().closePalette();
});

describe("projectModulesJumpPage", () => {
  it("carries the jump id and the command's own title as its prompt", () => {
    expect(page?.id).toBe(MODULE_JUMP_PAGE_ID);
    expect(MODULE_JUMP_PLACEHOLDER).toBe(MODULE_JUMP_TITLE);
    expect(page?.placeholder).toBe(MODULE_JUMP_TITLE);
  });

  it("shows loading with the plain empty line while the list read flies", async () => {
    harness.delayMs = 50;
    harness.rows = [makeModuleRow({ name: "Delivery" })];
    renderPage();
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("loading"));
    expect(captured?.options).toEqual([]);
    expect(captured?.emptyText).toBe(MODULE_JUMP_EMPTY_TEXT);
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("ready"));
  });

  it("lists each module with its name, status mark and status hint", async () => {
    harness.rows = [
      makeModuleRow({ id: ALPHA_ID, name: "Delivery", status: "in-progress" }),
      makeModuleRow({ id: BETA_ID, name: "Launch", status: "planned" }),
    ];
    renderPage();
    await waitFor(() => expect(screen.getByTestId(`title-${ALPHA_ID}`)).toHaveTextContent("Delivery"));
    expect(screen.getByTestId(`title-${BETA_ID}`)).toHaveTextContent("Launch");
    expect(screen.getByTestId(`hint-${ALPHA_ID}`)).toHaveTextContent("In Progress");
    expect(screen.getByTestId(`hint-${BETA_ID}`)).toHaveTextContent("Planned");
    const dots = screen.getAllByTestId("module-status-dot");
    expect(dots).toHaveLength(2);
    expect(dots[0]).toHaveAttribute("data-module-status", "in-progress");
    expect(dots[0]).toHaveAttribute("aria-hidden", "true");
    // Navigation, not selection: no row carries a check.
    expect(captured?.options.every((option) => option.selected === undefined)).toBe(true);
  });

  it("keeps the server order and filters by the query", async () => {
    harness.rows = [
      makeModuleRow({ id: ALPHA_ID, name: "Alpha delivery" }),
      makeModuleRow({ id: BETA_ID, name: "Beta launch" }),
    ];
    renderPage();
    await waitFor(() => expect(screen.getAllByRole("button")).toHaveLength(2));
    const unfiltered = screen.getAllByRole("button");
    expect(unfiltered[0]).toHaveAttribute("aria-label", "Alpha delivery");
    expect(unfiltered[1]).toHaveAttribute("aria-label", "Beta launch");
  });

  it("filters down to the matching module", async () => {
    harness.rows = [
      makeModuleRow({ id: ALPHA_ID, name: "Alpha delivery" }),
      makeModuleRow({ id: BETA_ID, name: "Beta launch" }),
    ];
    renderPage("launch");
    await waitFor(() => expect(screen.getByTestId(`title-${BETA_ID}`)).toBeInTheDocument());
    expect(screen.queryByTestId(`title-${ALPHA_ID}`)).not.toBeInTheDocument();
  });

  it("shows the plain empty line for a project with no modules", async () => {
    renderPage();
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("ready"));
    expect(captured?.options).toEqual([]);
    expect(screen.getByTestId("jump-empty")).toHaveTextContent(MODULE_JUMP_EMPTY_TEXT);
  });

  it("reads a failed list read as an empty page rather than an error", async () => {
    harness.status = 500;
    harness.rows = { detail: "boom" };
    renderPage();
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("ready"));
    expect(captured?.options).toEqual([]);
    expect(screen.getByTestId("jump-empty")).toHaveTextContent(MODULE_JUMP_EMPTY_TEXT);
  });

  it("closes the palette and hands the picked module to the owner", async () => {
    harness.rows = [makeModuleRow({ id: ALPHA_ID, name: "Delivery" })];
    const onPick = vi.fn();
    page = projectModulesJumpPage({ workspaceSlug: "acme", projectId: MODULES_PROJECT_ID, onPick });
    renderPage();
    await waitFor(() => expect(screen.getByRole("button", { name: "Delivery" })).toBeInTheDocument());
    useCommandPalette.getState().openPalette();
    fireEvent.click(screen.getByRole("button", { name: "Delivery" }));
    expect(onPick).toHaveBeenCalledWith(ALPHA_ID);
    expect(useCommandPalette.getState().open).toBe(false);
  });
});
