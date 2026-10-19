// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { RichTextEditor } from "./RichTextEditor.js";

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
});
