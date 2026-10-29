// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CommentRichTextEditor, IssueDescriptionEditor, RichTextEditor } from "./RichTextEditor.js";

// The editor arrives through a dynamic import (tipTap, its extensions).
// A full-suite run can keep that import in flight for seconds, so the
// content and surface waits get an explicit budget; the default 1 s
// find budget is what made the fallback test racy under load.
const LOAD_TIMEOUT = { timeout: 10_000 };

// A find budget above vitest's 5 s default test timeout is inert: the
// test dies before the find can fail with its own message. Every test
// here waits on the same lazily imported editor chunk, so the whole
// suite gets a backstop above LOAD_TIMEOUT.
const EDITOR_TEST_TIMEOUT = 30_000;

function renderEditor(props: Partial<React.ComponentProps<typeof RichTextEditor>> = {}) {
  const onChange = vi.fn();
  render(
    <React.Suspense fallback={<p>loading the editor</p>}>
      <RichTextEditor initialHtml="<p></p>" label="Body" onChange={onChange} {...props} />
    </React.Suspense>
  );
  return { onChange };
}

async function editorSurface(label: string): Promise<HTMLElement> {
  await vi.dynamicImportSettled();
  const editable = await screen.findByLabelText(label, undefined, LOAD_TIMEOUT);
  expect(editable).toHaveAttribute("contenteditable", "true");
  return editable;
}

describe("RichTextEditor", { timeout: EDITOR_TEST_TIMEOUT }, () => {
  it("shows a fallback while the editor chunk loads, then the content", async () => {
    render(
      <React.Suspense fallback={<p>loading the editor</p>}>
        <RichTextEditor initialHtml="<p>Stored words</p>" label="Body" onChange={() => {}} />
      </React.Suspense>
    );
    await vi.dynamicImportSettled();
    expect(await screen.findByText("Stored words", undefined, LOAD_TIMEOUT)).toBeInTheDocument();
  });

  it("keeps the editing surface mounted once the loaded content appears", async () => {
    // The lazy chunk resolves, then the editor instance settles. A frame
    // that paints content from one instance and swaps in another would
    // replace the editable node and drop a caret (or element handle)
    // taken on the first commit; the surface must be final once visible.
    renderEditor({ initialHtml: "<p>steady words</p>" });
    await vi.dynamicImportSettled();
    const [painted] = await screen.findAllByText("steady words", undefined, LOAD_TIMEOUT);
    // The editor manager schedules its one-tick reconciliation with a
    // 1 ms timer; these later timers run after it, so by the first the
    // instance has settled whether or not the machine is busy.
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 5));
    await new Promise((resolve) => setTimeout(resolve, 5));
    const editable = screen.getByLabelText("Body");
    expect(editable).toHaveTextContent("steady words");
    // The painted paragraph is the one still on screen, not a discarded
    // first instance replaced by the settled one.
    expect(painted?.isConnected).toBe(true);
    expect(editable).toContainElement(painted ?? null);
  });

  it("opens with the stored HTML and reports edits back as HTML", async () => {
    const user = userEvent.setup();
    const { onChange } = renderEditor({ initialHtml: "<p>alpha</p>" });
    const editable = await editorSurface("Body");
    expect(editable).toHaveTextContent("alpha");
    // Focus directly: a pointer click would route through document
    // geometry (elementFromPoint), which jsdom does not implement.
    editable.focus();
    await user.keyboard(" beta");
    await vi.waitFor(() => expect(onChange).toHaveBeenCalled());
    const last = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as string;
    expect(last).toContain("alpha");
    expect(last).toContain("beta");
  });

  it("shows the placeholder only while the document holds no text", async () => {
    const emptyMount = render(
      <React.Suspense fallback={<p>loading</p>}>
        <RichTextEditor initialHtml="<p></p>" label="Body" placeholder="Write something" onChange={() => {}} />
      </React.Suspense>
    );
    await editorSurface("Body");
    expect(screen.getByText("Write something")).toBeInTheDocument();
    emptyMount.unmount();
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <RichTextEditor initialHtml="<p>filled</p>" label="Notes" placeholder="Write something" onChange={() => {}} />
      </React.Suspense>
    );
    await editorSurface("Notes");
    expect(screen.queryByText("Write something")).not.toBeInTheDocument();
  });

  it("omits the placeholder when none is given", async () => {
    renderEditor({ initialHtml: "<p></p>" });
    const editable = await editorSurface("Body");
    expect(editable.parentElement?.textContent ?? "").not.toContain("Write");
  });

  it("renders read-only when editing is off", async () => {
    renderEditor({ initialHtml: "<p>locked</p>", editable: false });
    const editable = await screen.findByLabelText("Body", undefined, LOAD_TIMEOUT);
    expect(editable).toHaveAttribute("contenteditable", "false");
    expect(editable).toHaveTextContent("locked");
  });

  it("focuses the document when asked", async () => {
    renderEditor({ initialHtml: "<p>focus me</p>", autoFocus: true });
    const editable = await editorSurface("Body");
    await vi.waitFor(() => expect(document.activeElement).toBe(editable));
  });

  it("carries a caller class on its frame", async () => {
    renderEditor({ initialHtml: "<p>styled</p>", className: "mt-4" });
    const editable = await editorSurface("Body");
    expect(editable.closest("div.relative")).toHaveClass("mt-4");
  });

  it("hands the comment editor a handle that inserts an image", async () => {
    const handles: import("./RichTextEditor.js").RichTextEditorHandle[] = [];
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <CommentRichTextEditor
          initialHtml="<p>with image</p>"
          label="Comment"
          onChange={() => {}}
          onReady={(ready) => {
            handles.push(ready);
          }}
        />
      </React.Suspense>
    );
    await editorSurface("Comment");
    await vi.waitFor(() => expect(handles.length).toBeGreaterThan(0));
    // The newest handle is the live editor; an earlier one may be stale.
    handles[handles.length - 1]?.insertImage("https://cdn.test/shot.png");
    await vi.waitFor(() => expect(document.querySelector("img")).not.toBeNull());
    expect(document.querySelector("img")?.getAttribute("src")).toBe("https://cdn.test/shot.png");
  });

  it("hands out the live document for the key-time Enter decision (ISS-197)", async () => {
    const handles: import("./RichTextEditor.js").RichTextEditorHandle[] = [];
    const onChange = vi.fn();
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <CommentRichTextEditor
          initialHtml="<p></p>"
          label="CommentLive"
          onChange={onChange}
          onReady={(ready) => {
            handles.push(ready);
          }}
        />
      </React.Suspense>
    );
    const editable = await editorSurface("CommentLive");
    await vi.waitFor(() => expect(handles.length).toBeGreaterThan(0));
    const handle = handles[handles.length - 1];
    expect(handle?.getHtml()).toBe("<p></p>");
    // A word typed into the surface is readable from the handle right
    // away, before any re-render publishes it to the caller.
    editable.focus();
    await userEvent.setup().keyboard("Fresh");
    expect(onChange).toHaveBeenLastCalledWith("<p>Fresh</p>");
    expect(handle?.getHtml()).toBe("<p>Fresh</p>");
  });

  it("stores an uploaded comment picture as image-component with its status (CMT-009)", async () => {
    const onChange = vi.fn();
    const handles: import("./RichTextEditor.js").RichTextEditorHandle[] = [];
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <CommentRichTextEditor
          initialHtml="<p>with image</p>"
          label="CommentStored"
          onChange={onChange}
          onReady={(ready) => {
            handles.push(ready);
          }}
        />
      </React.Suspense>
    );
    await editorSurface("CommentStored");
    await vi.waitFor(() => expect(handles.length).toBeGreaterThan(0));
    const handle = handles[handles.length - 1];
    if (handle === undefined) throw new Error("the editor announced no handle");
    handle.insertImage("/api/assets/v2/workspaces/acme/projects/p-1/asset-9/", {
      storedSrc: "asset-9",
      status: "uploaded",
    });
    await vi.waitFor(() => expect(onChange).toHaveBeenCalled());
    const last = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as string;
    expect(last).toContain("<image-component");
    expect(last).toContain('src="asset-9"');
    expect(last).toContain('status="uploaded"');
    expect(last).not.toContain("data-stored-src");
  });

  it("stores a picked person in the comment editor (CMT-019)", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const handles: import("./RichTextEditor.js").RichTextEditorHandle[] = [];
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <CommentRichTextEditor
          initialHtml="<p></p>"
          label="CommentMention"
          onChange={onChange}
          onReady={(ready) => {
            handles.push(ready);
          }}
        />
      </React.Suspense>
    );
    const editable = await editorSurface("CommentMention");
    editable.focus();
    await user.keyboard("Hello @ad");
    const handle = handles[handles.length - 1];
    if (handle === undefined) throw new Error("the editor announced no handle");
    handle.insertMention({ id: "u-1", name: "Ada" });
    await vi.waitFor(() => expect(editable.querySelector("mention-component")).not.toBeNull());
    expect(editable.innerHTML).not.toContain("@ad");
    expect(editable.querySelector(".mention-chip")?.textContent).toBe("@Ada");
    await vi.waitFor(() => expect(onChange).toHaveBeenCalled());
    const last = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as string;
    expect(last).toContain("mention-component");
    expect(last).toContain('entity_identifier="u-1"');
    expect(last).toContain('data-name="Ada"');
  });

  it("keeps pasted formatting in the comment editor (CMT-001)", async () => {
    const onChange = vi.fn();
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <CommentRichTextEditor initialHtml="<p>typed marker</p>" label="CommentPaste" onChange={onChange} />
      </React.Suspense>
    );
    const editable = await editorSurface("CommentPaste");
    editable.focus();
    const paste = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(paste, "clipboardData", {
      // A real clipboard answers "" for types it does not carry; the
      // code-block plugin JSON-parses its own type, so anything else
      // there would throw before the paste lands.
      value: {
        getData: (type: string) => {
          if (type === "text/html") return "<p>pasted <strong>bold</strong> fragment</p>";
          if (type === "text/plain") return "pasted bold fragment";
          return "";
        },
      },
    });
    editable.dispatchEvent(paste);
    await vi.waitFor(() => expect(onChange).toHaveBeenCalled());
    const last = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as string;
    expect(last).toContain("typed marker");
    expect(last).toContain("<strong>bold</strong>");
  });

  it("reports the @-query under the caret and its absence", async () => {
    const user = userEvent.setup();
    const queries: Array<string | null> = [];
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <IssueDescriptionEditor
          initialHtml="<p></p>"
          label="Description"
          onChange={() => {}}
          onMentionQuery={(query) => queries.push(query)}
        />
      </React.Suspense>
    );
    const editable = await editorSurface("Description");
    editable.focus();
    await user.keyboard("Hello @ad");
    await vi.waitFor(() => expect(queries[queries.length - 1]).toBe("ad"));
    await user.keyboard(" ");
    await vi.waitFor(() => expect(queries[queries.length - 1]).toBeNull());
  });

  it("replaces the typed @query when a person is picked", async () => {
    const user = userEvent.setup();
    const handles: import("./RichTextEditor.js").RichTextEditorHandle[] = [];
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <IssueDescriptionEditor
          initialHtml="<p></p>"
          label="Description"
          onChange={() => {}}
          onReady={(ready) => {
            handles.push(ready);
          }}
        />
      </React.Suspense>
    );
    const editable = await editorSurface("Description");
    editable.focus();
    await user.keyboard("Hello @ad");
    const handle = handles[handles.length - 1];
    if (handle === undefined) throw new Error("the editor announced no handle");
    expect(handle.textBeforeCaret()).toContain("@ad");
    handle.insertMention({ id: "u-1", name: "Ada" });
    await vi.waitFor(() => expect(editable.querySelector("mention-component")).not.toBeNull());
    // The pick consumes the query text, so only the mention remains, and
    // the editing surface shows the person's name rather than an empty
    // element (ISS-127).
    expect(editable.innerHTML).not.toContain("@ad");
    expect(editable.textContent).toBe("Hello @Ada");
    expect(editable.querySelector("mention-component")?.getAttribute("id")).toBe("u-1");
    expect(editable.querySelector(".mention-chip")?.textContent).toBe("@Ada");
  });

  it("inserts HTML where the caret is, not at the end", async () => {
    const handles: import("./RichTextEditor.js").RichTextEditorHandle[] = [];
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <IssueDescriptionEditor
          initialHtml="<p>alpha omega</p>"
          label="Description"
          onChange={() => {}}
          onReady={(ready) => {
            handles.push(ready);
          }}
        />
      </React.Suspense>
    );
    const editable = await editorSurface("Description");
    const text = editable.querySelector("p")?.firstChild;
    if (text === null || text === undefined) throw new Error("the paragraph has no text node");
    // jsdom has no layout or native caret movement; put the DOM caret
    // after "alpha" and let ProseMirror read it as its selection.
    const caret = document.createRange();
    caret.setStart(text, "alpha".length);
    caret.collapse(true);
    editable.focus();
    const selection = document.getSelection();
    if (selection === null) throw new Error("jsdom exposes no selection");
    selection.removeAllRanges();
    selection.addRange(caret);
    fireEvent(document, new Event("selectionchange"));
    const handle = handles[handles.length - 1];
    if (handle === undefined) throw new Error("the editor announced no handle");
    expect(handle.textBeforeCaret()).toBe("alpha");
    handle.insertHtml("<strong>assistant</strong>");
    await vi.waitFor(() => expect(editable.querySelector("strong")).not.toBeNull());
    expect(editable.textContent).toBe("alphaassistant omega");
  });

  it("adds a mention at the caret when no @query is open", async () => {
    const handles: import("./RichTextEditor.js").RichTextEditorHandle[] = [];
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <IssueDescriptionEditor
          initialHtml="<p>Thanks</p>"
          label="Description"
          onChange={() => {}}
          onReady={(ready) => {
            handles.push(ready);
          }}
        />
      </React.Suspense>
    );
    const editable = await editorSurface("Description");
    const text = editable.querySelector("p")?.firstChild;
    if (text === null || text === undefined) throw new Error("the paragraph has no text node");
    const caret = document.createRange();
    caret.setStart(text, "Thanks".length);
    caret.collapse(true);
    editable.focus();
    const selection = document.getSelection();
    if (selection === null) throw new Error("jsdom exposes no selection");
    selection.removeAllRanges();
    selection.addRange(caret);
    fireEvent(document, new Event("selectionchange"));
    const handle = handles[handles.length - 1];
    if (handle === undefined) throw new Error("the editor announced no handle");
    expect(handle.textBeforeCaret()).toBe("Thanks");
    handle.insertMention({ id: "u-2", name: "Grace" });
    await vi.waitFor(() => expect(editable.querySelector("mention-component")).not.toBeNull());
    expect(editable.textContent).toBe("Thanks@Grace");
    expect(editable.querySelector("mention-component")?.getAttribute("entity_identifier")).toBe("u-2");
  });

  it("ignores an empty HTML insert", async () => {
    const onChange = vi.fn();
    const handles: import("./RichTextEditor.js").RichTextEditorHandle[] = [];
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <IssueDescriptionEditor
          initialHtml="<p>steady</p>"
          label="Description"
          onChange={onChange}
          onReady={(ready) => {
            handles.push(ready);
          }}
        />
      </React.Suspense>
    );
    const editable = await editorSurface("Description");
    onChange.mockClear();
    const handle = handles[handles.length - 1];
    if (handle === undefined) throw new Error("the editor announced no handle");
    handle.insertHtml("");
    await new Promise((resolve) => setTimeout(resolve, 5));
    expect(onChange).not.toHaveBeenCalled();
    expect(editable.textContent).toBe("steady");
  });

  it("opens a legacy image-component with its picture and keeps the stored asset id on save (ISS-127)", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const stored =
      '<p>hi</p><image-component src="35e8b958-6ee5-43ce-ae56-fb0e776f421e" status="uploaded" width="200" height="auto" aspectRatio="2" aspectratio="2" alignment="left"></image-component><p>there</p>';
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <IssueDescriptionEditor
          initialHtml={stored}
          label="Description"
          onChange={onChange}
          resolveAsset={(src) => `/api/assets/v2/workspaces/acme/projects/p-1/${src}/`}
        />
      </React.Suspense>
    );
    const editable = await editorSurface("Description");
    const image = editable.querySelector("image-component");
    expect(image?.querySelector("img")?.getAttribute("src")).toBe(
      "/api/assets/v2/workspaces/acme/projects/p-1/35e8b958-6ee5-43ce-ae56-fb0e776f421e/"
    );
    expect(image?.getAttribute("data-stored-src")).toBe("35e8b958-6ee5-43ce-ae56-fb0e776f421e");
    // An edit anywhere in the body must not rewrite the stored node into
    // an <img> pointing at the asset id.
    editable.focus();
    await user.keyboard("!");
    await vi.waitFor(() => expect(onChange).toHaveBeenCalled());
    const last = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as string;
    expect(last).toContain("<image-component");
    expect(last).toContain('src="35e8b958-6ee5-43ce-ae56-fb0e776f421e"');
    expect(last).toContain('status="uploaded"');
    expect(last).toContain('width="200"');
    // The old editor's measured ratio and the remaining stored
    // attributes survive too: a save must not narrow the element
    // (ISS-127).
    // HTML attribute names are case-insensitive, so the stored
    // `aspectRatio` comes back spelled in lowercase with its value kept.
    expect(last).toContain('aspectratio="2"');
    expect(last).toContain('height="auto"');
    expect(last).toContain('alignment="left"');
    expect(last).not.toContain("data-stored-src");
    expect(last).not.toContain("<img");
  });

  it("serializes a new picture as a plain img without a stored attribute", async () => {
    const onChange = vi.fn();
    const handles: import("./RichTextEditor.js").RichTextEditorHandle[] = [];
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <IssueDescriptionEditor
          initialHtml="<p></p>"
          label="Description"
          onChange={onChange}
          onReady={(ready) => {
            handles.push(ready);
          }}
        />
      </React.Suspense>
    );
    await editorSurface("Description");
    await vi.waitFor(() => expect(handles.length).toBeGreaterThan(0));
    handles[handles.length - 1]?.insertImage("https://cdn.test/shot.png");
    await vi.waitFor(() => expect(onChange).toHaveBeenCalled());
    const last = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as string;
    // A body written today carries a URL, not an asset id, so it saves as
    // an ordinary image with no legacy attribute (ISS-127).
    expect(last).toContain('<img src="https://cdn.test/shot.png"');
    expect(last).not.toContain("data-stored-src");
    expect(last).not.toContain("<image-component");
  });

  it("routes keys through the caller's guard first (CMT-011)", async () => {
    const seen: KeyboardEvent[] = [];
    let swallow = true;
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <CommentRichTextEditor
          initialHtml="<p>guarded</p>"
          label="CommentGuard"
          onChange={() => {}}
          handleKeyDown={(event) => {
            seen.push(event);
            return swallow;
          }}
        />
      </React.Suspense>
    );
    const editable = await editorSurface("CommentGuard");
    const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    editable.dispatchEvent(enter);
    expect(seen).toHaveLength(1);
    expect(seen[0]).toBe(enter);
    expect(enter.defaultPrevented).toBe(true);
    // A released key bubbles on unconsumed.
    swallow = false;
    const plain = new KeyboardEvent("keydown", { key: "a", bubbles: true, cancelable: true });
    editable.dispatchEvent(plain);
    expect(seen).toHaveLength(2);
    expect(plain.defaultPrevented).toBe(false);
  });

  it("paints a stored image without a source with an empty address", async () => {
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <IssueDescriptionEditor
          initialHtml='<image-component status="uploaded"></image-component>'
          label="DescriptionSrcless"
          onChange={() => {}}
        />
      </React.Suspense>
    );
    const editable = await editorSurface("DescriptionSrcless");
    expect(editable.querySelector("image-component img")?.getAttribute("src")).toBe("");
  });

  it("mounts without a ready handler", async () => {
    render(
      <React.Suspense fallback={<p>loading</p>}>
        <CommentRichTextEditor initialHtml="<p>no handle</p>" label="Plain" onChange={() => {}} />
      </React.Suspense>
    );
    expect(await editorSurface("Plain")).toHaveTextContent("no handle");
  });
});
