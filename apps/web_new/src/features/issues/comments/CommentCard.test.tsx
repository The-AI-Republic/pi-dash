// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import userEvent from "@testing-library/user-event";
import type { IssueComment } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CommentCard, commentMenuItems, COMMENT_HIGHLIGHT_MS, commentDomId } from "./CommentCard.js";

// The lazy Tiptap chunk is slow under the full suite; the card's own
// menu, fold and edit wiring is what this file pins.
vi.mock("../../../shared/editor/RichTextEditor.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/editor/RichTextEditor.js")>();
  function FakeEditor({
    initialHtml,
    onChange,
    label,
  }: {
    initialHtml: string;
    onChange: (html: string) => void;
    label: string;
  }) {
    return (
      <textarea
        aria-label={label}
        defaultValue={initialHtml
          .replace(/<[^>]*>/g, "")
          .replace(/\s+/g, " ")
          .trim()}
        onInput={(event) => onChange(`<p>${event.currentTarget.value}</p>`)}
      />
    );
  }
  return { ...original, RichTextEditor: FakeEditor, CommentRichTextEditor: FakeEditor };
});

const ADA = "523e4567-e89b-12d3-a456-426614174004";
const BOB = "623e4567-e89b-12d3-a456-426614174005";

function comment(overrides: Partial<IssueComment> = {}): IssueComment {
  return {
    id: "823e4567-e89b-12d3-a456-426614174008",
    issue: "223e4567-e89b-12d3-a456-426614174001",
    project: "323e4567-e89b-12d3-a456-426614174002",
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    actor: ADA,
    actor_detail: {
      id: ADA,
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: null,
      display_name: "Ada L",
      is_bot: false,
    },
    comment_html: "<p>Hello <strong>there</strong></p>",
    comment_stripped: "Hello there",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "INTERNAL",
    edited_at: null,
    comment_reactions: [],
    speaker_type: "human",
    speaker_label: "",
    speaker_agent_run_id: null,
    parent: null,
    external_source: null,
    external_id: null,
    is_synced: false,
    description: null,
    created_at: "2026-10-16T22:00:00Z",
    updated_at: "2026-10-16T22:00:00Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

function renderCard(props: Partial<React.ComponentProps<typeof CommentCard>> = {}) {
  const handlers = {
    onEdit: vi.fn(async () => undefined),
    onDelete: vi.fn(),
    onToggleAccess: vi.fn(),
    onToggleFold: vi.fn(),
    onCopyLink: vi.fn(),
  };
  const view = render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { mutations: { retry: false } } })}>
      <CommentCard
        workspaceSlug="acme"
        projectId="p-1"
        comment={comment()}
        viewerId={ADA}
        readOnly={false}
        access={{ toggle: true, copyLink: true }}
        {...handlers}
        {...props}
      />
    </QueryClientProvider>
  );
  return { ...handlers, view };
}

async function openMenu(_user: ReturnType<typeof userEvent.setup>) {
  // Pointer geometry is not implemented in jsdom; the kit menus are
  // driven with a click event, as the shell menu tests do.
  fireEvent.click(screen.getByRole("button", { name: "Comment actions" }));
  return screen.findByRole("menu");
}

afterEach(() => {
  vi.unstubAllGlobals();
  resetAppClient();
});

// The lazy Tiptap chunk loads once per worker; under the full suite
// that import can outlast the default per-test timeout.
describe("CommentCard", () => {
  it("shows the author, age, body and deep-link id", () => {
    renderCard();
    expect(screen.getByText("Ada L")).toBeInTheDocument();
    expect(screen.getByText("Hello", { exact: false })).toBeInTheDocument();
    expect(document.getElementById(commentDomId(comment().id))).not.toBeNull();
    expect(commentDomId("abc")).toBe("comment-abc");
    expect(COMMENT_HIGHLIGHT_MS).toBe(8000);
  });

  it("names agent comments by their speaker label and falls back for blank labels", () => {
    const { view } = renderCard({ comment: comment({ speaker_type: "agent", speaker_label: "Reviewer" }) });
    expect(screen.getByText("Reviewer")).toBeInTheDocument();
    view.unmount();
    renderCard({ comment: comment({ speaker_type: "agent", speaker_label: "  " }) });
    expect(screen.getByText("AI Agent")).toBeInTheDocument();
  });

  it("marks an edited comment in the age line", () => {
    renderCard({ comment: comment({ edited_at: "2026-10-17T10:00:00Z" }) });
    const age = screen.getByRole("button", { name: /Commented .*, edited/ });
    expect(age).toHaveTextContent("(edited)");
  });

  it("offers edit, copy link, access, fold and delete to the author", async () => {
    const user = userEvent.setup();
    renderCard();
    const menu = await openMenu(user);
    expect(Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent)).toEqual([
      "Edit",
      "Copy link",
      "Switch to public comment",
      "Fold comment",
      "Delete",
    ]);
  });

  it("hides edit and delete from everyone else and keeps fold", async () => {
    const user = userEvent.setup();
    renderCard({ viewerId: BOB });
    const menu = await openMenu(user);
    expect(Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent)).toEqual([
      "Copy link",
      "Switch to public comment",
      "Fold comment",
    ]);
  });

  it("drops the link on intake items and the access switch on unpublished projects", async () => {
    const user = userEvent.setup();
    const { view } = renderCard({ access: { toggle: false, copyLink: false } });
    const menu = await openMenu(user);
    expect(Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent)).toEqual([
      "Edit",
      "Fold comment",
      "Delete",
    ]);
    view.unmount();
    renderCard({ access: { toggle: false, copyLink: true }, viewerId: BOB });
    const second = await openMenu(user);
    expect(Array.from(second.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent)).not.toContain(
      "Switch to public comment"
    );
  });

  it("labels the access switch by the comment's current visibility", async () => {
    const user = userEvent.setup();
    renderCard({ comment: comment({ access: "EXTERNAL" }) });
    expect(screen.getByLabelText("Public comment")).toBeInTheDocument();
    const menu = await openMenu(user);
    expect(Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent)).toContain(
      "Switch to private comment"
    );
  });

  it("routes each menu choice to its handler", async () => {
    const user = userEvent.setup();
    const { onCopyLink, onToggleAccess, onToggleFold, onDelete } = renderCard();
    await openMenu(user);
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    expect(onCopyLink).toHaveBeenCalledWith(comment());
    await openMenu(user);
    fireEvent.click(screen.getByRole("menuitem", { name: "Switch to public comment" }));
    expect(onToggleAccess).toHaveBeenCalledWith(comment());
    await openMenu(user);
    fireEvent.click(screen.getByRole("menuitem", { name: "Fold comment" }));
    expect(onToggleFold).toHaveBeenCalledWith(comment());
    await openMenu(user);
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(onDelete).toHaveBeenCalledWith(comment().id);
  });

  it("opens the inline editor for the author and saves the new body", async () => {
    const user = userEvent.setup();
    createAppClient("https://api.test", async () => ({
      status: 204,
      url: "",
      headers: { get: () => null },
      text: async () => "",
    }));
    const { onEdit } = renderCard();
    await openMenu(user);
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    const editable = (await screen.findByLabelText("Edit comment", undefined, {
      timeout: 5000,
    })) as HTMLTextAreaElement;
    expect(editable.value).toBe("Hello there");
    editable.focus();
    editable.setSelectionRange(editable.value.length, editable.value.length);
    await user.keyboard(" more");
    await waitFor(() => expect(editable.value).toMatch(/more/));
    await user.click(screen.getByRole("button", { name: "Save comment" }));
    await waitFor(() => expect(onEdit).toHaveBeenCalledTimes(1));
    expect(onEdit).toHaveBeenCalledWith(comment().id, expect.stringContaining("more"));
    await waitFor(() => expect(screen.queryByLabelText("Edit comment")).not.toBeInTheDocument());
  });

  it("keeps the editor open when the save is refused and closes it on cancel", async () => {
    const user = userEvent.setup();
    createAppClient("https://api.test", async () => ({
      status: 204,
      url: "",
      headers: { get: () => null },
      text: async () => "",
    }));
    const onEdit = vi.fn(async () => Promise.reject(new Error("nope")));
    renderCard({ onEdit });
    await openMenu(user);
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    await screen.findByLabelText("Edit comment");
    await user.click(screen.getByRole("button", { name: "Save comment" }));
    await waitFor(() => expect(onEdit).toHaveBeenCalled());
    expect(await screen.findByLabelText("Edit comment")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel editing" }));
    expect(screen.queryByLabelText("Edit comment")).not.toBeInTheDocument();
  });

  it("folds the body behind an expand toggle for everyone", async () => {
    const user = userEvent.setup();
    renderCard({ viewerId: BOB, comment: comment({ labels: ["fold"] }) });
    expect(screen.queryByText("Hello", { exact: false })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Folded comment · Click to expand" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Folded comment · Click to expand" }));
    expect(screen.getByText("Hello", { exact: false })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Click to collapse" }));
    expect(screen.queryByText("Hello", { exact: false })).not.toBeInTheDocument();
  });

  it("shows the unfold entry for a folded comment", async () => {
    const user = userEvent.setup();
    renderCard({ comment: comment({ labels: ["fold"] }) });
    const menu = await openMenu(user);
    expect(Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent)).toContain(
      "Unfold comment"
    );
  });

  it("shows an empty-body note", () => {
    renderCard({ comment: comment({ comment_html: "<p></p>", comment_stripped: "" }) });
    expect(screen.getByText("This comment has no text.")).toBeInTheDocument();
  });

  it("is read-only on an archived issue: no menu and no editor", () => {
    renderCard({ readOnly: true });
    expect(screen.queryByRole("button", { name: "Comment actions" })).not.toBeInTheDocument();
    expect(screen.getByText("Hello", { exact: false })).toBeInTheDocument();
  });

  it("renders the author avatar when the profile carries one", () => {
    const { view } = renderCard({
      comment: comment({
        actor_detail: { ...comment().actor_detail!, avatar_url: "https://cdn.test/ada.png" },
      }),
    });
    expect(view.container.querySelector('img[src="https://cdn.test/ada.png"]')).not.toBeNull();
  });

  it("builds menu items with the fold icon flipped by state", () => {
    const handlers = {
      onEdit: vi.fn(),
      onCopyLink: vi.fn(),
      onToggleAccess: vi.fn(),
      onToggleFold: vi.fn(),
      onDelete: vi.fn(),
    };
    const folded = commentMenuItems(
      comment({ labels: ["fold"] }),
      { viewerId: BOB, accessToggle: false, copyLink: false },
      handlers
    );
    expect(folded.map((entry) => ("label" in entry ? entry.label : ""))).toEqual(["Unfold comment"]);
    const first = folded[0];
    if (first !== undefined && "onSelect" in first) first.onSelect();
    expect(handlers.onToggleFold).toHaveBeenCalled();
  });

  it("cancels an edit opened from the menu", async () => {
    const user = userEvent.setup();
    createAppClient("https://api.test", async () => ({
      status: 204,
      url: "",
      headers: { get: () => null },
      text: async () => "",
    }));
    renderCard();
    await openMenu(user);
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    expect(await screen.findByLabelText("Edit comment")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel editing" }));
    expect(screen.queryByLabelText("Edit comment")).not.toBeInTheDocument();
    expect(screen.getByText("Hello", { exact: false })).toBeInTheDocument();
  });
}, 20_000);
