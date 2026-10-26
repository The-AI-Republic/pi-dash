// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editable page canvas (parity/specs/pages.md §5, PAGE-026–032): the
// title's Enter reaches the body's start, the bar drives the editor, the
// editor reports its formatting state back, the cold-document skeleton
// holds the body area until the document is known, and the canvas hands
// its command handle to a caller outside it (the version restore).

import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { getPageBodyDraft, resetPageBodyDrafts, setPageBodyDraft } from "../lib/pageBodyDraft.js";
import { getPageBodyActions, resetPageBodyActions } from "../lib/pageBodyActions.js";
import { openPageImagePicker, PageEditorSurface, PAGE_BODY_LABEL, PAGE_BODY_PLACEHOLDER } from "./PageEditorSurface.js";

const EDITOR_TEST_TIMEOUT = 30_000;

const toaster = vi.hoisted(() => ({ notifyError: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyError: toaster.notifyError,
}));

const appClient = vi.hoisted(() => ({ getAppClient: vi.fn(() => ({ kind: "app-client" })) }));
vi.mock("../../../core/api/client.js", () => ({ getAppClient: appClient.getAppClient }));

const pageAssets = vi.hoisted(() => ({ uploadPageAsset: vi.fn(), duplicatePageAsset: vi.fn() }));
vi.mock("@pidash/api-client", async (original) => {
  const actual = await original<typeof import("@pidash/api-client")>();
  return {
    ...actual,
    uploadPageAsset: pageAssets.uploadPageAsset,
    duplicatePageAsset: pageAssets.duplicatePageAsset,
  };
});

function mount(props: Partial<React.ComponentProps<typeof PageEditorSurface>> = {}): ReturnType<typeof render> {
  return render(
    <PageEditorSurface
      pageId="page-1"
      workspaceSlug="acme"
      projectId="project-1"
      name="Launch notes"
      descriptionHtml="<p>Stored words</p>"
      logoProps={{}}
      onRename={vi.fn()}
      onMarkPick={vi.fn()}
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

/** Dispatch a stored-markup paste the editor's capture handler reads. */
function clipboardPaste(target: HTMLElement, html: string): void {
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: { getData: (type: string) => (type === "text/html" ? html : "") },
  });
  target.dispatchEvent(event);
}

afterEach(() => {
  resetPageBodyDrafts();
  resetPageBodyActions();
  toaster.notifyError.mockReset();
  appClient.getAppClient.mockClear();
  pageAssets.uploadPageAsset.mockReset();
  pageAssets.duplicatePageAsset.mockReset();
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

    it("marks the bar wide and stamps the stored type preferences", async () => {
      mount({ fullWidth: true, fontSize: "small-font", fontStyle: "serif" });
      await body();
      expect(screen.getByTestId("page-formatting-bar").querySelector(".page-toolbar-content")).toHaveClass(
        "wide-layout"
      );
      expect(screen.getByTestId("page-detail-title")).toHaveClass("text-size-small-font", "text-face-serif");
      expect(screen.getByTestId("page-canvas-header").closest(".frame-renderer") ?? document.body).toBeTruthy();
    });

    it("omits the wide marker without the full-width preference", async () => {
      mount();
      await body();
      expect(screen.getByTestId("page-formatting-bar").querySelector(".page-toolbar-content")).not.toHaveClass(
        "wide-layout"
      );
      expect(screen.getByTestId("page-detail-title")).toHaveClass("text-size-large-font", "text-face-sans-serif");
    });

    it("captures the standard save gesture without leaving the page", async () => {
      const onSave = vi.fn();
      mount({ onSave });
      const editable = await body();
      editable.focus();
      // The gesture is claimed: the browser's own save dialog never opens.
      expect(fireEvent.keyDown(editable, { key: "s", metaKey: true })).toBe(false);
      expect(onSave).toHaveBeenCalledTimes(1);
      expect(editable).toHaveTextContent("Stored words");
    });

    it("leaves the save gesture alone when it is not the standard chord", async () => {
      const onSave = vi.fn();
      mount({ onSave });
      const editable = await body();
      editable.focus();
      fireEvent.keyDown(editable, { key: "s", metaKey: true, shiftKey: true });
      fireEvent.keyDown(editable, { key: "s", altKey: true });
      expect(onSave).not.toHaveBeenCalled();
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

    it("drops the mark's pin once the title gains text", async () => {
      mount({ name: "" });
      await vi.waitFor(() =>
        expect(screen.getByTestId("page-mark-affordance")).toHaveAttribute("data-revealed", "true")
      );
      const user = userEvent.setup();
      const field = await screen.findByRole("textbox", { name: "Page title" }, { timeout: 10_000 });
      field.focus();
      await user.keyboard("Named");
      await vi.waitFor(() => expect(screen.getByTestId("page-mark-affordance")).not.toHaveAttribute("data-revealed"));
    });

    it("keeps the affordance pinned while a typed title is cleared", async () => {
      mount({ name: "Named" });
      const user = userEvent.setup();
      const field = await screen.findByRole("textbox", { name: "Page title" }, { timeout: 10_000 });
      field.focus();
      await user.clear(field);
      await vi.waitFor(() =>
        expect(screen.getByTestId("page-mark-affordance")).toHaveAttribute("data-revealed", "true")
      );
    });

    it("passes a picked mark to the page's save path", async () => {
      const onMarkPick = vi.fn();
      mount({ logoProps: { in_use: "emoji", emoji: { value: "1f680" } }, onMarkPick });
      fireEvent.click(screen.getByTestId("page-mark-trigger"));
      fireEvent.click(await screen.findByRole("button", { name: "Remove mark" }));
      expect(onMarkPick).toHaveBeenCalledWith({});
    });

    it("hands the live editor handle out and withdraws it on unmount", async () => {
      const onHandle = vi.fn();
      const view = mount({ onHandle });
      await body();
      const handle = onHandle.mock.calls.at(-1)?.[0];
      expect(handle).not.toBeNull();
      view.unmount();
      expect(onHandle.mock.calls.at(-1)).toEqual([null]);
    });

    it("writes a restored body into the editor and reports it as an edit", async () => {
      const onHandle = vi.fn();
      mount({ onHandle });
      const editable = await body();
      const handle = onHandle.mock.calls.at(-1)?.[0];
      expect(handle).not.toBeNull();
      act(() => {
        handle.replaceBody("<p>Snapshot words</p>");
      });
      await vi.waitFor(() => expect(editable).toHaveTextContent("Snapshot words"));
      expect(editable).not.toHaveTextContent("Stored words");
      await vi.waitFor(() => expect(getPageBodyDraft("page-1")).toContain("Snapshot words"));
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

    it("renders the table, image and colour entries after the core twelve", async () => {
      mount();
      await body();
      const bar = screen.getByRole("toolbar", { name: "Formatting" });
      expect(within(bar).getByRole("button", { name: "Table" })).toBeInTheDocument();
      expect(within(bar).getByRole("button", { name: "Image" })).toBeInTheDocument();
      expect(within(bar).getByRole("button", { name: "Color" })).toBeInTheDocument();
      expect(bar.querySelectorAll("button.grid.size-7")).toHaveLength(14);
    });

    it("drops a table at the caret from the toolbar tile", async () => {
      mount();
      const editable = await body();
      editable.focus();
      fireEvent.click(screen.getByRole("button", { name: "Table" }));
      await vi.waitFor(() => expect(editable.querySelector("table")).not.toBeNull());
      expect(editable.querySelector("table tr")?.children).toHaveLength(3);
    });

    it("uploads a picked picture through the injected path and inserts it inline", async () => {
      const uploadImage = vi.fn().mockResolvedValue({
        assetId: "asset-1",
        assetUrl: "/api/assets/v2/workspaces/acme/projects/project-1/asset-1/",
      });
      mount({ uploadImage });
      const editable = await body();
      const input = screen.getByTestId("page-toolbar-image-input");
      fireEvent.change(input, { target: { files: [new File(["x"], "shot.png", { type: "image/png" })] } });
      await vi.waitFor(() => expect(uploadImage).toHaveBeenCalledTimes(1));
      await vi.waitFor(() => {
        const image = editable.querySelector("image-component");
        expect(image).not.toBeNull();
        expect(image?.getAttribute("status")).toBe("uploaded");
      });
      expect(toaster.notifyError).not.toHaveBeenCalled();
    });

    it("falls back to the project asset URL when the upload names none", async () => {
      const uploadImage = vi.fn().mockResolvedValue({ assetId: "asset-9", assetUrl: "" });
      mount({ uploadImage });
      const editable = await body();
      fireEvent.change(screen.getByTestId("page-toolbar-image-input"), {
        target: { files: [new File(["x"], "shot.png", { type: "image/png" })] },
      });
      await vi.waitFor(() => {
        expect(editable.querySelector("image-component img")?.getAttribute("src")).toBe(
          "/api/assets/v2/workspaces/acme/projects/project-1/asset-9/"
        );
      });
    });

    it("reports a refused upload instead of inserting a broken picture", async () => {
      const uploadImage = vi.fn().mockRejectedValue(new Error("Too large."));
      mount({ uploadImage });
      const editable = await body();
      fireEvent.change(screen.getByTestId("page-toolbar-image-input"), {
        target: { files: [new File(["x"], "shot.png", { type: "image/png" })] },
      });
      await vi.waitFor(() => expect(toaster.notifyError).toHaveBeenCalledWith("Too large."));
      expect(editable.querySelector("image-component")).toBeNull();
    });

    it("refuses a second pick while the upload is in flight", async () => {
      let release: (value: { assetId: string; assetUrl: string }) => void = () => undefined;
      const uploadImage = vi.fn().mockReturnValue(
        new Promise((resolve) => {
          release = resolve;
        })
      );
      mount({ uploadImage });
      await body();
      fireEvent.change(screen.getByTestId("page-toolbar-image-input"), {
        target: { files: [new File(["x"], "shot.png", { type: "image/png" })] },
      });
      await vi.waitFor(() => expect(screen.getByRole("button", { name: "Image" })).toBeDisabled());
      release({ assetId: "a", assetUrl: "/api/assets/a/" });
      await vi.waitFor(() => expect(screen.getByRole("button", { name: "Image" })).toBeEnabled());
    });

    it("resolves a stored asset id for the editing surface", async () => {
      mount({ descriptionHtml: '<p><image-component src="asset-7"></image-component></p>' });
      const editable = await body();
      await vi.waitFor(() =>
        expect(editable.querySelector("image-component img")?.getAttribute("src")).toBe(
          "/api/assets/v2/workspaces/acme/projects/project-1/asset-7/"
        )
      );
    });

    it("uploads through the shared pipeline when no uploader is injected", async () => {
      pageAssets.uploadPageAsset.mockResolvedValue({ assetId: "asset-5", assetUrl: "/api/assets/asset-5/" });
      mount();
      const editable = await body();
      fireEvent.change(screen.getByTestId("page-toolbar-image-input"), {
        target: { files: [new File(["x"], "shot.png", { type: "image/png" })] },
      });
      await vi.waitFor(() => expect(pageAssets.uploadPageAsset).toHaveBeenCalledTimes(1));
      expect(appClient.getAppClient).toHaveBeenCalled();
      const call = pageAssets.uploadPageAsset.mock.calls[0] as unknown[];
      expect(call[1]).toBe("acme");
      expect(call[2]).toBe("project-1");
      expect(call[3]).toBe("page-1");
      await vi.waitFor(() => expect(editable.querySelector("image-component")).not.toBeNull());
    });

    it("applies and clears a letter colour from the bar's popover", async () => {
      mount();
      const editable = await body();
      const user = userEvent.setup();
      await user.dblClick(within(editable).getByText("Stored words"));
      // The mark must own a text selection before the colour command runs;
      // a click in the popover tears down a selection that has not landed.
      await vi.waitFor(() => expect(window.getSelection()?.toString().trim()).not.toBe(""));
      fireEvent.click(screen.getByRole("button", { name: "Color" }));
      fireEvent.click(await screen.findByRole("button", { name: "Letter color Pink" }));
      await vi.waitFor(() => expect(editable.querySelector('[data-text-color="pink"]')).not.toBeNull());

      await vi.waitFor(() => expect(screen.getByTestId("page-color-preview")).toBeInTheDocument());
      fireEvent.click(screen.getByRole("button", { name: "Color" }));
      fireEvent.click(await screen.findByRole("button", { name: "Clear letter color" }));
      await vi.waitFor(() => expect(editable.querySelector("[data-text-color]")).toBeNull());
    });

    it("opens the colour popover with eight swatches and a clear per row", async () => {
      mount();
      await body();
      fireEvent.click(screen.getByRole("button", { name: "Color" }));
      for (const kind of ["text", "background"] as const) {
        const row = await screen.findByTestId(`page-color-row-${kind}`);
        expect(within(row).getAllByRole("button")).toHaveLength(9);
      }
    });
    it("clones a foreign pasted asset before it lands", async () => {
      pageAssets.duplicatePageAsset.mockResolvedValue({ asset_id: "asset-cloned" });
      mount();
      const editable = await body();
      clipboardPaste(editable, '<image-component data-stored-src="asset-foreign"></image-component>');

      await vi.waitFor(() => expect(pageAssets.duplicatePageAsset).toHaveBeenCalledTimes(1));
      const call = pageAssets.duplicatePageAsset.mock.calls[0] as unknown[];
      expect(call[1]).toBe("acme");
      expect(call[2]).toBe("project-1");
      expect(call[3]).toBe("asset-foreign");
      expect(call[4]).toBe("page-1");
      await vi.waitFor(() =>
        expect(editable.querySelector("image-component")?.getAttribute("data-stored-src")).toBe("asset-cloned")
      );
    });

    it("opens the surface's own picker from another entry", () => {
      mount();
      const input = screen.getByTestId("page-toolbar-image-input") as HTMLInputElement;
      const click = vi.spyOn(input, "click").mockImplementation(() => undefined);
      openPageImagePicker();
      expect(click).toHaveBeenCalledTimes(1);
    });

    it("publishes the copy entry before the editor handle exists", async () => {
      mount();
      await vi.waitFor(() => expect(getPageBodyActions()).not.toBeNull());
      const writeText = vi.fn(async () => undefined);
      vi.stubGlobal("navigator", { clipboard: { writeText } });
      await getPageBodyActions()?.copyMarkdown();
      expect(writeText).toHaveBeenCalledWith("Stored words\n");
      vi.unstubAllGlobals();
    });

    it("reports a refused clone instead of pasting the foreign asset", async () => {
      pageAssets.duplicatePageAsset.mockRejectedValue(new Error("Clone refused."));
      mount();
      const editable = await body();
      clipboardPaste(editable, '<image-component data-stored-src="asset-foreign"></image-component>');
      await vi.waitFor(() => expect(toaster.notifyError).toHaveBeenCalledWith("Clone refused."));
      expect(editable.querySelector("image-component")).toBeNull();
    });
  },
  EDITOR_TEST_TIMEOUT
);
