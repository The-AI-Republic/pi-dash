// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

describe("SavedViewEmptyCard", () => {
  it("refuses to render before the preload awaited its chunk", async () => {
    vi.resetModules();
    const { SavedViewEmptyCard } = await import("./viewEmpty.js");
    expect(() => render(<SavedViewEmptyCard canAct onCreate={() => undefined} />)).toThrow(
      /await loadSavedViewEmptyCard/
    );
  });

  it("renders the saved-view card once its chunk has loaded", async () => {
    const { loadSavedViewEmptyCard, SavedViewEmptyCard } = await import("./viewEmpty.js");
    await loadSavedViewEmptyCard();
    const onCreate = vi.fn();
    render(<SavedViewEmptyCard canAct onCreate={onCreate} />);
    expect(screen.getByText("View work items will appear here")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "New work item" }));
    expect(onCreate).toHaveBeenCalledTimes(1);
  });
});
