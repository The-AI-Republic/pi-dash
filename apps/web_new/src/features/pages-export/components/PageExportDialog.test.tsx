// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export dialog (parity/specs/exports.md §5, EXP-016/017/021):
// defaults, conditional size row, feedback, reset and keyboard behavior.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import type { PageExportHost } from "../lib/pageExportDialogRegistry.js";
import type { SavedPageExport } from "../lib/savePageExport.js";
import { PageExportDialog } from "./PageExportDialog.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
  notifyInfo: vi.fn(),
}));

const HOST: PageExportHost = {
  pageTitle: "Export me",
  projectId: "project-1",
  workspaceSlug: "acme",
  getHtml: () => "<p>body</p>",
  getMarkdown: () => "body",
};

function formatSelect(): HTMLSelectElement {
  return screen.getByLabelText("Format") as HTMLSelectElement;
}

function scopeSelect(): HTMLSelectElement {
  return screen.getByLabelText("Content scope") as HTMLSelectElement;
}

function sizeSelect(): HTMLSelectElement | null {
  return screen.queryByLabelText("Page size") as HTMLSelectElement | null;
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("PageExportDialog", () => {
  it("opens at PDF / Everything / A4 without closing itself", () => {
    const onClose = vi.fn();
    render(
      <PageExportDialog host={HOST} open onClose={onClose} saveFile={vi.fn(async () => ({ filename: "x.pdf" }))} />
    );
    expect(screen.getByRole("dialog", { name: "Export page" })).toBeInTheDocument();
    expect(formatSelect().value).toBe("PDF");
    expect(scopeSelect().value).toBe("Everything");
    expect(sizeSelect()?.value).toBe("A4");
    expect([...(sizeSelect()?.options ?? [])].map((option) => option.value)).toEqual([
      "A4",
      "A3",
      "A2",
      "Letter",
      "Legal",
      "Tabloid",
    ]);
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Export" })).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("resets to defaults on every open", () => {
    const onClose = vi.fn();
    const saveFile = vi.fn(async () => ({ filename: "x.pdf" }));
    const view = render(<PageExportDialog host={HOST} open={false} onClose={onClose} saveFile={saveFile} />);
    view.rerender(<PageExportDialog host={HOST} open onClose={onClose} saveFile={saveFile} />);
    fireEvent.change(formatSelect(), { target: { value: "Markdown" } });
    fireEvent.change(scopeSelect(), { target: { value: "No images" } });
    view.rerender(<PageExportDialog host={HOST} open={false} onClose={onClose} saveFile={saveFile} />);
    view.rerender(<PageExportDialog host={HOST} open onClose={onClose} saveFile={saveFile} />);
    expect(formatSelect().value).toBe("PDF");
    expect(scopeSelect().value).toBe("Everything");
    expect(sizeSelect()?.value).toBe("A4");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("hides the size row for plain text and restores the choice", () => {
    const saveFile = vi.fn(async () => ({ filename: "x.pdf" }));
    render(<PageExportDialog host={HOST} open onClose={() => undefined} saveFile={saveFile} />);
    fireEvent.change(sizeSelect() as HTMLSelectElement, { target: { value: "Letter" } });
    fireEvent.change(formatSelect(), { target: { value: "Markdown" } });
    expect(sizeSelect()).toBeNull();
    fireEvent.change(formatSelect(), { target: { value: "PDF" } });
    expect(sizeSelect()?.value).toBe("Letter");
  });

  it("exports with a working state, then toasts and closes", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    let resolveSave!: (saved: SavedPageExport) => void;
    const pending = new Promise<SavedPageExport>((resolve) => {
      resolveSave = resolve;
    });
    const saveFile = vi.fn(async () => pending);
    render(<PageExportDialog host={HOST} open onClose={onClose} saveFile={saveFile} />);
    fireEvent.change(scopeSelect(), { target: { value: "No images" } });
    await user.click(screen.getByRole("button", { name: "Export" }));
    expect(screen.getByRole("button", { name: "Exporting" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(formatSelect()).toBeDisabled();
    resolveSave({ filename: "export-me-a4.pdf" });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(notifySuccess).toHaveBeenCalledWith("Page exported successfully.");
    expect(saveFile).toHaveBeenCalledTimes(1);
    expect(saveFile).toHaveBeenCalledWith(
      HOST,
      { format: "PDF", scope: "No images", size: "A4" },
      expect.objectContaining({ origin: window.location.origin, sink: expect.any(Object) })
    );
  });

  it("keeps the dialog open with choices intact when the save fails", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    const saveFile = vi.fn(async () => {
      throw new Error("sabotaged save");
    });
    render(<PageExportDialog host={HOST} open onClose={onClose} saveFile={saveFile} />);
    fireEvent.change(formatSelect(), { target: { value: "Markdown" } });
    fireEvent.change(scopeSelect(), { target: { value: "No images" } });
    await user.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Page could not be exported.", "Please try again later.")
    );
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Export page" })).toBeInTheDocument();
    expect(formatSelect().value).toBe("Markdown");
    expect(scopeSelect().value).toBe("No images");
    const confirm = screen.getByRole("button", { name: "Export" });
    expect(confirm).toBeEnabled();
  });

  it("cancelling downloads nothing", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    const saveFile = vi.fn(async () => ({ filename: "x.pdf" }));
    render(<PageExportDialog host={HOST} open onClose={onClose} saveFile={saveFile} />);
    fireEvent.change(formatSelect(), { target: { value: "Markdown" } });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(saveFile).not.toHaveBeenCalled();
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("dismisses on Escape when idle but not while saving", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    let resolveSave!: (saved: SavedPageExport) => void;
    const pending = new Promise<SavedPageExport>((resolve) => {
      resolveSave = resolve;
    });
    const saveFile = vi.fn(async () => pending);
    render(<PageExportDialog host={HOST} open onClose={onClose} saveFile={saveFile} />);
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);

    onClose.mockClear();
    await user.click(screen.getByRole("button", { name: "Export" }));
    await user.keyboard("{Escape}");
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Export page" })).toBeInTheDocument();
    resolveSave({ filename: "export-me-a4.pdf" });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("confirms with Enter on the focused Export button", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    const saveFile = vi.fn(async () => ({ filename: "export-me-a4.pdf" }));
    render(<PageExportDialog host={HOST} open onClose={onClose} saveFile={saveFile} />);
    // The mount autofocus fires once, asynchronously: settle it before
    // steering focus, or it can yank focus away mid-keystroke.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
    const confirm = screen.getByRole("button", { name: "Export" });
    confirm.focus();
    expect(document.activeElement).toBe(confirm);
    await user.keyboard("{Enter}");
    await waitFor(() => expect(saveFile).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("orders every control for Tab with a visible focus ring", () => {
    const saveFile = vi.fn(async () => ({ filename: "x.pdf" }));
    render(<PageExportDialog host={HOST} open onClose={() => undefined} saveFile={saveFile} />);
    // Tab follows DOM order through the trap: close, the three pickers in
    // row order, then the footer actions. Order is asserted structurally:
    // live Tab driving through the trap simulation goes unstable once
    // several dialogs mount in one file, and NEWFRONT-773 proves live
    // keyboard order across surfaces in a real browser.
    const dialog = screen.getByRole("dialog", { name: "Export page" });
    const controls = [...dialog.querySelectorAll<HTMLElement>("button, select")];
    const labelOf = (control: HTMLElement): string => {
      const labelled = control.getAttribute("aria-label");
      if (labelled !== null) return `${control.tagName}:${labelled}`;
      if (control.id !== "") return `${control.tagName}:${control.id}`;
      return `${control.tagName}:${(control.textContent ?? "").trim()}`;
    };
    expect(controls.map(labelOf)).toEqual([
      "BUTTON:Close dialog",
      "SELECT:page-export-format",
      "SELECT:page-export-scope",
      "SELECT:page-export-size",
      "BUTTON:Cancel",
      "BUTTON:Export",
    ]);
    for (const control of controls) {
      expect(control.tabIndex).toBeGreaterThanOrEqual(0);
      expect(control.className).toContain("focus-visible:outline-solid");
    }
  });

  it("saves through the browser surface by default", async () => {
    const user = userEvent.setup();
    Object.defineProperty(URL, "createObjectURL", { value: () => "blob:dialog", configurable: true, writable: true });
    const revoked: string[] = [];
    Object.defineProperty(URL, "revokeObjectURL", {
      value: (url: string) => {
        revoked.push(url);
      },
      configurable: true,
      writable: true,
    });
    const clicks: string[] = [];
    const click = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function (this: HTMLAnchorElement) {
      clicks.push(`${this.href} ${this.download}`);
    };
    try {
      const onClose = vi.fn();
      render(<PageExportDialog host={HOST} open onClose={onClose} />);
      fireEvent.change(formatSelect(), { target: { value: "Markdown" } });
      await user.click(screen.getByRole("button", { name: "Export" }));
      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
      expect(notifySuccess).toHaveBeenCalledWith("Page exported successfully.");
      expect(clicks).toEqual(["blob:dialog export-me.md"]);
      expect(revoked).toEqual(["blob:dialog"]);
    } finally {
      HTMLAnchorElement.prototype.click = click;
    }
  });
});
