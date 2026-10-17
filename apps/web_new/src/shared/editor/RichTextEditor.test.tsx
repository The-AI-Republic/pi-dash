// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { RichTextEditor } from "./RichTextEditor.js";

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
  const editable = await screen.findByLabelText(label);
  expect(editable).toHaveAttribute("contenteditable", "true");
  return editable;
}

describe("RichTextEditor", () => {
  it("shows a fallback while the editor chunk loads, then the content", async () => {
    render(
      <React.Suspense fallback={<p>loading the editor</p>}>
        <RichTextEditor initialHtml="<p>Stored words</p>" label="Body" onChange={() => {}} />
      </React.Suspense>
    );
    expect(await screen.findByText("Stored words")).toBeInTheDocument();
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
    const editable = await screen.findByLabelText("Body");
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
