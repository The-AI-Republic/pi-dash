// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages palette jump (parity/specs/pages.md §13.6, PAGE-090): the picker
// reads the whole visible forest, lists a child under its title with the
// parent path as the hint, filters by title or path, closes the palette and
// navigates on pick, and reads a failed or empty project as an empty page.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { useCommandPalette, type PalettePage, type PalettePageState } from "../../../shared/commands/index.js";
import { PAGES_PROJECT_ID, jsonResponse, makePageRow } from "../testing.js";
import { listPages } from "@pidash/api-client";
import { projectPagesJumpPage, PAGE_JUMP_EMPTY_TEXT, PAGE_JUMP_PLACEHOLDER } from "./pageJumpPage.js";
import { buildPageJumpCommand, PAGE_JUMP_COMMAND_ID, PAGE_JUMP_PAGE_ID, PAGE_JUMP_TITLE } from "./pageJumpCommands.js";

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  listPages: vi.fn(),
}));

const listPagesMock = vi.mocked(listPages);

const PARENT = "323e4567-e89b-12d3-a456-426614174301";
const CHILD = "323e4567-e89b-12d3-a456-426614174302";

const parent = makePageRow({ id: PARENT, name: "Handbook", sub_pages_count: 1 });
const child = makePageRow({ id: CHILD, name: "Leave policy", parent: PARENT, parent_id: PARENT });

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

function renderPage(query = "", onPick = vi.fn()): ReturnType<typeof vi.fn> {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PageHarness
        page={projectPagesJumpPage({ workspaceSlug: "acme", projectId: PAGES_PROJECT_ID, onPick })}
        query={query}
      />
    </QueryClientProvider>
  );
  return onPick;
}

beforeEach(() => {
  captured = null;
  listPagesMock.mockReset();
  listPagesMock.mockImplementation(async (_client, _ws, _project, query) => {
    if (query?.parent === undefined) return [parent];
    if (query.parent === PARENT) return [child];
    return [];
  });
  createAppClient("https://api.test", (async (url: string) => jsonResponse(url, 200, {})) as never);
});

afterEach(() => {
  resetAppClient();
  useCommandPalette.getState().closePalette();
});

describe("projectPagesJumpPage", () => {
  it("carries the jump id and prompt", () => {
    const page = projectPagesJumpPage({ workspaceSlug: "acme", projectId: PAGES_PROJECT_ID, onPick: () => undefined });
    expect(page.id).toBe(PAGE_JUMP_PAGE_ID);
    expect(page.placeholder).toBe(PAGE_JUMP_PLACEHOLDER);
    expect(PAGE_JUMP_PLACEHOLDER).toBe(PAGE_JUMP_TITLE);
  });

  it("lists the whole visible forest and carries each child's parent path", async () => {
    renderPage();
    expect(screen.getByTestId("jump-status")).toHaveTextContent("loading");
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("ready"));
    expect(screen.getByRole("button", { name: "Handbook" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Leave policy" })).toBeInTheDocument();
    expect(captured?.options.find((option) => option.id === CHILD)?.hint).toBe("Handbook");
    expect(captured?.options.find((option) => option.id === PARENT)?.hint).toBeUndefined();
  });

  it("filters by title and by parent path", async () => {
    renderPage("handbook");
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("ready"));
    expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual(["Handbook", "Leave policy"]);
  });

  it("navigates and closes the palette on pick", async () => {
    useCommandPalette.getState().openPalette();
    const onPick = renderPage();
    await waitFor(() => expect(screen.getByRole("button", { name: "Leave policy" })).toBeInTheDocument());
    screen.getByRole("button", { name: "Leave policy" }).click();
    expect(useCommandPalette.getState().open).toBe(false);
    expect(onPick).toHaveBeenCalledWith(CHILD);
  });

  it("reads a failed forest as an empty page", async () => {
    listPagesMock.mockRejectedValue(new Error("no"));
    renderPage();
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("ready"));
    expect(screen.getByTestId("jump-empty")).toHaveTextContent(PAGE_JUMP_EMPTY_TEXT);
    expect(captured?.options).toEqual([]);
  });

  it("excludes archived pages from the picker", async () => {
    const shelved = makePageRow({ id: "323e4567-e89b-12d3-a456-426614174303", name: "Old", archived_at: "x" });
    listPagesMock.mockImplementation(async (_client, _ws, _project, query) => {
      if (query?.parent === undefined) return [parent, shelved];
      return [child];
    });
    renderPage();
    await waitFor(() => expect(screen.getByTestId("jump-status")).toHaveTextContent("ready"));
    expect(screen.queryByRole("button", { name: "Old" })).not.toBeInTheDocument();
  });
});

describe("buildPageJumpCommand", () => {
  it("names the picker page and hides while pages are off", () => {
    const command = buildPageJumpCommand({ projectName: "Website", pagesEnabled: true });
    expect(command).toMatchObject({ id: PAGE_JUMP_COMMAND_ID, title: PAGE_JUMP_TITLE, page: PAGE_JUMP_PAGE_ID });
    expect(command?.run()).toBeUndefined();
    expect(buildPageJumpCommand({ projectName: "Website", pagesEnabled: false })).toBeNull();
  });
});
