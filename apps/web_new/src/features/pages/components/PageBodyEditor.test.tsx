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
import * as Y from "yjs";

import type { PageEditorHandle } from "../lib/pageEditorAdapter.js";
import type { PageFormattingState } from "../../../shared/editor/index.js";
import { handlePagePaste, looksLikeMarkdown, PageBodyEditor } from "./PageBodyEditor.js";

const LOAD_TIMEOUT = { timeout: 10_000 };
const EDITOR_TEST_TIMEOUT = 30_000;

interface Mounted {
  onChange: ReturnType<typeof vi.fn>;
  onFormattingChange: ReturnType<typeof vi.fn>;
  handles: PageEditorHandle[];
  rerender: (props: Partial<React.ComponentProps<typeof PageBodyEditor>>) => void;
}

function mount(
  props: Partial<React.ComponentProps<typeof PageBodyEditor>> = {},
  fallback: React.ReactNode = <p>read only body</p>
): Mounted {
  const onChange = vi.fn();
  const onFormattingChange = vi.fn<(state: PageFormattingState) => void>();
  const handles: PageEditorHandle[] = [];
  const element = (extra: Partial<React.ComponentProps<typeof PageBodyEditor>> = {}) => (
    <React.Suspense fallback={fallback}>
      <PageBodyEditor
        initialHtml="<p>Stored words</p>"
        onChange={onChange}
        onFormattingChange={onFormattingChange}
        onReady={(handle) => handles.push(handle)}
        label="Page body"
        {...props}
        {...extra}
      />
    </React.Suspense>
  );
  const view = render(element());
  return { onChange, onFormattingChange, handles, rerender: (extra) => view.rerender(element(extra)) };
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

    it("renders the body without an editable region when editing is off (PAGE-053)", async () => {
      mount({ editable: false });
      await vi.dynamicImportSettled();
      const body = await screen.findByLabelText("Page body", undefined, LOAD_TIMEOUT);
      expect(body).toHaveTextContent("Stored words");
      expect(body).not.toHaveAttribute("contenteditable", "true");
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

    it("mounts the standalone editor on an explicit null binding", async () => {
      mount({ collab: null });
      expect(await surface()).toHaveTextContent("Stored words");
    });
  },
  EDITOR_TEST_TIMEOUT
);

describe(
  "PageBodyEditor collaboration",
  () => {
    it("seeds the stored markup into an empty shared field", async () => {
      const doc = new Y.Doc();
      const mounted = mount({ collab: { doc }, seedDocument: true });
      expect(await surface()).toHaveTextContent("Stored words");
      await vi.waitFor(() => expect(doc.getXmlFragment("default").length).toBeGreaterThan(0));
      const peer = new Y.Doc();
      Y.applyUpdate(peer, Y.encodeStateAsUpdate(doc));
      expect(peer.getXmlFragment("default").toString()).toBe("<paragraph>Stored words</paragraph>");
      // The seed restores stored content, so it reports nothing to save.
      expect(mounted.onChange).not.toHaveBeenCalled();
    });

    it("waits for the server state before seeding", async () => {
      const doc = new Y.Doc();
      const mounted = mount({ collab: { doc } });
      const editable = await surface();
      expect(editable.textContent).toBe("");
      expect(doc.getXmlFragment("default").length).toBe(0);
      mounted.rerender({ seedDocument: true });
      await vi.waitFor(() => expect(editable).toHaveTextContent("Stored words"));
      expect(doc.getXmlFragment("default").length).toBeGreaterThan(0);
    });

    it("renders remote updates and reports the merged body", async () => {
      const doc = new Y.Doc();
      const mounted = mount({ collab: { doc }, seedDocument: true });
      await surface();
      await vi.waitFor(() => expect(doc.getXmlFragment("default").length).toBeGreaterThan(0));
      const peer = new Y.Doc();
      Y.applyUpdate(peer, Y.encodeStateAsUpdate(doc));
      const paragraph = peer.getXmlFragment("default").get(0) as Y.XmlElement;
      const text = paragraph.get(0) as Y.XmlText;
      await act(async () => {
        text.insert(text.length, " plus remote");
      });
      await act(async () => {
        Y.applyUpdate(doc, Y.encodeStateAsUpdate(peer));
      });
      const editable = await surface();
      await vi.waitFor(() => expect(editable).toHaveTextContent("plus remote"));
      await vi.waitFor(() => expect(mounted.onChange).toHaveBeenCalled());
      const last = mounted.onChange.mock.calls[mounted.onChange.mock.calls.length - 1]?.[0] as string;
      expect(last).toContain("Stored words plus remote");
    });

    it("converges two editors bound to one document", async () => {
      const doc = new Y.Doc();
      const collab = { doc };
      const first = mount({ collab, seedDocument: true, label: "First body" });
      await surface("First body");
      await vi.waitFor(() => expect(doc.getXmlFragment("default").length).toBeGreaterThan(0));
      const second = mount({ collab, seedDocument: true, label: "Second body" });
      const two = await surface("Second body");
      expect(two).toHaveTextContent("Stored words");
      const one = await handleOf(first);
      await act(async () => {
        one.applyBlockKind("heading2");
      });
      await vi.waitFor(() => expect(second.onChange).toHaveBeenCalled());
      const seen = second.onChange.mock.calls[second.onChange.mock.calls.length - 1]?.[0] as string;
      expect(seen).toContain("<h2");
      expect(two.textContent).toContain("Stored words");
      const other = await handleOf(second);
      await act(async () => {
        other.applyBlockKind("paragraph");
      });
      await vi.waitFor(() => {
        const calls = first.onChange.mock.calls;
        const latest = calls[calls.length - 1]?.[0] as string;
        expect(latest).toContain("<p>");
      });
    });

    it("converges two documents through exchanged updates", async () => {
      const docA = new Y.Doc();
      const first = mount({ collab: { doc: docA }, seedDocument: true, label: "Peer A" });
      await surface("Peer A");
      await vi.waitFor(() => expect(docA.getXmlFragment("default").length).toBeGreaterThan(0));
      const docB = new Y.Doc();
      Y.applyUpdate(docB, Y.encodeStateAsUpdate(docA));
      const second = mount({
        initialHtml: "<p>Stale seed</p>",
        collab: { doc: docB },
        seedDocument: true,
        label: "Peer B",
      });
      const editableB = await surface("Peer B");
      expect(editableB).toHaveTextContent("Stored words");
      expect(editableB).not.toHaveTextContent("Stale seed");
      const handleA = await handleOf(first);
      await act(async () => {
        handleA.applyBlockKind("heading2");
      });
      await act(async () => {
        Y.applyUpdate(docB, Y.encodeStateAsUpdate(docA));
      });
      await vi.waitFor(() => expect(second.onChange).toHaveBeenCalled());
      const seenByB = second.onChange.mock.calls[second.onChange.mock.calls.length - 1]?.[0] as string;
      expect(seenByB).toContain("<h2");
      const handleB = await handleOf(second);
      await act(async () => {
        handleB.applyBlockKind("paragraph");
      });
      await act(async () => {
        Y.applyUpdate(docA, Y.encodeStateAsUpdate(docB));
      });
      await vi.waitFor(() => {
        const calls = first.onChange.mock.calls;
        const latest = calls[calls.length - 1]?.[0] as string;
        expect(latest).toContain("<p>");
        expect(latest).toContain("Stored words");
      });
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

const HEAL_IDENTIFIERS = {
  id: "emb-oracle-1",
  entity_identifier: "N316-ISS-1",
  project_identifier: "N316-PROJ",
  workspace_identifier: "acme",
  entity_name: "Embedded issue",
};

/** The PAGE-040 stored shape: prose plus an embed the markup lost. */
function healSnapshot(): Record<string, unknown> {
  return {
    type: "doc",
    content: [
      { type: "paragraph", content: [{ type: "text", text: "before embed" }] },
      { type: "issue-embed-component", attrs: HEAL_IDENTIFIERS },
    ],
  };
}

/** The community upgrade card a restored embed paints. */
async function upgradeCard(editable: HTMLElement): Promise<HTMLElement> {
  return vi.waitFor(
    () => {
      const card = editable.querySelector(
        'issue-embed-component [data-testid="page-embed-upgrade"]'
      ) as HTMLElement | null;
      expect(card).not.toBeNull();
      return card as HTMLElement;
    },
    { timeout: 10_000 }
  );
}

describe(
  "PageBodyEditor snapshot heal",
  () => {
    it("restores a JSON-only embed as the community upgrade card", async () => {
      const mounted = mount({ initialHtml: "<p>before embed</p>", descriptionJson: healSnapshot() });
      const editable = await surface();
      const card = await upgradeCard(editable);
      expect(card.textContent).toContain("Pro feature");
      const link = card.querySelector("a");
      expect(link?.getAttribute("href")).toBe("https://airepublic.com/pricing");
      expect(editable).toHaveTextContent("before embed");
      await vi.waitFor(() =>
        expect(mounted.onChange.mock.calls.some((call) => String(call[0]).includes("<issue-embed-component"))).toBe(
          true
        )
      );
    });

    it("heals once per mount, never duplicating the restored embed", async () => {
      const mounted = mount({ initialHtml: "<p>before embed</p>", descriptionJson: healSnapshot() });
      const editable = await surface();
      await upgradeCard(editable);
      // A fresh snapshot object re-runs the effect; the mount guard holds.
      // The rerender flushes effects synchronously, so any duplicate would
      // already be in the document.
      mounted.rerender({ descriptionJson: healSnapshot() });
      expect(editable.querySelectorAll("issue-embed-component")).toHaveLength(1);
    });

    it("leaves a body whose markup already carries the embed alone", async () => {
      mount({
        initialHtml:
          '<p>before embed</p><issue-embed-component id="emb-oracle-1" entity_identifier="N316-ISS-1" project_identifier="N316-PROJ" workspace_identifier="acme" entity_name="Embedded issue"></issue-embed-component>',
        descriptionJson: healSnapshot(),
      });
      const editable = await surface();
      await upgradeCard(editable);
      expect(editable.querySelectorAll("issue-embed-component")).toHaveLength(1);
    });

    it("waits for the server state before healing a shared document", async () => {
      const doc = new Y.Doc();
      const mounted = mount({
        initialHtml: "<p>before embed</p>",
        collab: { doc },
        descriptionJson: healSnapshot(),
      });
      const editable = await surface();
      expect(editable.querySelector("issue-embed-component")).toBeNull();
      mounted.rerender({ seedDocument: true });
      const card = await upgradeCard(editable);
      expect(card.textContent).toContain("Pro feature");
    });

    it("heals after seeding an empty shared field", async () => {
      const doc = new Y.Doc();
      mount({
        initialHtml: "<p>before embed</p>",
        collab: { doc },
        seedDocument: true,
        descriptionJson: healSnapshot(),
      });
      const editable = await surface();
      // The seed restores the prose first; the heal's embed survives it.
      await vi.waitFor(() => expect(editable).toHaveTextContent("before embed"));
      const card = await upgradeCard(editable);
      expect(editable.querySelectorAll("issue-embed-component")).toHaveLength(1);
      expect(card.textContent).toContain("Pro feature");
    });
  },
  EDITOR_TEST_TIMEOUT
);
