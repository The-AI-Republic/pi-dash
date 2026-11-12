// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row star (parity/specs/pages.md §3.5 and §13.1; PAGE-009): the filled
// state follows the cached row, the label names the page, and the press
// runs the shared favourite write exactly once.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const favorites = vi.hoisted(() => ({ add: vi.fn(), remove: vi.fn() }));
const shell = vi.hoisted(() => ({
  notifyPending: vi.fn(),
  isFavoritesSectionOpen: vi.fn(() => true),
  setFavoritesSectionOpen: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  addPageFavorite: favorites.add,
  removePageFavorite: favorites.remove,
}));

vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyPending: shell.notifyPending,
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));
vi.mock("../../../shared/shell/favorites.js", () => ({
  isFavoritesSectionOpen: shell.isFavoritesSectionOpen,
  setFavoritesSectionOpen: shell.setFavoritesSectionOpen,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PAGES_PAGE_ID, PAGES_PROJECT_ID, makePageRow } from "../testing.js";
import { PageFavoriteStar } from "./PageFavoriteStar.js";

function renderStar(overrides: ReturnType<typeof makePageRow> = makePageRow()): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PageFavoriteStar workspaceSlug="acme" projectId={PAGES_PROJECT_ID} page={overrides} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  shell.isFavoritesSectionOpen.mockReturnValue(true);
  shell.notifyPending.mockReturnValue({ settle: vi.fn() });
  favorites.add.mockResolvedValue(undefined);
  favorites.remove.mockResolvedValue(undefined);
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

describe("PageFavoriteStar", () => {
  it("shows an unstarred page and adds it on press", async () => {
    renderStar();
    const star = screen.getByRole("button", { name: "Add Launch notes to favorites" });
    expect(star).toHaveAttribute("aria-pressed", "false");
    expect(star).not.toBeDisabled();

    fireEvent.click(star);
    await waitFor(() =>
      expect(favorites.add).toHaveBeenCalledWith(expect.anything(), "acme", PAGES_PROJECT_ID, PAGES_PAGE_ID)
    );
  });

  it("shows a starred page and drops it on press", async () => {
    renderStar(makePageRow({ is_favorite: true }));
    const star = screen.getByRole("button", { name: "Remove Launch notes from favorites" });
    expect(star).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(star);
    await waitFor(() =>
      expect(favorites.remove).toHaveBeenCalledWith(expect.anything(), "acme", PAGES_PROJECT_ID, PAGES_PAGE_ID)
    );
    expect(favorites.add).not.toHaveBeenCalled();
  });

  it("names an untitled page by its fallback label", () => {
    renderStar(makePageRow({ name: "  " }));
    expect(screen.getByRole("button", { name: "Add Untitled to favorites" })).toBeInTheDocument();
  });

  it("busies the star and refuses a second press while the write flies", async () => {
    favorites.add.mockImplementation(() => new Promise<void>(() => undefined));
    renderStar();
    const star = screen.getByRole("button", { name: "Add Launch notes to favorites" });
    fireEvent.click(star);
    await waitFor(() => expect(star).toHaveAttribute("aria-busy", "true"));
    expect(star).toBeDisabled();
    fireEvent.click(star);
    expect(favorites.add).toHaveBeenCalledTimes(1);
  });
});

afterEach(() => {
  resetAppClient();
});
