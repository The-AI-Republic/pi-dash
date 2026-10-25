// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page body editor (parity/specs/pages.md §5.3, PAGE-026–032): the lazy
// editing surface reports its HTML, the formatting state at the caret and
// a command handle, and lets the title's Enter and the key map interrupt.

import { act, fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { PageEditorHandle } from "../lib/pageEditorAdapter.js";
import type { PageFormattingState } from "../../../shared/editor/index.js";
import { PageBodyEditor } from "./PageBodyEditor.js";

const LOAD_TIMEOUT = { timeout: 10_000 };
const EDITOR_TEST_TIMEOUT = 30_000;

interface Mounted {
  onChange: ReturnType<typeof vi.fn>;
  onFormattingChange: ReturnType<typeof vi.fn>;
  handles: PageEditorHandle[];
}

function mount(
  props: Partial<React.ComponentProps<typeof PageBodyEditor>> = {},
  fallback: React.ReactNode = <p>read only body</p>
): Mounted {
  const onChange = vi.fn();
  const onFormattingChange = vi.fn<(state: PageFormattingState) => void>();
  const handles: PageEditorHandle[] = [];
  render(
    <React.Suspense fallback={fallback}>
      <PageBodyEditor
        initialHtml="<p>Stored words</p>"
        onChange={onChange}
        onFormattingChange={onFormattingChange}
        onReady={(handle) => handles.push(handle)}
        label="Page body"
        {...props}
      />
    </React.Suspense>
  );
  return { onChange, onFormattingChange, handles };
}

async function surface(label = "Page body"): Promise<HTMLElement> {
  await vi.dynamicImportSettled();
  const editable = await screen.findByLabelText(label, undefined, LOAD_TIMEOUT);
  expect(editable).toHaveAttribute("contenteditable", "true");
  return editable;
}

async function handleOf(mounted: Mounted): Promise<PageEditorHandle> {
  await vi.waitFor(() => expect(mounted.handles.length).toBeGreaterThan(0), LOAD_TIMEOUT);
  const handle = mounted.handles[mounted.handles.length - 1];
  if (handle === undefined) throw new Error("no editor handle");
  return handle;
}

/** The editor's own view, for the formatting state it reports. */
function formattingOf(mounted: Mounted): PageFormattingState | undefined {
  const calls = mounted.onFormattingChange.mock.calls;
  return calls[calls.length - 1]?.[0];
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe(
  "PageBodyEditor",
  () => {
    it("shows the read-only fallback until the editor chunk arrives", async () => {
      mount();
      expect(screen.getByText("read only body")).toBeInTheDocument();
      await surface();
      expect(screen.queryByText("read only body")).not.toBeInTheDocument();
    });

    it("opens with the stored HTML", async () => {
      mount();
      expect(await surface()).toHaveTextContent("Stored words");
    });

    it("reports the edited HTML and the caret's formatting state", async () => {
      const mounted = mount();
      const editable = await surface();
      editable.focus();
      const handle = await handleOf(mounted);
      await act(async () => {
        handle.applyBlockKind("heading2");
      });
      await vi.waitFor(() => expect(mounted.onChange).toHaveBeenCalled());
      const last = mounted.onChange.mock.calls[mounted.onChange.mock.calls.length - 1]?.[0] as string;
      expect(last).toContain("<h2");
      expect(last).toContain("Stored words");
      await vi.waitFor(() => expect(formattingOf(mounted)?.block).toBe("heading2"));
    });

    it("hands out a handle that restyles the caret's block", async () => {
      const mounted = mount();
      await vi.dynamicImportSettled();
      const handle = await handleOf(mounted);
      await act(async () => {
        handle.applyBlockKind("heading2");
      });
      await vi.waitFor(() => expect(formattingOf(mounted)?.block).toBe("heading2"));
      await act(async () => {
        handle.focusStart();
      });
      await vi.waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("Page body")));
    });

    it("reads its formatting state on demand", async () => {
      const mounted = mount();
      await vi.dynamicImportSettled();
      const handle = await handleOf(mounted);
      expect(handle.readState()).toMatchObject({ block: "paragraph", align: "left" });
    });

    it("reports the list command it runs", async () => {
      const mounted = mount();
      const handle = await handleOf(mounted);
      await act(async () => {
        handle.applyCommand("taskList");
      });
      await vi.waitFor(() => expect(formattingOf(mounted)?.list).toBe("task"));
    });

    it("reports the alignment command it runs", async () => {
      const mounted = mount();
      const handle = await handleOf(mounted);
      await act(async () => {
        handle.applyCommand("alignCenter");
      });
      await vi.waitFor(() => expect(formattingOf(mounted)?.align).toBe("center"));
    });

    it("reports the quote command it runs", async () => {
      const mounted = mount();
      const handle = await handleOf(mounted);
      await act(async () => {
        handle.applyCommand("blockquote");
      });
      await vi.waitFor(() => expect(formattingOf(mounted)?.blockquote).toBe(true));
    });

    it("lets the keyboard handler claim a key before the editor", async () => {
      const handleKeyDown = vi.fn<(event: KeyboardEvent) => boolean>(() => true);
      mount({ handleKeyDown });
      const editable = await surface();
      editable.focus();
      fireEvent.keyDown(editable, { key: "l", metaKey: true, shiftKey: true });
      expect(handleKeyDown).toHaveBeenCalled();
    });

    it("leaves a key the handler declines to the editor", async () => {
      const handleKeyDown = vi.fn<(event: KeyboardEvent) => boolean>(() => false);
      mount({ handleKeyDown });
      const editable = await surface();
      editable.focus();
      expect(fireEvent.keyDown(editable, { key: "q" })).toBe(true);
      expect(handleKeyDown).toHaveBeenCalled();
    });

    it("works without a keyboard handler", async () => {
      mount({ handleKeyDown: undefined });
      const editable = await surface();
      expect(fireEvent.keyDown(editable, { key: "q" })).toBe(true);
    });

    it("shows the placeholder only while the document is empty", async () => {
      mount({ initialHtml: "<p></p>", placeholder: "Start writing" }, undefined);
      await surface();
      expect(screen.getByText("Start writing")).toBeInTheDocument();
    });

    it("keeps the placeholder off a document with text", async () => {
      mount({ initialHtml: "<p>filled</p>", placeholder: "Start writing" }, undefined);
      await surface();
      expect(screen.queryByText("Start writing")).not.toBeInTheDocument();
    });

    it("omits the placeholder when none is given", async () => {
      mount({ initialHtml: "<p></p>", placeholder: undefined }, undefined);
      const editable = await surface();
      expect(editable.parentElement?.textContent ?? "").not.toContain("Start writing");
    });
  },
  EDITOR_TEST_TIMEOUT
);
