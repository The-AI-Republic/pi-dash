// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row and header triggers (parity/specs/issues.md §7, ISS-060/066): the
// hover ellipsis, the right-click menu, the archive note rendered inside
// its item, and the read-only row that grows no trigger at all.

import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ARCHIVE_GATING_NOTE } from "./model.js";
import { menuItemsFor, RowContextMenu, RowMenuButton } from "./RowQuickActions.js";
import { DetailQuickActions } from "./DetailQuickActions.js";
import type { QuickActionsState } from "./useQuickActions.js";

function state(overrides: Partial<QuickActionsState> = {}): QuickActionsState {
  return {
    entries: [
      { key: "edit", label: "Edit", disabled: false },
      { key: "archive", label: "Archive", disabled: true, description: ARCHIVE_GATING_NOTE },
    ],
    handlers: { run: vi.fn() },
    overlays: <p data-testid="overlays" />,
    ...overrides,
  };
}

describe("menuItemsFor", () => {
  it("maps entries to kit items with icons, notes and the run handler", () => {
    const run = vi.fn();
    const items = menuItemsFor(state({ handlers: { run } }));
    const archive = items[1];
    expect(archive && "label" in archive ? archive.label : null).toBe("Archive");
    expect(archive && "description" in archive ? archive.description : null).toBe(ARCHIVE_GATING_NOTE);
    expect(archive && "disabled" in archive ? archive.disabled : null).toBe(true);
    fireEvent.keyDown(document.body);
    if (archive && "onSelect" in archive) archive.onSelect();
    expect(run).toHaveBeenCalledWith("archive");
  });
});

describe("RowMenuButton", () => {
  it("shows the hover trigger, opens the menu and mounts the overlays", async () => {
    const user = userEvent.setup();
    render(<RowMenuButton state={state()} issueKey="WEB-42" className="hidden" />);
    expect(screen.getByTestId("overlays")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Actions for WEB-42" }));
    expect(await screen.findByRole("menuitem", { name: /Edit/ })).toBeInTheDocument();
    expect(screen.getByText(ARCHIVE_GATING_NOTE)).toBeInTheDocument();
  });

  it("renders nothing without entries", () => {
    const { container } = render(<RowMenuButton state={state({ entries: [] })} issueKey="WEB-42" />);
    expect(container).toBeEmptyDOMElement();
  });
});

describe("RowContextMenu", () => {
  it("opens the same entries on right-click", async () => {
    render(<RowContextMenu state={state()} trigger={<span>Row body</span>} />);
    fireEvent.contextMenu(screen.getByText("Row body"));
    expect(await screen.findByRole("menuitem", { name: /Edit/ })).toBeInTheDocument();
  });

  it("passes the trigger through untouched without entries", () => {
    render(<RowContextMenu state={state({ entries: [] })} trigger={<span>Row body</span>} />);
    expect(screen.getByText("Row body")).toBeInTheDocument();
  });
});

describe("DetailQuickActions", () => {
  it("carries its own name and the main-menu marker", async () => {
    const user = userEvent.setup();
    const { container } = render(<DetailQuickActions state={state()} />);
    expect(container.querySelector("[data-main-menu='true']")).not.toBeNull();
    await user.click(screen.getByRole("button", { name: "Work item actions" }));
    expect(await screen.findByRole("menuitem", { name: /Archive/ })).toBeInTheDocument();
  });
});
