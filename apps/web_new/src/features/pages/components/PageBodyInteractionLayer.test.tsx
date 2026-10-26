// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page interaction layer (parity/specs/pages.md §6.1–§6.4, PAGE-037–044):
// trigger detection, catalogue forwarding, mention search and selection
// commands all cross the same handle seam.

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
    setLink: vi.fn(),
    unsetLink: vi.fn(),
    html: vi.fn(() => "<p>html</p>"),
    linkHref: vi.fn(() => null),
    hasSelection: vi.fn(() => true),
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
      {...overrides}
    />
  );
  return { ...view, page, onDocumentChange, onPickImage };
}

afterEach(() => {
  api.searchMentionPeople.mockReset();
  api.searchEntityPreviews.mockReset();
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
    api.searchEntityPreviews.mockResolvedValue({});
    const page = handle();
    (page.textBeforeCaret as ReturnType<typeof vi.fn>).mockReturnValue("/entity");
    mount({ getHandle: () => page });
    fireEvent.mouseDown(screen.getByRole("button", { name: "Entity reference" }));
    expect(page.insertEntityReference).not.toHaveBeenCalled();
    expect(screen.getByText("No work items match.")).toBeInTheDocument();
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
    const { page } = mount();
    fireEvent.click(screen.getByTestId("page-block-handle"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Duplicate" }));
    expect(page.duplicateBlock).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("page-block-handle"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(page.deleteBlock).toHaveBeenCalledTimes(1);
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
    const first = picker.querySelector("button");
    if (first === null) throw new Error("the picker has no entries");
    fireEvent.mouseDown(first);
    expect(page.insertEmoji).toHaveBeenCalledWith(first.textContent);
    expect(onDocumentChange).toHaveBeenCalledWith("<p>html</p>");
    expect(screen.queryByTestId("page-emoji-picker")).not.toBeInTheDocument();
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

  it("ignores every command while the editor has not mounted", async () => {
    const view = render(
      <PageBodyInteractionLayer
        workspaceSlug="acme"
        projectId="project-1"
        getHandle={() => null}
        html="<p>/emoji</p>"
        onPickImage={vi.fn()}
        onDocumentChange={vi.fn()}
      />
    );
    expect(view.queryByTestId("page-emoji-picker")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Block actions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Duplicate" }));
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
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply link" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove link" }));
    expect(page.setLink).not.toHaveBeenCalled();
    expect(page.unsetLink).not.toHaveBeenCalled();
  });
});
