// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor-level proof for the toolbar's advertised shortcuts and the
// bar's caret tracking (parity/specs/pages.md §5.3, §9.2,
// PAGE-028–032): every chord in `PAGE_TOOLBAR_SHORTCUTS` runs against a
// real Tiptap instance and leaves the mark, alignment or list the bar
// highlights, and moving the caret between blocks changes the block the
// selector names.

import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { PageFormattingState } from "../../../shared/editor/index.js";
import type { PageEditorHandle } from "../lib/pageEditorAdapter.js";
import { PAGE_TOOLBAR_SHORTCUTS, pageKeyCommand } from "../lib/pageToolbar.js";
import { PageBodyEditor } from "./PageBodyEditor.js";

const LOAD_TIMEOUT = { timeout: 10_000 };
const TEST_TIMEOUT = 30_000;

const MODIFIER = /Mac|iP/.test(navigator.platform) ? "Meta" : "Control";

interface Mounted {
  surface: HTMLElement;
  handle: PageEditorHandle;
  states: PageFormattingState[];
}

async function mount(initialHtml = "<h2>Heading</h2><p>Body words</p>"): Promise<Mounted> {
  const handles: PageEditorHandle[] = [];
  const states: PageFormattingState[] = [];
  render(
    <React.Suspense fallback={null}>
      <PageBodyEditor
        initialHtml={initialHtml}
        onChange={() => undefined}
        onFormattingChange={(state) => states.push(state)}
        onReady={(handle) => handles.push(handle)}
        label="Page body"
        // The page's own handler, as PageEditorSurface installs it: only
        // the chords the editor leaves unbound run here.
        handleKeyDown={(event) => {
          const command = pageKeyCommand(event);
          if (command === null) return false;
          event.preventDefault();
          handles[handles.length - 1]?.applyCommand(command);
          return true;
        }}
      />
    </React.Suspense>
  );
  const surface = await screen.findByLabelText("Page body", undefined, LOAD_TIMEOUT);
  surface.focus();
  return { surface, handle: handles[0] as PageEditorHandle, states };
}

/** The DOM text node holding `needle`, for a selection real typing shares. */
function textNodeOf(root: HTMLElement, needle: string): Node {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    if ((node.textContent ?? "").includes(needle)) return node;
  }
  throw new Error(`no text node holds ${needle}`);
}

/** Select `needle` inside the editor and let ProseMirror see the change. */
function selectText(surface: HTMLElement, needle: string): void {
  const node = textNodeOf(surface, needle);
  const start = (node.textContent ?? "").indexOf(needle);
  const range = document.createRange();
  range.setStart(node, start);
  range.setEnd(node, start + needle.length);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  document.dispatchEvent(new Event("selectionchange"));
}

/** Put a collapsed caret at the start of `needle`. */
function caretAt(surface: HTMLElement, needle: string): void {
  const node = textNodeOf(surface, needle);
  const range = document.createRange();
  range.setStart(node, (node.textContent ?? "").indexOf(needle));
  range.collapse(true);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  document.dispatchEvent(new Event("selectionchange"));
}

/** Press one advertised chord the way the platform writes it. */
async function chord(user: ReturnType<typeof userEvent.setup>, row: { key: string; shift: boolean }): Promise<void> {
  const keys = [`{${MODIFIER}>}`, row.shift ? "{Shift>}" : "", row.key, row.shift ? "{/Shift}" : "", `{/${MODIFIER}}`];
  await user.keyboard(keys.join(""));
  await act(async () => {
    await Promise.resolve();
  });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe(
  "advertised toolbar chords against the live editor",
  () => {
    it("bold: Mod+B marks the selection and the bar reads it active", async () => {
      const user = userEvent.setup();
      const { surface, handle } = await mount();
      selectText(surface, "Body");
      await chord(user, { key: "b", shift: false });
      expect(surface.querySelector("p strong")?.textContent).toBe("Body");
      await vi.waitFor(() => expect(handle.readState().bold).toBe(true));
    });

    it("italic: Mod+I marks the selection", async () => {
      const user = userEvent.setup();
      const { surface, handle } = await mount();
      selectText(surface, "Body");
      await chord(user, { key: "i", shift: false });
      expect(surface.querySelector("p em")?.textContent).toBe("Body");
      await vi.waitFor(() => expect(handle.readState().italic).toBe(true));
    });

    it("underline: Mod+U marks the selection", async () => {
      const user = userEvent.setup();
      const { surface, handle } = await mount();
      selectText(surface, "Body");
      await chord(user, { key: "u", shift: false });
      expect(surface.querySelector("p u")?.textContent).toBe("Body");
      await vi.waitFor(() => expect(handle.readState().underline).toBe(true));
    });

    it("strikethrough: Mod+Shift+S marks the selection", async () => {
      const user = userEvent.setup();
      const { surface, handle } = await mount();
      selectText(surface, "Body");
      await chord(user, { key: "s", shift: true });
      expect(surface.querySelector("p s")?.textContent).toBe("Body");
      await vi.waitFor(() => expect(handle.readState().strike).toBe(true));
    });

    it("ordered list: Mod+Shift+7 converts the block", async () => {
      const user = userEvent.setup();
      const { surface, handle } = await mount("<p>Body words</p>");
      caretAt(surface, "Body");
      await chord(user, { key: "7", shift: true });
      expect(surface.querySelector("ol li")?.textContent).toBe("Body words");
      await vi.waitFor(() => expect(handle.readState().list).toBe("ordered"));
    });

    it("bulleted list: Mod+Shift+8 converts the block", async () => {
      const user = userEvent.setup();
      const { surface, handle } = await mount("<p>Body words</p>");
      caretAt(surface, "Body");
      await chord(user, { key: "8", shift: true });
      expect(surface.querySelector("ul li")?.textContent).toBe("Body words");
      await vi.waitFor(() => expect(handle.readState().list).toBe("bullet"));
    });

    it("to-do list: Mod+Shift+9 converts the block", async () => {
      const user = userEvent.setup();
      const { surface, handle } = await mount("<p>Body words</p>");
      caretAt(surface, "Body");
      await chord(user, { key: "9", shift: true });
      expect(surface.querySelector("ul[data-type='taskList'] li")).not.toBeNull();
      await vi.waitFor(() => expect(handle.readState().list).toBe("task"));
    });

    it.each([
      { key: "l", align: "left" },
      { key: "e", align: "center" },
      { key: "r", align: "right" },
    ] as const)("alignment $key: Mod+Shift+$key realigns the block", async ({ key, align }) => {
      const user = userEvent.setup();
      const { surface, handle } = await mount("<p>Body words</p>");
      caretAt(surface, "Body");
      await chord(user, { key, shift: true });
      expect(handle.readState().align).toBe(align);
      expect(surface.querySelector("p")?.getAttribute("style") ?? "").toContain(`text-align: ${align}`);
    });

    it("leaves every editor-native chord unclaimed so nothing toggles twice", () => {
      for (const row of PAGE_TOOLBAR_SHORTCUTS.filter((entry) => entry.native)) {
        expect(
          pageKeyCommand({ key: row.key, metaKey: true, ctrlKey: false, altKey: false, shiftKey: row.shift })
        ).toBeNull();
      }
    });

    it("claims exactly the alignment chords for the page's own handler", () => {
      const claimed = PAGE_TOOLBAR_SHORTCUTS.filter(
        (row) =>
          pageKeyCommand({ key: row.key, metaKey: true, ctrlKey: false, altKey: false, shiftKey: row.shift }) !== null
      );
      expect(claimed.map((row) => row.command)).toEqual(["alignLeft", "alignCenter", "alignRight"]);
    });
  },
  TEST_TIMEOUT
);

describe(
  "formatting state at the caret",
  () => {
    it("follows the caret from a heading into a paragraph", async () => {
      const { surface, handle } = await mount();
      caretAt(surface, "Heading");
      await vi.waitFor(() => expect(handle.readState().block).toBe("heading2"));
      caretAt(surface, "Body");
      await vi.waitFor(() => expect(handle.readState().block).toBe("paragraph"));
      caretAt(surface, "Heading");
      await vi.waitFor(() => expect(handle.readState().block).toBe("heading2"));
    });

    it("reports the marks, alignment and list under the caret", async () => {
      const user = userEvent.setup();
      const { surface, handle } = await mount();
      selectText(surface, "Body");
      await chord(user, { key: "b", shift: false });
      await chord(user, { key: "e", shift: true });
      caretAt(surface, "Body");
      await vi.waitFor(() =>
        expect(handle.readState()).toMatchObject({ bold: true, align: "center", block: "paragraph" })
      );
    });

    it("reads the heading's own state, not the paragraph's", async () => {
      const { surface, handle } = await mount();
      caretAt(surface, "Heading");
      await vi.waitFor(() =>
        expect(handle.readState()).toMatchObject({
          bold: false,
          italic: false,
          underline: false,
          strike: false,
          blockquote: false,
          code: false,
          block: "heading2",
        })
      );
    });
  },
  TEST_TIMEOUT
);

describe(
  "quote and inline code",
  () => {
    it("restyles the caret's block as a quote and reads it active", async () => {
      const { surface, handle } = await mount();
      caretAt(surface, "Body");
      act(() => handle.applyCommand("blockquote"));
      await vi.waitFor(() => expect(surface.querySelector("blockquote p")?.textContent).toBe("Body words"));
      await vi.waitFor(() => expect(handle.readState().blockquote).toBe(true));
    });

    it("restyles the selection as inline code and reads it active", async () => {
      const { surface, handle } = await mount();
      selectText(surface, "Body");
      act(() => handle.applyCommand("code"));
      await vi.waitFor(() => expect(surface.querySelector("p code")?.textContent).toBe("Body"));
      await vi.waitFor(() => expect(handle.readState().code).toBe(true));
    });
  },
  TEST_TIMEOUT
);
