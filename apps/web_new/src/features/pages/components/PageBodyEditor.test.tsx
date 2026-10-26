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
import { handlePagePaste, looksLikeMarkdown, PageBodyEditor } from "./PageBodyEditor.js";

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

/** Dispatch a clipboard paste with the flavours the editor reads. */
function paste(target: HTMLElement, data: { html?: string; text?: string }): Event {
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: {
      getData: (type: string) => (type === "text/html" ? (data.html ?? "") : (data.text ?? "")),
    },
  });
  target.dispatchEvent(event);
  return event;
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

    it("routes a stored-markup paste through the caller's rewrite", async () => {
      const onPasteHtml = vi.fn(async (html: string) => html.replace("foreign", "cloned"));
      mount({ onPasteHtml });
      const editable = await surface();
      paste(editable, { html: "<p>foreign</p>" });
      await vi.waitFor(() => expect(onPasteHtml).toHaveBeenCalledWith("<p>foreign</p>"));
      await vi.waitFor(() => expect(editable).toHaveTextContent("cloned"));
    });

    it("keeps the paste out of the body when the caller refuses it", async () => {
      const onPasteHtml = vi.fn(async () => null);
      const mounted = mount({ onPasteHtml });
      const editable = await surface();
      paste(editable, { html: "<p>foreign</p>" });
      await vi.waitFor(() => expect(onPasteHtml).toHaveBeenCalledTimes(1));
      expect(editable).toHaveTextContent("Stored words");
      expect(mounted.onChange).not.toHaveBeenCalled();
    });
  },
  EDITOR_TEST_TIMEOUT
);

describe("looksLikeMarkdown", () => {
  it("recognises every structural flavour", () => {
    for (const text of [
      "# heading",
      "- item",
      "1. item",
      "> quote",
      "```js",
      "**bold**",
      "__bold__",
      "[site](https://example.com)",
    ]) {
      expect(looksLikeMarkdown(text)).toBe(true);
    }
  });

  it("leaves prose alone", () => {
    expect(looksLikeMarkdown("plain words")).toBe(false);
  });
});

describe("handlePagePaste", () => {
  function event(data: { html?: string; text?: string }): ReturnType<typeof fakeEvent> {
    return fakeEvent(data);
  }

  it("claims stored markup and inserts only the rewritten form", async () => {
    const paste = vi.fn(async () => "<p>rewritten</p>");
    const insert = vi.fn();
    expect(handlePagePaste(event({ html: "<p>original</p>" }), paste, undefined, insert)).toBe(true);
    await vi.waitFor(() => expect(insert).toHaveBeenCalledWith("<p>rewritten</p>"));
  });

  it("claims stored markup even when the rewrite refuses it", async () => {
    const insert = vi.fn();
    expect(handlePagePaste(event({ html: "<p>original</p>" }), async () => null, undefined, insert)).toBe(true);
    await vi.waitFor(() => expect(insert).not.toHaveBeenCalled());
  });

  it("leaves stored markup to the editor without a rewrite", () => {
    const insert = vi.fn();
    expect(handlePagePaste(event({ html: "<p>original</p>" }), undefined, undefined, insert)).toBe(false);
    expect(insert).not.toHaveBeenCalled();
  });

  it("turns Markdown text into blocks and claims it", async () => {
    const insert = vi.fn();
    expect(handlePagePaste(event({ text: "# Pasted Heading\n\n- item one" }), undefined, undefined, insert)).toBe(true);
    await vi.waitFor(() => expect(insert).toHaveBeenCalled());
    expect(insert.mock.calls[0]?.[0]).toContain("<h1");
  });

  it("leaves plain text to the editor", () => {
    const insert = vi.fn();
    expect(handlePagePaste(event({ text: "plain words" }), undefined, undefined, insert)).toBe(false);
    expect(insert).not.toHaveBeenCalled();
  });

  it("claims Markdown that converts to nothing without inserting", async () => {
    const insert = vi.fn();
    expect(handlePagePaste(event({ text: "```" }), undefined, undefined, insert)).toBe(true);
    await vi.waitFor(() => expect(insert).not.toHaveBeenCalled());
  });

  it("ignores a paste with no clipboard data", () => {
    const insert = vi.fn();
    expect(
      handlePagePaste(
        { clipboardData: null, preventDefault: vi.fn(), stopPropagation: vi.fn() },
        undefined,
        undefined,
        insert
      )
    ).toBe(false);
  });

  it("claims only recognised entity text and inserts the resolved markup", async () => {
    const insert = vi.fn();
    const pasteText = vi.fn(async () => "<p><entity-reference-component></entity-reference-component></p>");
    const claims = vi.fn((text: string) => text === "WEB-42");
    expect(handlePagePaste(event({ text: "WEB-42" }), undefined, pasteText, insert, claims)).toBe(true);
    await vi.waitFor(() =>
      expect(insert).toHaveBeenCalledWith("<p><entity-reference-component></entity-reference-component></p>")
    );
    expect(pasteText).toHaveBeenCalledWith("WEB-42");
  });

  it("returns an unknown entity key as escaped, line-preserving text", async () => {
    const insert = vi.fn();
    const pasteText = vi.fn(async () => null);
    expect(handlePagePaste(event({ text: "WEB-99\n<b>" }), undefined, pasteText, insert, () => true)).toBe(true);
    await vi.waitFor(() => expect(insert).toHaveBeenCalledWith("<p>WEB-99</p><p>&lt;b&gt;</p>"));
  });

  it("leaves ordinary prose to the editor even when a text resolver exists", () => {
    const insert = vi.fn();
    const pasteText = vi.fn(async () => null);
    expect(handlePagePaste(event({ text: "just prose" }), undefined, pasteText, insert, () => false)).toBe(false);
    expect(pasteText).not.toHaveBeenCalled();
  });
});

function fakeEvent(data: { html?: string; text?: string }): {
  clipboardData: { getData: (type: string) => string };
  preventDefault: () => void;
  stopPropagation: () => void;
  defaultPrevented: boolean;
} {
  let prevented = false;
  return {
    clipboardData: {
      getData: (type: string) => (type === "text/html" ? (data.html ?? "") : (data.text ?? "")),
    },
    preventDefault: () => {
      prevented = true;
    },
    stopPropagation: vi.fn(),
    get defaultPrevented() {
      return prevented;
    },
  };
}
