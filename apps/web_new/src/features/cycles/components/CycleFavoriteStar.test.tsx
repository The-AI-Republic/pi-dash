// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle favorite star (parity/specs/cycles.md §11, CYC-029): the mark
// reads from the toggle state, the label names the direction, and the
// star pins its progress state while the write flies.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const favorite = vi.hoisted(() => ({ starred: false, pending: false, toggle: vi.fn() }));
vi.mock("../api/cycleFavorite.js", () => ({
  useCycleFavorite: () => favorite,
}));

import { CYCLES_PROJECT_ID, makeCycleRow } from "../testing.js";
import { CycleFavoriteStar } from "./CycleFavoriteStar.js";

function renderStar(starred: boolean, pending: boolean): void {
  favorite.starred = starred;
  favorite.pending = pending;
  favorite.toggle.mockClear();
  render(
    <QueryClientProvider client={new QueryClient()}>
      <CycleFavoriteStar
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        cycle={makeCycleRow({ is_favorite: starred })}
      />
    </QueryClientProvider>
  );
}

describe("CycleFavoriteStar", () => {
  it("offers the add direction on an unmarked row", () => {
    renderStar(false, false);
    const star = screen.getByTestId("cycle-favorite-marker");
    expect(star).toHaveAttribute("data-favorite", "false");
    expect(star).toHaveAccessibleName(`Add Cycle 7 to favorites`);
    expect(star).toHaveAttribute("aria-pressed", "false");
    expect(star).not.toBeDisabled();
    expect(star.querySelector("svg")?.getAttribute("class") ?? "").not.toContain("fill-");
    fireEvent.click(star);
    expect(favorite.toggle).toHaveBeenCalledTimes(1);
  });

  it("fills the mark and offers the remove direction on a starred row", () => {
    renderStar(true, false);
    const star = screen.getByTestId("cycle-favorite-marker");
    expect(star).toHaveAttribute("data-favorite", "true");
    expect(star).toHaveAccessibleName(`Remove Cycle 7 from favorites`);
    expect(star).toHaveAttribute("aria-pressed", "true");
    expect(star.querySelector("svg")?.getAttribute("class") ?? "").toContain("fill-");
  });

  it("pins the progress state while the write flies", () => {
    renderStar(true, true);
    const star = screen.getByTestId("cycle-favorite-marker");
    expect(star).toHaveAttribute("aria-busy", "true");
    expect(star).toBeDisabled();
  });
});
