// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Snapshot restore write (parity/specs/pages.md §6.9, PAGE-053): the
// snapshot's body reaches the live editor in place, and a canvas without a
// live editor or an editor that refuses the write answers false so the
// reader is told instead of being shown a silent no-op.

import { describe, expect, it, vi } from "vitest";

import type { PageEditorHandle } from "./pageEditorAdapter.js";
import { applyPageRestore, PAGE_VERSION_RESTORE_FAILED, PAGE_VERSION_RESTORE_SUCCESS } from "./pageRestore.js";

function handle(overrides: Partial<PageEditorHandle> = {}): PageEditorHandle {
  return {
    applyCommand: vi.fn(),
    applyBlockKind: vi.fn(),
    insertTable: vi.fn(),
    applyColor: vi.fn(),
    clearColor: vi.fn(),
    insertImage: vi.fn(),
    focusStart: vi.fn(),
    readState: vi.fn(),
    replaceBody: vi.fn(() => true),
    applyCatalogBlock: vi.fn(),
    insertEmoji: vi.fn(),
    insertMention: vi.fn(),
    insertEmbed: vi.fn(),
    insertEntityReference: vi.fn(),
    insertText: vi.fn(),
    duplicateBlock: vi.fn(),
    deleteBlock: vi.fn(),
    setLink: vi.fn(),
    unsetLink: vi.fn(),
    html: vi.fn(() => ""),
    linkHref: vi.fn(() => null),
    hasSelection: vi.fn(() => false),
    textBeforeCaret: vi.fn(() => ""),
    deleteBeforeCaret: vi.fn(),
    selectedText: vi.fn(() => ""),
    focusEnd: vi.fn(),
    ...overrides,
  };
}

describe("applyPageRestore", () => {
  it("writes the snapshot markup into the live editor", () => {
    const editor = handle();
    expect(applyPageRestore(editor, "<p>Snapshot body</p>")).toBe(true);
    expect(editor.replaceBody).toHaveBeenCalledWith("<p>Snapshot body</p>");
  });

  it("answers false when there is no live editor to write into", () => {
    expect(applyPageRestore(null, "<p>Snapshot body</p>")).toBe(false);
  });

  it("answers false when the editor refuses the write", () => {
    const editor = handle({
      replaceBody: () => {
        throw new Error("destroyed");
      },
    });
    expect(applyPageRestore(editor, "<p>Snapshot body</p>")).toBe(false);
  });

  it("answers the editor's own refusal, which is not an exception", () => {
    const editor = handle({ replaceBody: () => false });
    expect(applyPageRestore(editor, "<p>Snapshot body</p>")).toBe(false);
  });

  it("pins the notices the reader sees for each outcome", () => {
    expect(PAGE_VERSION_RESTORE_SUCCESS).toBe("Page version restored.");
    expect(PAGE_VERSION_RESTORE_FAILED).toBe("Page version could not be restored.");
  });
});
