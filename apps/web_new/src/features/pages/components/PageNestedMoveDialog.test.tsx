// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Within-project move picker (parity/specs/pages.md §12.2, PAGE-087): the
// picker starts on the page's current parent, offers only live pages
// outside the page's own subtree plus the top level, searches titles and
// parent paths, refuses a target that would cycle, and hands the chosen
// container to the host.

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { makePageRow } from "../testing.js";
import { PageNestedMoveDialog, PAGE_MOVE_EMPTY, PAGE_MOVE_SEARCH_PLACEHOLDER } from "./PageNestedMoveDialog.js";
import { PAGE_MOVE_DIALOG_TITLE, PAGE_MOVE_TOP_LEVEL } from "../lib/pageNested.js";

const TOP = "323e4567-e89b-12d3-a456-426614174201";
const MIDDLE = "323e4567-e89b-12d3-a456-426614174202";
const LEAF = "323e4567-e89b-12d3-a456-426614174203";
const OTHER = "323e4567-e89b-12d3-a456-426614174204";
const SHELVED = "323e4567-e89b-12d3-a456-426614174205";

const top = makePageRow({ id: TOP, name: "Handbook", sub_pages_count: 1 });
const middle = makePageRow({ id: MIDDLE, name: "Policies", parent: TOP, parent_id: TOP, sub_pages_count: 1 });
const leaf = makePageRow({ id: LEAF, name: "Leave", parent: MIDDLE, parent_id: MIDDLE });
const other = makePageRow({ id: OTHER, name: "Other" });
const shelved = makePageRow({ id: SHELVED, name: "Shelved", archived_at: "2026-10-20T00:00:00Z" });
const pool = [top, middle, leaf, other, shelved];

function renderDialog(overrides: Partial<React.ComponentProps<typeof PageNestedMoveDialog>> = {}) {
  const onOpenChange = vi.fn();
  const onConfirm = vi.fn();
  const view = render(
    <PageNestedMoveDialog
      open
      onOpenChange={onOpenChange}
      page={leaf}
      pool={pool}
      pending={false}
      onConfirm={onConfirm}
      {...overrides}
    />
  );
  const rerenderOpen = (open: boolean): void => {
    view.rerender(
      <PageNestedMoveDialog
        open={open}
        onOpenChange={onOpenChange}
        page={leaf}
        pool={pool}
        pending={false}
        onConfirm={onConfirm}
        {...overrides}
      />
    );
  };
  return { onOpenChange, onConfirm, rerenderOpen };
}

describe("PageNestedMoveDialog", () => {
  it("heads the picker and offers the top level plus every allowed page", () => {
    renderDialog();
    expect(screen.getByRole("dialog")).toHaveTextContent(PAGE_MOVE_DIALOG_TITLE);
    const options = screen.getAllByRole("option").map((option) => option.textContent ?? "");
    expect(options[0]).toContain(PAGE_MOVE_TOP_LEVEL);
    expect(options.some((text) => text.includes("Handbook"))).toBe(true);
    expect(options.some((text) => text.includes("Other"))).toBe(true);
    expect(options.some((text) => text.includes("Shelved"))).toBe(false);
    expect(options.some((text) => text.includes("Leave"))).toBe(false);
  });

  it("starts on the page's current parent", () => {
    renderDialog();
    expect(screen.getByRole("option", { name: /Policies/ })).toHaveAttribute("aria-selected", "true");
  });

  it("shows each candidate's parent path", () => {
    renderDialog();
    expect(screen.getByRole("option", { name: /Policies.*Handbook/ })).toBeInTheDocument();
  });

  it("holds the list not-ready while the full pool loads, then offers targets", () => {
    renderDialog({ loading: true });
    expect(screen.getByTestId("page-move-listbox")).toHaveAttribute("data-ready", "false");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    renderDialog();
    expect(screen.getAllByTestId("page-move-listbox").at(-1)).toHaveAttribute("data-ready", "true");
  });

  it("filters by title and by parent path", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.type(screen.getByRole("searchbox", { name: PAGE_MOVE_SEARCH_PLACEHOLDER }), "other");
    expect(screen.getByRole("option", { name: /Other/ })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /Handbook/ })).not.toBeInTheDocument();
    await user.clear(screen.getByRole("searchbox"));
    await user.type(screen.getByRole("searchbox"), "handbook");
    expect(screen.getByRole("option", { name: /Policies/ })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /Other/ })).not.toBeInTheDocument();
  });

  it("says when nothing matches", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.type(screen.getByRole("searchbox"), "zzz");
    expect(screen.getByRole("status")).toHaveTextContent(PAGE_MOVE_EMPTY);
  });

  it("never offers the page itself or its descendants", () => {
    renderDialog({ page: top });
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).queryByRole("option", { name: /Handbook/ })).not.toBeInTheDocument();
    expect(within(dialog).queryByRole("option", { name: /Policies/ })).not.toBeInTheDocument();
    expect(within(dialog).queryByRole("option", { name: /Leave/ })).not.toBeInTheDocument();
    expect(within(dialog).getByRole("option", { name: /Other/ })).toBeInTheDocument();
  });

  it("confirms the top level and hands back null", async () => {
    const user = userEvent.setup();
    const { onConfirm } = renderDialog();
    await user.click(screen.getByRole("option", { name: new RegExp(PAGE_MOVE_TOP_LEVEL) }));
    await user.click(screen.getByRole("button", { name: "Move page" }));
    expect(onConfirm).toHaveBeenCalledWith(null);
  });

  it("exposes a refusal when the current target is not in the pool", async () => {
    const user = userEvent.setup();
    const { onConfirm } = renderDialog({ page: { ...leaf, parent: "gone", parent_id: "gone" }, pool: [top, other] });
    expect(screen.getByTestId("page-move-refusal")).toHaveTextContent(/no longer available/i);
    expect(screen.getByRole("button", { name: "Move page" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Move page" }));
    expect(onConfirm).not.toHaveBeenCalled();
    await user.click(screen.getByRole("option", { name: /Other/ }));
    expect(screen.queryByTestId("page-move-refusal")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Move page" }));
    expect(onConfirm).toHaveBeenCalledWith(OTHER);
  });

  it("reports an explicit close request through the host callback", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("pins both controls while a move is in flight and refuses to dismiss", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog({ pending: true });
    expect(screen.getByRole("button", { name: "Moving…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).not.toHaveBeenCalled();
    await user.keyboard("{Escape}");
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("closes on cancel and starts from the current parent again when reopened", async () => {
    const user = userEvent.setup();
    const { onOpenChange, rerenderOpen } = renderDialog();
    await user.click(screen.getByRole("option", { name: /Other/ }));
    expect(screen.getByRole("option", { name: /Other/ })).toHaveAttribute("aria-selected", "true");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    rerenderOpen(false);
    rerenderOpen(true);
    expect(screen.getByRole("option", { name: /Policies/ })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("searchbox")).toHaveValue("");
  });
});
