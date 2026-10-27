// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { POPOVER_LAYER_CLASS } from "../lib/styles";
import { ContextMenu } from "./ContextMenu";
import { positionerOf } from "./overlays.testSupport";

function setup(onSelect: () => void = vi.fn()) {
  return render(
    <ContextMenu
      trigger={<span>Tab</span>}
      items={[
        { label: "Set as default", onSelect },
        { label: "Hide in more menu", onSelect },
      ]}
    />
  );
}

describe("ContextMenu", () => {
  it("opens on right-click and selects with the mouse", () => {
    // fireEvent dispatches the real pointer path (see the Menu suite for
    // why mouse behavior avoids user-event's back-to-back sequence).
    const onSelect = vi.fn();
    setup(onSelect);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    fireEvent.contextMenu(screen.getByText("Tab"));
    expect(screen.getByRole("menu")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Set as default" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("lands the context menu on the popover tier above in-page chrome", () => {
    setup();
    fireEvent.contextMenu(screen.getByText("Tab"));
    expect(positionerOf(screen.getByRole("menu")).className).toContain(POPOVER_LAYER_CLASS);
  });

  it("closes on Escape", async () => {
    const user = userEvent.setup();
    setup();
    fireEvent.contextMenu(screen.getByText("Tab"));
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("renders separators, icons and disabled rows keyed apart", () => {
    const onSelect = vi.fn();
    render(
      <ContextMenu
        trigger={<span>Tab</span>}
        items={[
          { label: "Web", itemKey: "p1", icon: <i data-testid="mark-p1" />, onSelect },
          { separator: true },
          { label: "Web", itemKey: "p2", icon: <i data-testid="mark-p2" />, onSelect, disabled: true },
        ]}
      />
    );
    fireEvent.contextMenu(screen.getByText("Tab"));
    expect(screen.getByRole("separator")).toBeInTheDocument();
    expect(screen.getByTestId("mark-p1")).toBeInTheDocument();
    expect(screen.getByTestId("mark-p2")).toBeInTheDocument();
    const rows = screen.getAllByRole("menuitem", { name: "Web" });
    expect(rows).toHaveLength(2);
    fireEvent.click(rows[0]!);
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("renders custom entries as direct popup children", () => {
    render(
      <ContextMenu
        trigger={<span>Tab</span>}
        items={[
          { custom: true, entryKey: "note", node: <span data-testid="note">Pinned tab</span> },
          { label: "Close", onSelect: () => {} },
        ]}
      />
    );
    fireEvent.contextMenu(screen.getByText("Tab"));
    const menu = screen.getByRole("menu");
    expect(menu).toContainElement(screen.getByTestId("note"));
    expect(screen.getByTestId("note").parentElement).toBe(menu);
  });

  it("carries the action description inside the right-click item too", () => {
    render(
      <ContextMenu
        trigger={<span>Tab</span>}
        items={[{ label: "Archive", description: "Not finished yet", onSelect: () => {}, disabled: true }]}
      />
    );
    fireEvent.contextMenu(screen.getByText("Tab"));
    const item = screen.getByRole("menuitem", { name: /Archive/ });
    expect(item.querySelector("[data-menu-description='true']")?.textContent).toBe("Not finished yet");
  });
});
