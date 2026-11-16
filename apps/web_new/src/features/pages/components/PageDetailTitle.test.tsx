// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Title surface (parity/specs/pages.md §5.1, PAGE-026): one in-place
// heading inside the title editor, the 255-character cap (type and
// paste), the focus-only count, Enter leaving for the body, and the
// read-only face that keeps the heading shape with its placeholder.

import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as Y from "yjs";

import {
  PageDetailTitle,
  PAGE_TITLE_MAX_LENGTH,
  PAGE_TITLE_PLACEHOLDER,
  PAGE_TITLE_SAVE_DEBOUNCE_MS,
  TitleHeading,
  titleDocument,
  titleShapeOf,
  titleTextBetween,
  titleTextOf,
  TITLE_EDITOR_ATTRIBUTES,
} from "./PageDetailTitle.js";
import { clipTitle, pastedTitleOf, pasteTitleText, titleLength, titleOverflow, titleRoom } from "../lib/pageTitle.js";

const LOAD_TIMEOUT = { timeout: 10_000 };
const TITLE_TEST_TIMEOUT = 30_000;

interface Mounted {
  onRename: ReturnType<typeof vi.fn>;
  onJumpToBody: ReturnType<typeof vi.fn>;
  onDraftChange: ReturnType<typeof vi.fn>;
  surface: () => HTMLElement;
  container: HTMLElement;
  rerender: (props: Partial<React.ComponentProps<typeof PageDetailTitle>>) => void;
}

/** The editing surface the oracle reads: `.page-title-editor .ProseMirror`. */
function surfaceOf(): HTMLElement {
  const surface = document.querySelector<HTMLElement>(".page-title-editor .ProseMirror");
  if (surface === null) throw new Error("the title editor never mounted");
  return surface;
}

async function mount(props: Partial<React.ComponentProps<typeof PageDetailTitle>> = {}): Promise<Mounted> {
  const onRename = vi.fn();
  const onJumpToBody = vi.fn();
  const onDraftChange = vi.fn();
  const view = render(
    <PageDetailTitle
      pageId="page-1"
      name="Old"
      editable
      onRename={onRename}
      onJumpToBody={onJumpToBody}
      onDraftChange={onDraftChange}
      {...props}
    />
  );
  await vi.waitFor(
    () => expect(document.querySelector(".page-title-editor .ProseMirror")).not.toBeNull(),
    LOAD_TIMEOUT
  );
  return {
    onRename,
    onJumpToBody,
    onDraftChange,
    surface: surfaceOf,
    container: view.container,
    rerender: (next) =>
      view.rerender(
        <PageDetailTitle
          pageId="page-1"
          name="Old"
          editable
          onRename={onRename}
          onJumpToBody={onJumpToBody}
          onDraftChange={onDraftChange}
          {...next}
        />
      ),
  };
}

/** The h1 the title's document renders. */
function headingOf(): HTMLElement {
  const heading = surfaceOf().querySelector<HTMLElement>("h1");
  if (heading === null) throw new Error("the title has no heading");
  return heading;
}

/** Put the caret at the very end of the one heading. */
function caretToEnd(): void {
  const heading = headingOf();
  const range = document.createRange();
  range.selectNodeContents(heading);
  range.collapse(false);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  document.dispatchEvent(new Event("selectionchange"));
}

afterEach(() => {
  vi.useRealTimers();
});

describe("title rules", () => {
  it("clips a stored name to the cap and reports its room", () => {
    expect(clipTitle("short")).toBe("short");
    expect(clipTitle("x".repeat(PAGE_TITLE_MAX_LENGTH + 4))).toHaveLength(PAGE_TITLE_MAX_LENGTH);
    expect(titleRoom(PAGE_TITLE_MAX_LENGTH - 3)).toBe(3);
    expect(titleRoom(PAGE_TITLE_MAX_LENGTH + 9)).toBe(0);
    expect(titleLength("abc")).toBe(3);
    expect(titleOverflow("abc")).toBe(0);
    expect(titleOverflow("x".repeat(PAGE_TITLE_MAX_LENGTH + 2))).toBe(2);
    expect(titleTextOf("  padded  ")).toBe("  padded  ");
  });

  it("collapses whitespace in a pasted title so it stays one line", () => {
    expect(pasteTitleText("one\n\ntwo\tthree  ")).toBe("one two three");
    expect(pastedTitleOf("one\ntwo")).toBe("one two");
    expect(pastedTitleOf(undefined)).toBe("");
  });
});

describe("title document", () => {
  it("holds one level-one heading, with or without text", () => {
    expect(titleDocument("")).toEqual({ type: "doc", content: [{ type: "heading", attrs: { level: 1 } }] });
    expect(titleDocument("Hi")).toEqual({
      type: "doc",
      content: [{ type: "heading", attrs: { level: 1 }, content: [{ type: "text", text: "Hi" }] }],
    });
  });

  it("reads the text of the document with no block separators", () => {
    const texts: string[] = [];
    const doc = {
      content: { size: 4 },
      textBetween: (from: number, to: number, blockSeparator: string, leafText: string) => {
        texts.push(`${from}:${to}:${blockSeparator}:${leafText}`);
        return "words";
      },
    };
    expect(titleTextBetween(doc)).toBe("words");
    expect(texts).toEqual(["0:4::"]);
  });

  it("carries the surface attributes the editor and the oracle read", () => {
    expect(TITLE_EDITOR_ATTRIBUTES).toMatchObject({ role: "textbox", "aria-label": "Page title" });
  });

  it("folds a drifted document back into one heading and keeps the caret", () => {
    const children = [
      { nodeSize: 3, textContent: "ab" },
      { nodeSize: 4, textContent: "cd" },
    ];
    const doc = {
      childCount: 2,
      forEach: (visit: (child: (typeof children)[number], offset: number) => void) => {
        let offset = 0;
        for (const child of children) {
          visit(child, offset);
          offset += child.nodeSize;
        }
      },
    };
    // Inside the first block's text (offset 0 -> start 1).
    expect(titleShapeOf(doc, 2)).toEqual({ text: "abcd", caret: 1 });
    // Inside the second block: its offset in the joined text.
    expect(titleShapeOf(doc, 5)).toEqual({ text: "abcd", caret: 3 });
    // Beyond the text: clamped to its end.
    expect(titleShapeOf(doc, 99)).toEqual({ text: "abcd", caret: 4 });
  });

  it("draws the placeholder heading for the pre-chunk face", () => {
    const { rerender } = render(<TitleHeading value="" editable={false} />);
    expect(screen.getByRole("textbox", { name: "Page title" })).toHaveAttribute(
      "data-placeholder",
      PAGE_TITLE_PLACEHOLDER
    );
    expect(screen.getByRole("textbox", { name: "Page title" })).toHaveTextContent("");
    rerender(<TitleHeading value="Named" editable />);
    expect(screen.getByRole("textbox", { name: "Page title" })).toHaveTextContent("Named");
    expect(screen.getByRole("textbox", { name: "Page title" })).not.toHaveAttribute("data-empty");
  });
});

describe(
  "PageDetailTitle editable",
  () => {
    it("edits the name in place inside the title editor", async () => {
      const mounted = await mount({ name: "Launch notes" });
      expect(mounted.surface()).toHaveAttribute("contenteditable", "true");
      expect(mounted.surface()).toHaveAttribute("role", "textbox");
      expect(headingOf()).toHaveTextContent("Launch notes");
      expect(mounted.surface().closest("#page-1-title")).not.toBeNull();
    });

    it("keeps the count hidden until the field is focused", async () => {
      await mount({ name: "abc" });
      expect(screen.queryByText(`3/${PAGE_TITLE_MAX_LENGTH}`)).not.toBeInTheDocument();
      const user = userEvent.setup();
      surfaceOf().focus();
      await user.click(surfaceOf());
      expect(await screen.findByText(`3/${PAGE_TITLE_MAX_LENGTH}`)).toBeInTheDocument();
    });

    it("types, counts and saves the draft on the debounce", async () => {
      const mounted = await mount({ name: "" });
      const user = userEvent.setup();
      surfaceOf().focus();
      caretToEnd();
      await user.keyboard("Hello");
      expect(headingOf()).toHaveTextContent("Hello");
      expect(mounted.onDraftChange).toHaveBeenCalledWith("Hello");
      expect(screen.getByText(`5/${PAGE_TITLE_MAX_LENGTH}`)).toBeInTheDocument();
      await vi.waitFor(() => expect(mounted.onRename).toHaveBeenCalledWith("Hello"), {
        timeout: PAGE_TITLE_SAVE_DEBOUNCE_MS + 2_000,
      });
    });

    it("refuses typing past the cap", async () => {
      const mounted = await mount({ name: "" });
      const user = userEvent.setup();
      surfaceOf().focus();
      caretToEnd();
      await user.keyboard("x".repeat(PAGE_TITLE_MAX_LENGTH + 5));
      expect(headingOf().textContent).toHaveLength(PAGE_TITLE_MAX_LENGTH);
      expect(screen.getByText(`${PAGE_TITLE_MAX_LENGTH}/${PAGE_TITLE_MAX_LENGTH}`)).toBeInTheDocument();
      await vi.waitFor(() => expect(mounted.onRename).toHaveBeenCalledWith("x".repeat(PAGE_TITLE_MAX_LENGTH)), {
        timeout: PAGE_TITLE_SAVE_DEBOUNCE_MS + 2_000,
      });
    });

    it("pastes plain text only up to the room the title has", async () => {
      const mounted = await mount({ name: "Old" });
      const user = userEvent.setup();
      surfaceOf().focus();
      caretToEnd();
      await user.paste("y\n".repeat(PAGE_TITLE_MAX_LENGTH));
      const text = headingOf().textContent ?? "";
      expect(text).toHaveLength(PAGE_TITLE_MAX_LENGTH);
      expect(text.startsWith("Old")).toBe(true);
      expect(text).not.toContain("\n");
      expect(mounted.onDraftChange).toHaveBeenCalled();
    });

    it("refuses a blank paste without changing the title", async () => {
      await mount({ name: "Old" });
      const user = userEvent.setup();
      surfaceOf().focus();
      const before = headingOf().textContent;
      await user.paste("   ");
      expect(headingOf().textContent).toBe(before);
    });

    it("folds a select-all delete back into the one empty heading", async () => {
      const mounted = await mount({ name: "Old" });
      const user = userEvent.setup();
      surfaceOf().focus();
      caretToEnd();
      await user.keyboard("er");
      await vi.waitFor(() => expect(mounted.onRename).toHaveBeenCalledWith("Older"), {
        timeout: PAGE_TITLE_SAVE_DEBOUNCE_MS + 2_000,
      });
      await user.clear(surfaceOf());
      expect(surfaceOf().querySelectorAll("h1")).toHaveLength(1);
      expect(surfaceOf().querySelectorAll("p")).toHaveLength(0);
      expect(headingOf().textContent).toBe("");
      await vi.waitFor(() => expect(mounted.onRename).toHaveBeenCalledWith(""), {
        timeout: PAGE_TITLE_SAVE_DEBOUNCE_MS + 20_000,
      });
    });

    it("paints the placeholder on the empty heading, never a literal", async () => {
      await mount({ name: "" });
      await vi.waitFor(() => {
        expect(headingOf()).toHaveAttribute("data-placeholder", PAGE_TITLE_PLACEHOLDER);
      });
      expect(headingOf()).toHaveAttribute("data-empty", "true");
      expect(headingOf().textContent).toBe("");
    });

    it("drops the placeholder once the title has text", async () => {
      await mount({ name: "Named" });
      expect(headingOf()).not.toHaveAttribute("data-placeholder");
      expect(surfaceOf().closest("[data-title-editor-id]")).not.toHaveAttribute("data-empty");
    });

    it("Enter commits the draft and hands the body the caret, adding no line", async () => {
      const mounted = await mount({ name: "Old" });
      const user = userEvent.setup();
      surfaceOf().focus();
      caretToEnd();
      await user.keyboard("er");
      await user.keyboard("{Enter}");
      expect(mounted.onRename).toHaveBeenCalledWith("Older");
      expect(mounted.onJumpToBody).toHaveBeenCalled();
      expect(surfaceOf().querySelectorAll("p")).toHaveLength(0);
      expect(surfaceOf().querySelectorAll("h1")).toHaveLength(1);
      expect(headingOf().textContent).not.toContain("\n");
    });

    it("leaves other keys to the field", async () => {
      const mounted = await mount({ name: "Old" });
      surfaceOf().focus();
      expect(fireEvent.keyDown(surfaceOf(), { key: "a" })).toBe(true);
      expect(mounted.onJumpToBody).not.toHaveBeenCalled();
    });

    it("commits the latest draft when the field loses focus", async () => {
      const mounted = await mount({ name: "Old" });
      const user = userEvent.setup();
      surfaceOf().focus();
      caretToEnd();
      await user.keyboard("er");
      surfaceOf().blur();
      expect(mounted.onRename).toHaveBeenCalledWith("Older");
    });

    it("saves immediately on Enter even before the debounce elapses", async () => {
      const mounted = await mount({ name: "Old" });
      const user = userEvent.setup();
      surfaceOf().focus();
      caretToEnd();
      await user.keyboard("er{Enter}");
      expect(mounted.onRename).toHaveBeenCalledTimes(1);
      expect(mounted.onRename).toHaveBeenCalledWith("Older");
    });

    it("adopts a rename from elsewhere when the field is idle", async () => {
      const mounted = await mount({ name: "Before" });
      mounted.rerender({ name: "After" });
      await vi.waitFor(() => expect(headingOf().textContent).toBe("After"));
    });

    it("leaves a focused draft alone when the server name changes", async () => {
      const user = userEvent.setup();
      const mounted = await mount({ name: "Before" });
      surfaceOf().focus();
      caretToEnd();
      await user.keyboard("!");
      expect(headingOf().textContent).toBe("Before!");
      mounted.rerender({ name: "Remote rename" });
      await vi.waitFor(() => expect(headingOf().textContent).toBe("Before!"));
    });

    it("sends nothing when Enter commits an unchanged title", async () => {
      const mounted = await mount({ name: "Same" });
      const user = userEvent.setup();
      surfaceOf().focus();
      caretToEnd();
      await user.keyboard("{Enter}");
      expect(mounted.onRename).not.toHaveBeenCalled();
      expect(mounted.onJumpToBody).toHaveBeenCalled();
    });

    it("refuses a paste that carries no plain text", async () => {
      const mounted = await mount({ name: "Old" });
      surfaceOf().focus();
      const before = mounted.onDraftChange.mock.calls.length;
      // A paste event with no clipboard data at all (the browser handed
      // over no text/plain flavor) is refused as a blank paste.
      surfaceOf().dispatchEvent(new Event("paste", { bubbles: true, cancelable: true }));
      await vi.waitFor(() => expect(headingOf().textContent).toBe("Old"));
      expect(mounted.onDraftChange.mock.calls).toHaveLength(before);
    });

    it("shares one editor chunk across titles", async () => {
      await mount({ name: "First" });
      render(<PageDetailTitle pageId="page-2" name="Second" editable onRename={vi.fn()} />);
      await vi.waitFor(
        () => expect(document.querySelectorAll(".page-title-editor .ProseMirror")).toHaveLength(2),
        LOAD_TIMEOUT
      );
    });

    it("works without a rename handler", async () => {
      await mount({ onRename: undefined });
      const user = userEvent.setup();
      surfaceOf().focus();
      caretToEnd();
      await user.keyboard("new");
      expect(headingOf()).toHaveTextContent("Oldnew");
      expect(() => surfaceOf().blur()).not.toThrow();
    });
  },
  TITLE_TEST_TIMEOUT
);

describe(
  "PageDetailTitle read-only",
  () => {
    it("keeps the editor shape with editing off and no caret", async () => {
      await mount({ name: "Launch notes", editable: false });
      expect(surfaceOf()).toHaveAttribute("contenteditable", "false");
      expect(headingOf()).toHaveTextContent("Launch notes");
      expect(screen.queryByTestId("page-title-count")).not.toBeInTheDocument();
    });

    it("falls back to Untitled on an empty locked page", async () => {
      await mount({ name: "   ", editable: false });
      await vi.waitFor(() => expect(headingOf()).toHaveAttribute("data-placeholder", "Untitled"));
      expect(headingOf().textContent).toBe("");
    });

    it("never saves a title nobody can edit", async () => {
      const mounted = await mount({ name: "Locked", editable: false });
      fireEvent.keyDown(surfaceOf(), { key: "Enter" });
      expect(mounted.onRename).not.toHaveBeenCalled();
      expect(mounted.onJumpToBody).not.toHaveBeenCalled();
    });
  },
  TITLE_TEST_TIMEOUT
);

describe(
  "PageDetailTitle collaboration",
  () => {
    it("seeds the stored name into an empty shared field", async () => {
      const doc = new Y.Doc();
      await mount({ name: "Old", collab: { doc }, seedDocument: true });
      await vi.waitFor(() => expect(headingOf()).toHaveTextContent("Old"));
      const peer = new Y.Doc();
      Y.applyUpdate(peer, Y.encodeStateAsUpdate(doc));
      expect(peer.getXmlFragment("title").toString()).toBe('<heading level="1">Old</heading>');
    });

    it("waits for the server state before seeding", async () => {
      const doc = new Y.Doc();
      const collab = { doc };
      const mounted = await mount({ name: "Old", collab });
      expect(headingOf().textContent).toBe("");
      expect(doc.getXmlFragment("title").toString()).not.toContain("Old");
      mounted.rerender({ collab, seedDocument: true });
      await vi.waitFor(() => expect(headingOf()).toHaveTextContent("Old"));
      const peer = new Y.Doc();
      Y.applyUpdate(peer, Y.encodeStateAsUpdate(doc));
      expect(peer.getXmlFragment("title").toString()).toBe('<heading level="1">Old</heading>');
    });

    it("prefers the shared field over a stale stored name", async () => {
      const doc = new Y.Doc();
      const peer = new Y.Doc();
      const heading = new Y.XmlElement("heading");
      heading.insert(0, [new Y.XmlText("Shared")]);
      peer.getXmlFragment("title").insert(0, [heading]);
      Y.applyUpdate(doc, Y.encodeStateAsUpdate(peer));
      await mount({ name: "Stale", collab: { doc }, seedDocument: true });
      await vi.waitFor(() => expect(headingOf()).toHaveTextContent("Shared"));
      expect(headingOf()).not.toHaveTextContent("Stale");
    });

    it("renders a remote title update", async () => {
      const doc = new Y.Doc();
      await mount({ name: "Old", collab: { doc }, seedDocument: true });
      await vi.waitFor(() => expect(headingOf()).toHaveTextContent("Old"));
      const peer = new Y.Doc();
      Y.applyUpdate(peer, Y.encodeStateAsUpdate(doc));
      const heading = peer.getXmlFragment("title").get(0) as Y.XmlElement;
      const text = heading.get(0) as Y.XmlText;
      await act(async () => {
        text.delete(0, text.length);
        text.insert(0, "Remote");
      });
      await act(async () => {
        Y.applyUpdate(doc, Y.encodeStateAsUpdate(peer));
      });
      await vi.waitFor(() => expect(headingOf()).toHaveTextContent("Remote"));
    });

    it("mounts the standalone field on an explicit null binding", async () => {
      await mount({ name: "Old", collab: null });
      expect(headingOf()).toHaveTextContent("Old");
    });
  },
  TITLE_TEST_TIMEOUT
);
