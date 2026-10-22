// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One draft block (parity/specs/issues.md §12, ISS-137/138/139/140): the
// project code's click-to-copy, double-click edit, and the ellipsis menu.

import { fireEvent, render, screen, within } from "@testing-library/react";
import type { DraftIssue, ProjectDetails } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const toasts = vi.hoisted(() => ({ notifySuccess: vi.fn(), notifyError: vi.fn() }));
vi.mock("../../../shared/shell/index.js", () => toasts);

vi.mock("./DraftProperties.js", () => ({
  DraftProperties: () => <div data-testid="draft-properties" />,
}));

import { copyIdentifier, DraftBlock } from "./DraftBlock.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function draft(overrides: Partial<DraftIssue> = {}): DraftIssue {
  return {
    id: "draft-1",
    name: "Half written",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: "user-1",
    updated_by: null,
    type_id: null,
    description_html: "",
    ...overrides,
  } as DraftIssue;
}

const project = { id: PROJECT, name: "Atlas", identifier: "PAR" } as unknown as ProjectDetails;

function setup(row = draft(), overrides: { identifier?: string | null; editable?: boolean } = {}) {
  const handlers = {
    onEdit: vi.fn(),
    onCopy: vi.fn(),
    onMove: vi.fn(),
    onDelete: vi.fn(),
    onWrite: vi.fn(),
  };
  const result = render(
    <DraftBlock
      workspaceSlug={SLUG}
      draft={row}
      project={project}
      projectIdentifier={overrides.identifier === undefined ? "PAR" : overrides.identifier}
      editable={overrides.editable ?? true}
      {...handlers}
    />
  );
  return { ...result, ...handlers };
}

beforeEach(() => {
  toasts.notifySuccess.mockReset();
  Object.defineProperty(globalThis.navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn(async () => undefined) },
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("DraftBlock", () => {
  it("shows the project code, name and property strip", () => {
    setup();
    expect(screen.getByRole("button", { name: "Copy PAR" })).toHaveTextContent("PAR");
    expect(screen.getByText("Half written", { exact: true }).tagName).toBe("P");
    expect(screen.getByTestId("draft-properties")).toBeInTheDocument();
  });

  it("describes itself for reading surfaces and names its row", () => {
    const { container } = setup();
    const block = container.querySelector("[data-draft-block]");
    expect(block).toHaveAttribute("aria-label", "Draft Half written");
    expect(block).toHaveAttribute("data-draft-id", "draft-1");
    expect(container.querySelector("p[data-draft-name]")).toHaveTextContent("Half written");
  });

  it("copies the code on click, confirms, and does not open the edit", async () => {
    const h = setup();
    fireEvent.click(screen.getByRole("button", { name: "Copy PAR" }));
    expect(globalThis.navigator.clipboard.writeText).toHaveBeenCalledWith("PAR");
    await vi.waitFor(() => expect(toasts.notifySuccess).toHaveBeenCalledWith("Copied PAR"));
    expect(h.onEdit).not.toHaveBeenCalled();
  });

  it("does not confirm when the clipboard refuses", async () => {
    (globalThis.navigator.clipboard.writeText as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("no"));
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Copy PAR" }));
    await Promise.resolve();
    await Promise.resolve();
    expect(toasts.notifySuccess).not.toHaveBeenCalled();
  });

  it("offers no copy button for a draft without a project", () => {
    setup(draft({ project_id: null }), { identifier: null });
    expect(screen.queryByRole("button", { name: /^Copy / })).not.toBeInTheDocument();
  });

  it("names an untitled draft with an empty title and skips its copy", async () => {
    const h = setup(draft({ name: null, project_id: null }), { identifier: null });
    expect(screen.getByRole("article", { name: "Draft" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Actions for" })).toBeInTheDocument();
    // A null identifier never writes: the guard returns before touching
    // the clipboard (the copy button is not rendered for such a row).
    expect(globalThis.navigator.clipboard.writeText).not.toHaveBeenCalled();
    fireEvent.doubleClick(screen.getByRole("article", { name: "Draft" }));
    expect(h.onEdit).toHaveBeenCalledTimes(1);
  });

  it("edits on double-click", () => {
    const h = setup();
    fireEvent.doubleClick(screen.getByRole("article", { name: "Draft Half written" }));
    expect(h.onEdit).toHaveBeenCalledTimes(1);
  });

  it("opens the menu on the ellipsis and on right-click, and closes on leave", () => {
    const h = setup();
    const toggle = screen.getByRole("button", { name: "Actions for Half written" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(toggle);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    fireEvent.contextMenu(screen.getByRole("article", { name: "Draft Half written" }));
    const menu = screen.getByRole("menu", { name: "Draft actions" });
    fireEvent.mouseLeave(menu);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(h.onEdit).not.toHaveBeenCalled();
  });

  it("runs each menu entry and closes the menu", () => {
    for (const [label, key] of [
      ["Edit", "onEdit"],
      ["Make a copy", "onCopy"],
      ["Move to project", "onMove"],
      ["Delete", "onDelete"],
    ] as const) {
      const h = setup();
      fireEvent.click(screen.getByRole("button", { name: "Actions for Half written" }));
      fireEvent.click(within(screen.getByRole("menu")).getByRole("menuitem", { name: label }));
      expect(h[key]).toHaveBeenCalledTimes(1);
      expect(screen.queryByRole("menu")).not.toBeInTheDocument();
      h.unmount();
    }
  });

  it("keeps a menu click from reaching the block's edit", () => {
    const h = setup();
    fireEvent.click(screen.getByRole("button", { name: "Actions for Half written" }));
    expect(h.onEdit).not.toHaveBeenCalled();
  });

  it("exposes identical translated entries from the ellipsis and right-click", () => {
    setup();
    const block = screen.getByRole("article", { name: "Draft Half written" });
    const entries = (): string[] =>
      within(screen.getByRole("menu", { name: "Draft actions" }))
        .getAllByRole("menuitem")
        .map((item) => item.textContent ?? "");
    fireEvent.click(screen.getByRole("button", { name: "Actions for Half written" }));
    expect(entries()).toEqual(["Edit", "Make a copy", "Move to project", "Delete"]);
    fireEvent.mouseLeave(screen.getByRole("menu", { name: "Draft actions" }));
    fireEvent.contextMenu(block);
    expect(entries()).toEqual(["Edit", "Make a copy", "Move to project", "Delete"]);
  });

  it("closes the menu on Escape and ignores other keys", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Actions for Half written" }));
    const menu = screen.getByRole("menu", { name: "Draft actions" });
    fireEvent.keyDown(menu, { key: "Enter" });
    expect(screen.queryByRole("menu", { name: "Draft actions" })).toBeInTheDocument();
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "Draft actions" })).not.toBeInTheDocument();
  });
});

describe("copyIdentifier", () => {
  it("reports success and failure", async () => {
    expect(await copyIdentifier("PAR", { writeText: async () => undefined })).toBe(true);
    expect(
      await copyIdentifier("PAR", {
        writeText: async () => {
          throw new Error("denied");
        },
      })
    ).toBe(false);
  });

  it("reports failure without a clipboard", async () => {
    expect(await copyIdentifier("PAR", undefined)).toBe(false);
  });
});
