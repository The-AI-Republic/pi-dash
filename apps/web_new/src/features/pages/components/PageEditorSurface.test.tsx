// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editable page canvas (parity/specs/pages.md §5, PAGE-026–032): the
// title's Enter reaches the body's start, the bar drives the editor, the
// editor reports its formatting state back, and the cold-document
// skeleton holds the body area until the document is known.

import { act, fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { resetPageBodyDrafts, setPageBodyDraft } from "../lib/pageBodyDraft.js";
import { PageEditorSurface, PAGE_BODY_LABEL, PAGE_BODY_PLACEHOLDER } from "./PageEditorSurface.js";

const EDITOR_TEST_TIMEOUT = 30_000;

function mount(props: Partial<React.ComponentProps<typeof PageEditorSurface>> = {}): ReturnType<typeof render> {
  return render(
    <PageEditorSurface
      pageId="page-1"
      name="Launch notes"
      descriptionHtml="<p>Stored words</p>"
      onRename={vi.fn()}
      showBar
      {...props}
    />
  );
}

function editableBody(): HTMLElement | null {
  return (
    screen.queryAllByLabelText(PAGE_BODY_LABEL).find((element) => element.getAttribute("contenteditable") === "true") ??
    null
  );
}

async function body(): Promise<HTMLElement> {
  await vi.dynamicImportSettled();
  // The read-only body is the Suspense fallback; wait for the editor.
  await vi.waitFor(() => expect(editableBody()).not.toBeNull());
  const editable = editableBody();
  if (editable === null) throw new Error("the editor never became editable");
  return editable;
}

afterEach(() => {
  resetPageBodyDrafts();
});

describe(
  "PageEditorSurface",
  () => {
    it("renders the title, the bar and the editor for an editable page", async () => {
      mount();
      expect(screen.getByRole("textbox", { name: "Page title" })).toBeInTheDocument();
      expect(screen.getByRole("toolbar", { name: "Formatting" })).toBeInTheDocument();
      expect(await body()).toHaveAttribute("contenteditable", "true");
    });

    it("withholds the bar when asked, for the sticky preference", async () => {
      mount({ showBar: false });
      await body();
      expect(screen.queryByRole("toolbar", { name: "Formatting" })).not.toBeInTheDocument();
    });

    it("holds the body area with a skeleton until the document is known", async () => {
      mount({ bodyPending: true });
      expect(screen.getByRole("status", { name: "Loading page body" })).toBeInTheDocument();
      expect(screen.getByRole("textbox", { name: "Page title" })).toBeInTheDocument();
      expect(screen.queryByLabelText(PAGE_BODY_LABEL)).not.toBeInTheDocument();
    });

    it("opens the stored body and reports edits as a draft", async () => {
      mount();
      const editable = await body();
      expect(editable).toHaveTextContent("Stored words");
      await vi.waitFor(() => expect(screen.getByRole("button", { name: "Bold" })).toBeInTheDocument());
      act(() => {
        fireEvent.click(screen.getByRole("button", { name: "Bold" }));
      });
      await vi.waitFor(() =>
        expect(screen.getByRole("button", { name: "Bold" })).toHaveAttribute("aria-pressed", "true")
      );
    });

    it("reopens a body typed earlier this session over the stored one", async () => {
      setPageBodyDraft("page-1", "<p>Typed earlier</p>");
      mount();
      expect(await body()).toHaveTextContent("Typed earlier");
    });

    it("puts the caret at the body's top when Enter leaves the title", async () => {
      mount();
      const editable = await body();
      const field = screen.getByRole("textbox", { name: "Page title" });
      fireEvent.focus(field);
      fireEvent.keyDown(field, { key: "Enter" });
      await vi.waitFor(() => expect(document.activeElement).toBe(editable));
    });

    it("shows the page-body placeholder while the document is empty", async () => {
      mount({ descriptionHtml: "<p></p>" });
      await body();
      expect(screen.getByText(PAGE_BODY_PLACEHOLDER)).toBeInTheDocument();
    });

    it("runs the alignment chords the editor itself does not bind", async () => {
      mount();
      const editable = await body();
      editable.focus();
      const event = fireEvent.keyDown(editable, { key: "e", metaKey: true, shiftKey: true });
      expect(event).toBe(false);
      await vi.waitFor(() =>
        expect(screen.getByRole("button", { name: "Center align" })).toHaveAttribute("aria-pressed", "true")
      );
    });

    it("leaves the editor's own chords alone", async () => {
      mount();
      const editable = await body();
      editable.focus();
      // Bold is the editor's own binding; the surface must not also fire it.
      expect(fireEvent.keyDown(editable, { key: "b", metaKey: true })).toBe(true);
    });

    it("restyles the caret's block from the typography selector", async () => {
      mount();
      await body();
      await act(async () => {
        fireEvent.click(screen.getByTestId("page-toolbar-typography"));
      });
      await act(async () => {
        fireEvent.click(await screen.findByRole("menuitem", { name: "Heading 2" }));
      });
      await vi.waitFor(() => expect(screen.getByTestId("page-toolbar-typography")).toHaveTextContent("Heading 2"));
    });
  },
  EDITOR_TEST_TIMEOUT
);
