// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page interaction layer (parity/specs/pages.md §6.1–§6.4, PAGE-037–044):
// trigger detection, catalogue forwarding, mention search and selection
// commands all cross the same handle seam.

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { PageEditorHandle } from "../lib/pageEditorAdapter.js";
import { EMPTY_FORMATTING_STATE } from "../../../shared/editor/pageFormatting.js";
import { PageBodyInteractionLayer } from "./PageBodyInteractionLayer.js";

const api = vi.hoisted(() => ({ searchMentionPeople: vi.fn(), searchEntityPreviews: vi.fn() }));
vi.mock("@pidash/api-client", async (original) => {
  const actual = await original<typeof import("@pidash/api-client")>();
  return {
    ...actual,
    searchMentionPeople: api.searchMentionPeople,
    searchEntityPreviews: api.searchEntityPreviews,
  };
});
vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

const assistantTask = vi.hoisted(() => ({
  mutateAsync: vi.fn(async () => ({ text: "done", html: "<p>done</p>" })),
  isPending: false,
}));
vi.mock("../api/assistantMutations.js", () => ({ usePageAssistantAnswer: () => assistantTask }));

const edition = vi.hoisted(() => ({ slots: {} as Record<string, unknown> }));
vi.mock("@pidash/edition", () => ({ edition }));

function handle(overrides: Partial<PageEditorHandle> = {}): PageEditorHandle {
  return {
    applyCommand: vi.fn(),
    applyBlockKind: vi.fn(),
    insertTable: vi.fn(),
    applyColor: vi.fn(),
    clearColor: vi.fn(),
    insertImage: vi.fn(),
    applyCatalogBlock: vi.fn(),
    insertEmoji: vi.fn(),
    insertMention: vi.fn(),
    insertEmbed: vi.fn(),
    insertEntityReference: vi.fn(),
    insertText: vi.fn(),
    duplicateBlock: vi.fn(),
    deleteBlock: vi.fn(),
    moveBlock: vi.fn(),
    blockAtPoint: vi.fn(() => null),
    caretAnchor: vi.fn(() => null),
    setLink: vi.fn(),
    unsetLink: vi.fn(),
    html: vi.fn(() => "<p>html</p>"),
    linkHref: vi.fn(() => null),
    hasSelection: vi.fn(() => true),
    selectionRange: vi.fn(() => null),
    caretPosition: vi.fn(() => null),
    insertAt: vi.fn(),
    textBeforeCaret: vi.fn(() => ""),
    deleteBeforeCaret: vi.fn(),
    selectedText: vi.fn(() => ""),
    focusEnd: vi.fn(),
    focusStart: vi.fn(),
    readState: vi.fn(() => EMPTY_FORMATTING_STATE),
    replaceBody: vi.fn(() => true),
    ...overrides,
  };
}

function mount(overrides: Partial<React.ComponentProps<typeof PageBodyInteractionLayer>> = {}) {
  const page = handle();
  const onDocumentChange = vi.fn();
  const onPickImage = vi.fn();
  const view = render(
    <PageBodyInteractionLayer
      workspaceSlug="acme"
      projectId="project-1"
      getHandle={() => page}
      html="<p>html</p>"
      onPickImage={onPickImage}
      onDocumentChange={onDocumentChange}
      assistantReady={false}
      {...overrides}
    />
  );
  return { ...view, page, onDocumentChange, onPickImage };
}

afterEach(() => {
  api.searchMentionPeople.mockReset();
  api.searchEntityPreviews.mockReset();
  assistantTask.mutateAsync.mockClear();
  edition.slots = {};
});

describe("PageBodyInteractionLayer", () => {
  it("opens the palette for a slash query and forwards a structural entry", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/head");
    const { onDocumentChange } = mount({ getHandle: () => page });
    expect(screen.getByRole("button", { name: "Heading 2" })).toBeInTheDocument();
    fireEvent.mouseDown(screen.getByRole("button", { name: "Heading 2" }));
    expect(page.deleteBeforeCaret).toHaveBeenCalledWith(5);
    expect(page.applyCatalogBlock).toHaveBeenCalledWith("heading2");
    expect(onDocumentChange).toHaveBeenCalledWith("<p>html</p>");
  });

  it("routes an Image entry to the file picker and an Emoji entry to the picker", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/im");
    const { onPickImage } = mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Image" }));
    expect(onPickImage).toHaveBeenCalledTimes(1);
  });

  it("opens the work-item picker from /embed and inserts the chosen embed", async () => {
    api.searchEntityPreviews.mockResolvedValue({
      issue: [
        {
          name: "Fix login",
          id: "11111111-1111-4111-8111-111111111111",
          sequence_id: 42,
          project__identifier: "WEB",
          project_id: "22222222-2222-4222-8222-222222222222",
          priority: "medium",
          state_id: "33333333-3333-4333-8333-333333333333",
          type_id: null,
        },
      ],
    });
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/embed");
    const { onDocumentChange } = mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Embed work item" }));
    expect(page.deleteBeforeCaret).toHaveBeenCalledWith(6);
    const row = await screen.findByRole("button", { name: /WEB-42 Fix login/ });
    fireEvent.mouseDown(row);
    expect(page.insertEmbed).toHaveBeenCalledWith({
      id: "11111111-1111-4111-8111-111111111111",
      entity_identifier: "11111111-1111-4111-8111-111111111111",
      project_identifier: "WEB",
      workspace_identifier: "acme",
      entity_name: "Fix login",
    });
    expect(onDocumentChange).toHaveBeenCalledWith("<p>html</p>");
  });

  it("opens the reference picker from /entity and inserts a normalised reference", async () => {
    api.searchEntityPreviews.mockResolvedValue({
      issue: [
        {
          name: "Fix login",
          id: "11111111-1111-4111-8111-111111111111",
          sequence_id: 42,
          project__identifier: "WEB",
          project_id: "22222222-2222-4222-8222-222222222222",
          priority: "medium",
          state_id: "33333333-3333-4333-8333-333333333333",
          type_id: null,
        },
      ],
    });
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/entity");
    const { onDocumentChange } = mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Entity reference" }));
    const row = await screen.findByRole("button", { name: /WEB-42 Fix login/ });
    fireEvent.mouseDown(row);
    expect(page.insertEntityReference).toHaveBeenCalledWith({
      entity_name: "issue",
      entity_identifier: "11111111-1111-4111-8111-111111111111",
      project_identifier: "WEB",
      workspace_identifier: "acme",
      project_id: "22222222-2222-4222-8222-222222222222",
      "data-label": "Fix login",
    });
    expect(onDocumentChange).toHaveBeenCalledWith("<p>html</p>");
  });

  it("shows the picker's empty state when its search fails", async () => {
    api.searchEntityPreviews.mockRejectedValue(new Error("offline"));
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/embed");
    mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Embed work item" }));
    await waitFor(() => expect(api.searchEntityPreviews).toHaveBeenCalled());
    expect(await screen.findByText("No work items match.")).toBeInTheDocument();
  });

  it("dismisses the work-item picker without inserting", async () => {
    api.searchEntityPreviews.mockResolvedValue({});
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/embed");
    mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Embed work item" }));
    fireEvent.click(await screen.findByRole("button", { name: "Dismiss work-item picker" }));
    expect(screen.queryByTestId("page-entity-picker")).not.toBeInTheDocument();
    expect(page.insertEmbed).not.toHaveBeenCalled();
  });

  it.each([
    ["align", "Left align", ["applyCommand", "alignLeft"]],
    ["align", "Center align", ["applyCommand", "alignCenter"]],
    ["align", "Right align", ["applyCommand", "alignRight"]],
    ["inline", "Inline code", ["applyCommand", "code"]],
    ["link", "Link", ["setLinkOpen", null]],
    ["mention", "Mention", ["insertText", "@"]],
  ] as const)("runs the %s query command %s", (query, label, expected) => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue(`/${query}`);
    const { onDocumentChange } = mount({ getHandle: () => page });
    const palette = screen.getByTestId("page-slash-palette");
    fireEvent.mouseDown(within(palette).getByRole("button", { name: label }));
    if (expected[0] === "setLinkOpen") {
      expect(screen.getByTestId("page-link-bubble")).toBeInTheDocument();
    } else if (expected[0] === "insertText") {
      expect(page.insertText).toHaveBeenCalledWith("@");
    } else {
      expect(page.applyCommand).toHaveBeenCalledWith(expected[1]);
    }
    expect(onDocumentChange).toHaveBeenCalledWith("<p>html</p>");
  });

  it("runs the query-only inline commands from the palette", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/under");
    mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getAllByRole("button", { name: "Underline" })[1] as HTMLElement);
    expect(page.applyCommand).toHaveBeenCalledWith("underline");
  });

  it("searches project-scoped people and inserts the chosen mention", async () => {
    api.searchMentionPeople.mockResolvedValue([
      { member__id: "u-1", member__display_name: "Ada", member__avatar_url: null },
    ]);
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("@ad");
    mount({ getHandle: () => page, html: "<p>@ad</p>" });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 150));
    });
    await waitFor(() => expect(api.searchMentionPeople).toHaveBeenCalled());
    expect(api.searchMentionPeople).toHaveBeenCalledWith(expect.anything(), "acme", {
      query: "ad",
      projectId: "project-1",
      count: 5,
    });
    fireEvent.mouseDown(await screen.findByRole("button", { name: /Ada/ }));
    expect(page.deleteBeforeCaret).toHaveBeenCalledWith(3);
    expect(page.insertMention).toHaveBeenCalledWith({ id: "u-1", name: "Ada" });
  });

  it("shows the selection bubble and forwards formatting and link actions", () => {
    const page = handle();
    (page.linkHref as ReturnType<typeof vi.fn>).mockReturnValue("https://example.com");
    const { onDocumentChange } = mount({ getHandle: () => page });
    fireEvent.click(screen.getByRole("button", { name: "Underline" }));
    expect(page.applyCommand).toHaveBeenCalledWith("underline");
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Paste a link" }), {
      target: { value: "https://example.com/new" },
    });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Paste a link" }), { key: "Enter" });
    expect(page.setLink).toHaveBeenCalledWith("https://example.com/new");
    expect(onDocumentChange).toHaveBeenCalled();
  });

  it("forwards duplicate and delete from the block menu", () => {
    const page = handle();
    const hovered = { from: 0, to: 12, json: { type: "paragraph" }, top: 8 };
    (page.blockAtPoint as ReturnType<typeof vi.fn>).mockReturnValue(hovered);
    mount({ getHandle: () => page });
    fireEvent.mouseMove(document, { clientX: 120, clientY: 340 });
    fireEvent.click(screen.getByTestId("page-block-handle"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Duplicate" }));
    expect(page.duplicateBlock).toHaveBeenCalledWith(hovered);
    fireEvent.click(screen.getByTestId("page-block-handle"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(page.deleteBlock).toHaveBeenCalledWith(hovered);
  });

  it("keeps the hovered block while the pointer moves onto the handle itself (PAGE-037)", () => {
    const page = handle();
    const hovered = { from: 12, to: 20, json: { type: "paragraph" }, top: 40 };
    (page.blockAtPoint as ReturnType<typeof vi.fn>).mockReturnValue(hovered);
    mount({ getHandle: () => page });
    fireEvent.mouseMove(document, { clientX: 120, clientY: 340 });
    const handleButton = screen.getByTestId("page-block-handle");
    // The handle lives in the gutter, where the editor cannot place the
    // point: the hover target must survive the pointer moving onto it.
    (page.blockAtPoint as ReturnType<typeof vi.fn>).mockReturnValue(null);
    fireEvent.mouseMove(handleButton, { clientX: 30, clientY: 344 });
    fireEvent.mouseMove(handleButton, { clientX: 30, clientY: 346 });
    expect(screen.getByTestId("page-block-handle")).toBeInTheDocument();
    fireEvent.click(handleButton);
    fireEvent.click(screen.getByRole("menuitem", { name: "Move down" }));
    expect(page.moveBlock).toHaveBeenCalledWith(hovered, "down");
  });

  it("hides the handle once the pointer leaves the editor and its handle", () => {
    const page = handle();
    (page.blockAtPoint as ReturnType<typeof vi.fn>).mockReturnValue({
      from: 0,
      to: 12,
      json: { type: "paragraph" },
      top: 0,
    });
    mount({ getHandle: () => page });
    fireEvent.mouseMove(document, { clientX: 120, clientY: 340 });
    expect(screen.getByTestId("page-block-handle")).toBeInTheDocument();
    (page.blockAtPoint as ReturnType<typeof vi.fn>).mockReturnValue(null);
    fireEvent.mouseMove(document, { clientX: 900, clientY: 900 });
    expect(screen.queryByTestId("page-block-handle")).not.toBeInTheDocument();
  });

  it("opens the link fields when a click lands inside a link (PAGE-043)", () => {
    const page = handle();
    (page.hasSelection as ReturnType<typeof vi.fn>).mockReturnValue(false);
    (page.linkHref as ReturnType<typeof vi.fn>).mockReturnValue("https://example.com/docs");
    mount({ getHandle: () => page });
    expect(screen.queryByTestId("page-link-bubble")).not.toBeInTheDocument();
    fireEvent.click(document.body);
    expect(screen.getByRole("textbox", { name: "Paste a link" })).toHaveValue("https://example.com/docs");
    fireEvent.change(screen.getByRole("textbox", { name: "Paste a link" }), {
      target: { value: "https://example.com/edited" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply link" }));
    expect(page.setLink).toHaveBeenCalledWith("https://example.com/edited");
  });

  it("leaves a click outside any link alone", () => {
    const page = handle();
    (page.hasSelection as ReturnType<typeof vi.fn>).mockReturnValue(false);
    (page.linkHref as ReturnType<typeof vi.fn>).mockReturnValue(null);
    mount({ getHandle: () => page });
    fireEvent.click(document.body);
    expect(screen.queryByTestId("page-link-bubble")).not.toBeInTheDocument();
    expect(page.setLink).not.toHaveBeenCalled();
  });

  it("anchors the palette beside the caret rather than at the body's end (PAGE-037)", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/head");
    (page.caretAnchor as ReturnType<typeof vi.fn>).mockReturnValue({ top: 180, left: 24 });
    mount({ getHandle: () => page });
    const palette = screen.getByTestId("page-slash-palette").parentElement;
    expect(palette?.style.top).toBe("204px");
    expect(palette?.style.left).toBe("24px");
  });

  it("clears one colour kind and applies the other before reporting the edit", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/gray");
    const first = mount({ getHandle: () => page, html: "<p>html</p>" });
    fireEvent.mouseDown(screen.getAllByRole("button", { name: "Gray" })[0] as HTMLElement);
    expect(page.applyColor).toHaveBeenCalledWith("text", "gray");
    expect(first.onDocumentChange).toHaveBeenCalledWith("<p>html</p>");
    first.unmount();

    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/background");
    const second = mount({ getHandle: () => page, html: "<p>html2</p>" });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Default background" }));
    expect(page.clearColor).toHaveBeenCalledWith("background");
    expect(second.onDocumentChange).toHaveBeenCalledWith("<p>html</p>");
  });

  it("opens the inline emoji picker from its catalogue row and inserts the glyph", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/emoji");
    const { onDocumentChange } = mount({ getHandle: () => page });
    expect(screen.queryByTestId("page-emoji-picker")).not.toBeInTheDocument();
    fireEvent.mouseDown(screen.getByRole("button", { name: "Emoji" }));
    const picker = screen.getByTestId("page-emoji-picker");
    const first = picker.querySelector('[data-testid="page-emoji-option"]');
    if (first === null) throw new Error("the picker has no entries");
    fireEvent.mouseDown(first);
    expect(page.insertEmoji).toHaveBeenCalledWith(first.textContent);
    expect(onDocumentChange).toHaveBeenCalledWith("<p>html</p>");
    expect(screen.queryByTestId("page-emoji-picker")).not.toBeInTheDocument();
  });

  it("closes the emoji picker from its own dismiss control (PAGE-042)", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/emoji");
    mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Emoji" }));
    expect(screen.getByTestId("page-emoji-picker")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss emoji picker" }));
    expect(screen.queryByTestId("page-emoji-picker")).not.toBeInTheDocument();
  });

  it("dismisses the slash palette and the emoji picker with Escape (PAGE-037/042)", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/head");
    const { unmount } = mount({ getHandle: () => page });
    expect(screen.getByRole("button", { name: "Heading 2" })).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("button", { name: "Heading 2" })).not.toBeInTheDocument();
    unmount();

    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/emoji");
    mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Emoji" }));
    expect(screen.getByTestId("page-emoji-picker")).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("page-emoji-picker")).not.toBeInTheDocument();
  });

  it("targets the hovered block, not the caret's block, and offers move up/down (PAGE-037)", () => {
    const page = handle();
    const hovered = { from: 12, to: 20, json: { type: "paragraph" }, top: 40 };
    (page.blockAtPoint as ReturnType<typeof vi.fn>).mockReturnValue(hovered);
    mount({ getHandle: () => page });
    fireEvent.mouseMove(document, { clientX: 120, clientY: 340 });
    fireEvent.click(screen.getByRole("button", { name: "Block actions" }));
    const menu = screen.getByRole("menu");
    for (const label of ["Move up", "Move down", "Duplicate", "Delete"]) {
      expect(within(menu).getByRole("menuitem", { name: label })).toBeInTheDocument();
    }
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Move up" }));
    expect(page.moveBlock).toHaveBeenCalledWith(hovered, "up");
    expect(page.duplicateBlock).not.toHaveBeenCalled();
    expect(page.deleteBlock).not.toHaveBeenCalled();
  });

  it("dismisses the entity picker with Escape without inserting anything", async () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/entity");
    mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Entity reference" }));
    await screen.findByTestId("page-entity-picker");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("page-entity-picker")).not.toBeInTheDocument();
    expect(page.insertEntityReference).not.toHaveBeenCalled();
  });

  it("applies and removes a link, then closes the editor from its own control", () => {
    const page = handle();
    (page.linkHref as ReturnType<typeof vi.fn>).mockReturnValue("https://example.com");
    const { onDocumentChange } = mount({ getHandle: () => page });
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Paste a link" }), {
      target: { value: "https://example.com/new" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply link" }));
    expect(page.setLink).toHaveBeenCalledWith("https://example.com/new");
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove link" }));
    expect(page.unsetLink).toHaveBeenCalledTimes(1);
    expect(onDocumentChange).toHaveBeenCalled();
  });

  it("shows a link editor for a selection that has no active link", () => {
    const page = handle();
    (page.linkHref as ReturnType<typeof vi.fn>).mockReturnValue(null);
    mount({ getHandle: () => page });
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    expect(screen.getByRole("textbox", { name: "Paste a link" })).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    expect(screen.queryByRole("textbox", { name: "Paste a link" })).not.toBeInTheDocument();
  });

  it("renders no surface while the editor has not mounted", () => {
    const view = render(
      <PageBodyInteractionLayer
        workspaceSlug="acme"
        projectId="project-1"
        getHandle={() => null}
        html="<p>/emoji</p>"
        onPickImage={vi.fn()}
        onDocumentChange={vi.fn()}
        assistantReady={false}
      />
    );
    expect(view.queryByTestId("page-emoji-picker")).not.toBeInTheDocument();
    fireEvent.mouseMove(document, { clientX: 120, clientY: 340 });
    expect(view.queryByTestId("page-block-handle")).not.toBeInTheDocument();
  });

  it("ignores a block action once the editor went away (PAGE-037)", () => {
    const page = handle();
    (page.blockAtPoint as ReturnType<typeof vi.fn>).mockReturnValue({
      from: 0,
      to: 12,
      json: { type: "paragraph" },
      top: 0,
    });
    const { rerender } = mount({ getHandle: () => page });
    fireEvent.mouseMove(document, { clientX: 120, clientY: 340 });
    rerender(
      <PageBodyInteractionLayer
        workspaceSlug="acme"
        projectId="project-1"
        getHandle={() => null}
        html="<p>html</p>"
        onPickImage={vi.fn()}
        onDocumentChange={vi.fn()}
        assistantReady={false}
      />
    );
    fireEvent.click(screen.getByTestId("page-block-handle"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Duplicate" }));
    expect(page.duplicateBlock).not.toHaveBeenCalled();
  });

  it("dismisses the mention popup without inserting", async () => {
    api.searchMentionPeople.mockResolvedValue([
      { member__id: "u-1", member__display_name: "Ada", member__avatar_url: null },
    ]);
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("@ad");
    mount({ getHandle: () => page, html: "<p>@ad</p>" });
    fireEvent.click(await screen.findByRole("button", { name: "Dismiss mentions" }));
    expect(page.insertMention).not.toHaveBeenCalled();
  });

  it("reports an empty suggestion list when the search fails", async () => {
    api.searchMentionPeople.mockRejectedValue(new Error("offline"));
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("@ad");
    mount({ getHandle: () => page, html: "<p>@ad</p>" });
    await act(async () => {
      await Promise.resolve();
    });
    await waitFor(() => expect(api.searchMentionPeople).toHaveBeenCalled());
    expect(screen.queryByTestId("page-mention-suggestions")).not.toBeInTheDocument();
  });

  it("dismisses the slash palette without inserting", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/head");
    mount({ getHandle: () => page });
    fireEvent.click(screen.getByRole("button", { name: "Close block menu" }));
    expect(screen.queryByTestId("page-slash-palette")).not.toBeInTheDocument();
    expect(page.applyCatalogBlock).not.toHaveBeenCalled();
  });

  it("closes the link editor from the bubble's own close control", () => {
    const page = handle();
    (page.linkHref as ReturnType<typeof vi.fn>).mockReturnValue("https://example.com");
    mount({ getHandle: () => page });
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    expect(screen.getByTestId("page-link-bubble")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    expect(screen.queryByTestId("page-link-bubble")).not.toBeInTheDocument();
    expect(page.setLink).not.toHaveBeenCalled();
  });

  it("ignores a late mention answer after the query is gone", async () => {
    let resolve: ((people: unknown[]) => void) | undefined;
    api.searchMentionPeople.mockReturnValue(
      new Promise((done) => {
        resolve = done;
      })
    );
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("@ad");
    const { rerender } = mount({ getHandle: () => page, html: "<p>@ad</p>" });
    await act(async () => {
      await new Promise((done) => setTimeout(done, 150));
    });
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("");
    rerender(
      <PageBodyInteractionLayer
        workspaceSlug="acme"
        projectId="project-1"
        getHandle={() => page}
        html="<p>done</p>"
        onPickImage={vi.fn()}
        onDocumentChange={vi.fn()}
        assistantReady={false}
      />
    );
    await act(async () => {
      resolve?.([{ member__id: "u-1", member__display_name: "Ada", member__avatar_url: null }]);
      await Promise.resolve();
    });
    expect(screen.queryByTestId("page-mention-suggestions")).not.toBeInTheDocument();
  });

  it("carries an edition embed slot through the layer", () => {
    mount({
      embed: { id: "e", entity_identifier: "I", project_identifier: "P", workspace_identifier: "w", entity_name: "n" },
    });
    expect(screen.getByTestId("page-embed-slot")).toBeInTheDocument();
  });

  it("ignores a chosen entry after the editor went away", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/head");
    const { rerender } = mount({ getHandle: () => page });
    rerender(
      <PageBodyInteractionLayer
        workspaceSlug="acme"
        projectId="project-1"
        getHandle={() => null}
        html="<p>/head</p>"
        onPickImage={vi.fn()}
        onDocumentChange={vi.fn()}
        assistantReady={false}
      />
    );
    fireEvent.mouseDown(screen.getByRole("button", { name: "Heading 2" }));
    expect(page.applyCatalogBlock).not.toHaveBeenCalled();
  });

  it("ignores the bubble commands after the editor went away", () => {
    const page = handle();
    const { rerender } = mount({ getHandle: () => page });
    rerender(
      <PageBodyInteractionLayer
        workspaceSlug="acme"
        projectId="project-1"
        getHandle={() => null}
        html="<p>html</p>"
        onPickImage={vi.fn()}
        onDocumentChange={vi.fn()}
        assistantReady={false}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
  });

  it("keeps the mention list empty when the search rejects while mounted", async () => {
    let reject: ((error: Error) => void) | undefined;
    api.searchMentionPeople.mockReturnValue(
      new Promise((_done, fail) => {
        reject = fail;
      })
    );
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("@ad");
    mount({ getHandle: () => page, html: "<p>@ad</p>" });
    await act(async () => {
      await new Promise((done) => setTimeout(done, 150));
    });
    await act(async () => {
      reject?.(new Error("offline"));
      await Promise.resolve();
    });
    expect(screen.queryByTestId("page-mention-suggestions")).not.toBeInTheDocument();
  });

  it("dismisses the mention popup and skips the query strip once the caret leaves", async () => {
    api.searchMentionPeople.mockResolvedValue([
      { member__id: "u-1", member__display_name: "Ada", member__avatar_url: null },
    ]);
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("@ad");
    mount({ getHandle: () => page, html: "<p>@ad</p>" });
    fireEvent.click(await screen.findByRole("button", { name: "Dismiss mentions" }));
    expect(page.deleteBeforeCaret).not.toHaveBeenCalled();
  });

  it("ignores emoji, mention and link commands once the handle is gone", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/emoji");
    const { rerender } = mount({ getHandle: () => page });
    rerender(
      <PageBodyInteractionLayer
        workspaceSlug="acme"
        projectId="project-1"
        getHandle={() => null}
        html="<p>/emoji</p>"
        onPickImage={vi.fn()}
        onDocumentChange={vi.fn()}
        assistantReady={false}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    expect(page.applyCommand).not.toHaveBeenCalled();
  });

  it("drops a rejected mention answer after the layer closes", async () => {
    let reject: ((error: Error) => void) | undefined;
    api.searchMentionPeople.mockReturnValue(
      new Promise((_done, fail) => {
        reject = fail;
      })
    );
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("@ad");
    const view = mount({ getHandle: () => page, html: "<p>@ad</p>" });
    await act(async () => {
      await new Promise((done) => setTimeout(done, 150));
    });
    view.unmount();
    await act(async () => {
      reject?.(new Error("offline"));
      await Promise.resolve();
    });
  });

  it("keeps the emoji picker inert once the editor is torn down", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/emoji");
    const { rerender } = mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Emoji" }));
    rerender(
      <PageBodyInteractionLayer
        workspaceSlug="acme"
        projectId="project-1"
        getHandle={() => null}
        html="<p>/emoji</p>"
        onPickImage={vi.fn()}
        onDocumentChange={vi.fn()}
        assistantReady={false}
      />
    );
    const picker = screen.getByTestId("page-emoji-picker");
    const first = picker.querySelector("button");
    if (first === null) throw new Error("the picker has no entries");
    fireEvent.mouseDown(first);
    expect(page.insertEmoji).not.toHaveBeenCalled();
  });

  it("keeps a chosen mention inert once the editor is torn down", async () => {
    api.searchMentionPeople.mockResolvedValue([
      { member__id: "u-1", member__display_name: "Ada", member__avatar_url: null },
    ]);
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("@ad");
    const { rerender } = mount({ getHandle: () => page, html: "<p>@ad</p>" });
    const row = await screen.findByRole("button", { name: /Ada/ });
    rerender(
      <PageBodyInteractionLayer
        workspaceSlug="acme"
        projectId="project-1"
        getHandle={() => null}
        html="<p>@ad</p>"
        onPickImage={vi.fn()}
        onDocumentChange={vi.fn()}
        assistantReady={false}
      />
    );
    fireEvent.mouseDown(row);
    expect(page.insertMention).not.toHaveBeenCalled();
  });

  it("keeps link edits inert once the editor is torn down", () => {
    const page = handle();
    (page.linkHref as ReturnType<typeof vi.fn>).mockReturnValue("https://example.com");
    const { rerender } = mount({ getHandle: () => page });
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    rerender(
      <PageBodyInteractionLayer
        workspaceSlug="acme"
        projectId="project-1"
        getHandle={() => null}
        html="<p>html</p>"
        onPickImage={vi.fn()}
        onDocumentChange={vi.fn()}
        assistantReady={false}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply link" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove link" }));
    expect(page.setLink).not.toHaveBeenCalled();
    expect(page.unsetLink).not.toHaveBeenCalled();
  });
});

describe("assistant menu (PAGE-075)", () => {
  function hover(page: ReturnType<typeof handle>): void {
    (page.blockAtPoint as ReturnType<typeof vi.fn>).mockReturnValue({
      from: 0,
      to: 12,
      json: { type: "paragraph" },
      top: 8,
    });
    fireEvent.mouseMove(document, { clientX: 120, clientY: 340 });
  }

  function selected(page: ReturnType<typeof handle>): void {
    (page.selectionRange as ReturnType<typeof vi.fn>).mockReturnValue({ from: 2, to: 8 });
    (page.selectedText as ReturnType<typeof vi.fn>).mockReturnValue("chosen text");
  }

  async function answer(): Promise<void> {
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Summarize" } });
    fireEvent.click(screen.getByRole("button", { name: "Generate" }));
    await screen.findByTestId("page-assistant-reply");
  }

  it("renders no AI surface when the assistant is not ready (community pin)", () => {
    const page = handle();
    mount({ getHandle: () => page, assistantReady: false });
    expect(screen.queryByTestId("page-assistant-mount")).not.toBeInTheDocument();
    hover(page);
    expect(screen.getByTestId("page-block-handle")).toBeInTheDocument();
    expect(screen.queryByTestId("page-ai-handle")).not.toBeInTheDocument();
    expect(screen.queryByTestId("page-assistant-menu")).not.toBeInTheDocument();
  });

  it("opens the panel over the snapshotted selection", () => {
    const page = handle();
    selected(page);
    mount({ getHandle: () => page, assistantReady: true });
    expect(screen.getByTestId("page-assistant-mount")).toBeInTheDocument();
    hover(page);
    fireEvent.click(screen.getByTestId("page-ai-handle"));
    expect(screen.getByRole("dialog", { name: "Assistant" })).toBeInTheDocument();
    expect(assistantTask.mutateAsync).not.toHaveBeenCalled();
  });

  it("treats an empty selection as a no-op", () => {
    const page = handle();
    mount({ getHandle: () => page, assistantReady: true });
    hover(page);
    // Collapsed selection: no range.
    fireEvent.click(screen.getByTestId("page-ai-handle"));
    expect(screen.queryByTestId("page-assistant-menu")).not.toBeInTheDocument();
    // A range without text opens nothing either.
    (page.selectionRange as ReturnType<typeof vi.fn>).mockReturnValue({ from: 2, to: 8 });
    fireEvent.click(screen.getByTestId("page-ai-handle"));
    expect(screen.queryByTestId("page-assistant-menu")).not.toBeInTheDocument();
  });

  it("opens nothing once the editor went away", () => {
    const page = handle();
    selected(page);
    let live: ReturnType<typeof handle> | null = page;
    mount({ getHandle: () => live, assistantReady: true });
    hover(page);
    live = null;
    fireEvent.click(screen.getByTestId("page-ai-handle"));
    expect(screen.queryByTestId("page-assistant-menu")).not.toBeInTheDocument();
  });

  it("applies the reply over the snapshotted range and closes", async () => {
    const page = handle();
    selected(page);
    const { onDocumentChange } = mount({ getHandle: () => page, assistantReady: true });
    hover(page);
    fireEvent.click(screen.getByTestId("page-ai-handle"));
    await answer();
    expect(assistantTask.mutateAsync).toHaveBeenCalledWith({ task: "Summarize", prompt: "chosen text" });
    fireEvent.click(screen.getByRole("button", { name: "Apply in place" }));
    expect(page.insertAt).toHaveBeenCalledWith({ from: 2, to: 8 }, "<p>done</p>");
    expect(onDocumentChange).toHaveBeenCalledWith("<p>html</p>");
    expect(screen.queryByTestId("page-assistant-menu")).not.toBeInTheDocument();
  });

  it("appends the reply below the snapshotted range and closes", async () => {
    const page = handle();
    selected(page);
    mount({ getHandle: () => page, assistantReady: true });
    hover(page);
    fireEvent.click(screen.getByTestId("page-ai-handle"));
    await answer();
    fireEvent.click(screen.getByRole("button", { name: "Append below" }));
    expect(page.insertAt).toHaveBeenCalledWith(8, "<p><p>done</p></p>");
    expect(screen.queryByTestId("page-assistant-menu")).not.toBeInTheDocument();
  });

  it("keeps the panel when the editor went away mid-turn", async () => {
    const page = handle();
    selected(page);
    let live: ReturnType<typeof handle> | null = page;
    mount({ getHandle: () => live, assistantReady: true });
    hover(page);
    fireEvent.click(screen.getByTestId("page-ai-handle"));
    await answer();
    live = null;
    fireEvent.click(screen.getByRole("button", { name: "Apply in place" }));
    expect(page.insertAt).not.toHaveBeenCalled();
    expect(screen.getByTestId("page-assistant-menu")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Append below" }));
    expect(page.insertAt).not.toHaveBeenCalled();
    expect(screen.getByTestId("page-assistant-menu")).toBeInTheDocument();
  });

  it("closes the panel on Close and on Escape", () => {
    const page = handle();
    selected(page);
    mount({ getHandle: () => page, assistantReady: true });
    hover(page);
    fireEvent.click(screen.getByTestId("page-ai-handle"));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByTestId("page-assistant-menu")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("page-ai-handle"));
    expect(screen.getByTestId("page-assistant-menu")).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("page-assistant-menu")).not.toBeInTheDocument();
  });
});

describe("cloud slash registry (PAGE-078)", () => {
  it("adds nothing to the palette when the registry is absent", () => {
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/");
    mount({ getHandle: () => page });
    const palette = screen.getByTestId("page-slash-palette");
    expect(within(palette).queryByRole("button", { name: "Cloud callout" })).not.toBeInTheDocument();
  });

  it("lists registry snippets after the community catalogue when filled", () => {
    edition.slots = {
      "page.slashRegistry": { entries: [{ label: "Cloud callout", group: "Cloud", html: "<p>x</p>" }] },
    };
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/");
    mount({ getHandle: () => page });
    const palette = screen.getByTestId("page-slash-palette");
    const names = within(palette)
      .getAllByRole("button")
      .map((entry) => entry.textContent);
    expect(names.slice(0, 35)).toHaveLength(35);
    expect(names[35]).toBe("Cloud callout");
    expect(within(palette).getByText("Cloud")).toBeInTheDocument();
  });

  it("inserts a picked snippet where the trigger was", () => {
    edition.slots = {
      "page.slashRegistry": { entries: [{ label: "Cloud callout", group: "Cloud", html: "<p>x</p>" }] },
    };
    // A slash palette only exists at a collapsed caret, which carries no
    // selection range — the snippet lands at the caret position instead.
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/");
    (page.caretPosition as ReturnType<typeof vi.fn>).mockReturnValue(4);
    const { onDocumentChange } = mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Cloud callout" }));
    expect(page.deleteBeforeCaret).toHaveBeenCalledWith(1);
    expect(page.insertAt).toHaveBeenCalledWith(4, "<p>x</p>");
    expect(onDocumentChange).toHaveBeenCalledWith("<p>html</p>");
    expect(screen.queryByTestId("page-slash-palette")).not.toBeInTheDocument();
  });

  it("closes the palette without inserting when the caret is gone", () => {
    edition.slots = {
      "page.slashRegistry": { entries: [{ label: "Cloud callout", group: "Cloud", html: "<p>x</p>" }] },
    };
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/");
    mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Cloud callout" }));
    expect(page.insertAt).not.toHaveBeenCalled();
    expect(screen.queryByTestId("page-slash-palette")).not.toBeInTheDocument();
  });
});
