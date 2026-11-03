// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module favorite star (parity/specs/modules.md §4.4, MOD-016): the
// pressed state follows the favorite mark, the label names the
// direction, and a press toggles through the favorite write. The caller
// hides the star for guests and archived rows; this button only ever
// renders where a toggle is legal.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  favoriteModule: vi.fn(),
  unfavoriteModule: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  notifySuccess: mocks.notifySuccess,
  notifyError: mocks.notifyError,
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  favoriteModule: mocks.favoriteModule,
  unfavoriteModule: mocks.unfavoriteModule,
}));

vi.mock("../../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { makeModuleRow, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../../testing.js";
import { ModuleStarButton } from "./ModuleStarButton.js";

function renderStar(isFavorite: boolean): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ModuleStarButton
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={makeModuleRow({ id: MODULES_MODULE_ID, is_favorite: isFavorite })}
        testId="modules-row-star"
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  mocks.favoriteModule.mockResolvedValue(undefined);
  mocks.unfavoriteModule.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("ModuleStarButton", () => {
  it("offers to add an unstarred module and toggles on press", async () => {
    renderStar(false);
    const star = screen.getByTestId("modules-row-star");
    expect(star).toHaveAttribute("aria-pressed", "false");
    expect(star).toHaveAccessibleName("Add Delivery to favorites");
    expect(star).toBeEnabled();

    fireEvent.click(star);
    await waitFor(() => {
      expect(mocks.favoriteModule).toHaveBeenCalledTimes(1);
    });
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Success!", "Module added to favorites.");
  });

  it("offers to remove a starred module and toggles on press", async () => {
    renderStar(true);
    const star = screen.getByTestId("modules-row-star");
    expect(star).toHaveAttribute("aria-pressed", "true");
    expect(star).toHaveAccessibleName("Remove Delivery from favorites");

    fireEvent.click(star);
    await waitFor(() => {
      expect(mocks.unfavoriteModule).toHaveBeenCalledTimes(1);
    });
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Success!", "Module removed from favorites.");
  });
});
