// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueComment, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

vi.mock("../../../shared/editor/RichTextEditor.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/editor/RichTextEditor.js")>();
  // The composer has its own suite; here a plain textarea keeps the
  // thread tests off the lazy Tiptap chunk.
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
        defaultValue={initialHtml.replace(/<[^>]*>/g, "").trim()}
        onChange={(event) => onChange(`<p>${event.target.value}</p>`)}
      />
    );
  }
  return { ...original, RichTextEditor: FakeEditor, CommentRichTextEditor: FakeEditor };
});

vi.mock("../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/session/index.js")>();
  return { ...original, useMe: () => ({ data: { id: "523e4567-e89b-12d3-a456-426614174004" } }) };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { COMMENT_LINK_COPIED_TITLE, COMMENT_LINK_COPY_FAILED_TITLE, COMMENT_REMOVED_TITLE } from "./api.js";
import { CommentThread, commentIdFromHash, commentLinkFor } from "./CommentThread.js";

const ISSUE = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const ADA = "523e4567-e89b-12d3-a456-426614174004";
const COMMENT = "823e4567-e89b-12d3-a456-426614174008";

function comment(overrides: Partial<IssueComment> = {}): IssueComment {
  return {
    id: COMMENT,
    issue: ISSUE,
    project: PROJECT,
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
    comment_html: "<p>Thread words</p>",
    comment_stripped: "Thread words",
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

interface Recorded {
  method: string;
  url: string;
  body: string;
}

function stubTransport(handler: (call: Recorded) => { status: number; body: unknown }) {
  const calls: Recorded[] = [];
  createAppClient("https://api.test", async (url: string, init: TransportInit) => {
    const call = { method: init.method, url, body: init.body ?? "" };
    calls.push(call);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "x" }) };
    }
    const answer = handler(call);
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === "" ? "" : JSON.stringify(answer.body)),
    };
  });
  return calls;
}

function renderThread(props: Partial<React.ComponentProps<typeof CommentThread>> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <CommentThread
        workspaceSlug="acme"
        projectId={PROJECT}
        issueId={ISSUE}
        archived={false}
        published={false}
        {...props}
      />
    </QueryClientProvider>
  );
}

function stubClipboard(write: () => Promise<void>) {
  const writeText = vi.fn(write);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  return writeText;
}

beforeEach(() => {
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
  window.history.replaceState({}, "", "/acme/projects/p/issues/i");
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  resetAppClient();
  delete (navigator as { clipboard?: unknown }).clipboard;
});

describe("comment deep links", () => {
  it("pulls a comment id out of a hash and ignores other hashes", () => {
    expect(commentIdFromHash("#comment-abc")).toBe("abc");
    expect(commentIdFromHash("#comment-  abc  ")).toBe("abc");
    expect(commentIdFromHash("#comment-")).toBeNull();
    expect(commentIdFromHash("#issue-comment-1")).toBeNull();
    expect(commentIdFromHash("")).toBeNull();
  });

  it("builds a link from the page address without its current hash", () => {
    expect(
      commentLinkFor(
        { href: "https://p.test/acme/issues/i#comment-old", hash: "#comment-old", pathname: "/acme/issues/i" },
        "new"
      )
    ).toBe("https://p.test/acme/issues/i#comment-new");
    expect(commentLinkFor({ href: "https://p.test/a", hash: "", pathname: "/a" }, "x")).toBe(
      "https://p.test/a#comment-x"
    );
  });
});

// The lazy Tiptap chunk loads once per worker; under the full suite
// that import can outlast the default per-test timeout.
describe("CommentThread", () => {
  it("shows the composer, heading and cards once the history arrives", async () => {
    stubTransport(() => ({ status: 200, body: [comment()] }));
    renderThread();
    expect(screen.getByRole("heading", { name: "Comments" })).toBeInTheDocument();
    expect(await screen.findByText("Thread words", undefined, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Add comment" })).toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "Comment visibility" })).not.toBeInTheDocument();
  }, 20000);

  it("shows skeleton rows while the history is loading", async () => {
    stubTransport(() => ({ status: 200, body: [] }));
    renderThread();
    expect(document.querySelector('[aria-busy="true"]')).not.toBeNull();
    await waitFor(() => expect(document.querySelector('[aria-busy="true"]')).toBeNull());
  });

  it("shows the read failure copy when the history is refused", async () => {
    stubTransport(() => ({ status: 500, body: { error: "boom" } }));
    renderThread();
    expect(await screen.findByText("Couldn't load the comments.")).toBeInTheDocument();
  });

  it("keeps cards but drops the composer and menus on an archived issue", async () => {
    stubTransport(() => ({ status: 200, body: [comment()] }));
    renderThread({ archived: true });
    expect(await screen.findByText("Thread words", undefined, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Add comment" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Comment actions" })).not.toBeInTheDocument();
  });

  it("shows the access switch and marks on a published project", async () => {
    stubTransport(() => ({ status: 200, body: [comment()] }));
    renderThread({ published: true });
    expect(screen.getByRole("radiogroup", { name: "Comment visibility" })).toBeInTheDocument();
    expect(await screen.findByLabelText("Private comment", undefined, { timeout: 5000 })).toBeInTheDocument();
  });

  it("copies the absolute deep link for a comment", async () => {
    const writeText = stubClipboard(async () => undefined);
    stubTransport(() => ({ status: 200, body: [comment()] }));
    renderThread();
    await screen.findByText("Thread words", undefined, { timeout: 5000 });
    await screen.findByRole("button", { name: "Comment actions" }, { timeout: 5000 });
    fireEvent.click(screen.getByRole("button", { name: "Comment actions" }));
    const menu = await screen.findByRole("menu", undefined, { timeout: 5000 });
    await act(async () => {
      fireEvent.click(within(menu).getByRole("menuitem", { name: "Copy link" }));
    });
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    expect(writeText).toHaveBeenCalledWith(`${window.location.origin}/acme/projects/p/issues/i#comment-${COMMENT}`);
    expect(notifySuccess).toHaveBeenCalledWith(COMMENT_LINK_COPIED_TITLE);
  }, 20000);

  it("notices a clipboard refusal", async () => {
    stubClipboard(async () => Promise.reject(new Error("Denied")));
    stubTransport(() => ({ status: 200, body: [comment()] }));
    renderThread();
    await screen.findByText("Thread words", undefined, { timeout: 5000 });
    await screen.findByRole("button", { name: "Comment actions" }, { timeout: 5000 });
    fireEvent.click(screen.getByRole("button", { name: "Comment actions" }));
    const menu = await screen.findByRole("menu", undefined, { timeout: 5000 });
    await act(async () => {
      fireEvent.click(within(menu).getByRole("menuitem", { name: "Copy link" }));
    });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(COMMENT_LINK_COPY_FAILED_TITLE, "Denied"));
  }, 20000);

  it("omits the link entry for intake items", async () => {
    stubTransport(() => ({ status: 200, body: [comment()] }));
    renderThread({ isIntake: true });
    await screen.findByText("Thread words", undefined, { timeout: 5000 });
    await screen.findByRole("button", { name: "Comment actions" }, { timeout: 5000 });
    fireEvent.click(screen.getByRole("button", { name: "Comment actions" }));
    const menu = await screen.findByRole("menu", undefined, { timeout: 5000 });
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent)
    ).not.toContain("Copy link");
  }, 20000);

  it("scrolls to a comment named in the address and clears the highlight", async () => {
    window.history.replaceState({}, "", `/acme/projects/p/issues/i#comment-${COMMENT}`);
    stubTransport(() => ({ status: 200, body: [comment()] }));
    renderThread();
    const card = await screen.findByText("Thread words", undefined, { timeout: 5000 });
    const frame = card.closest("article")?.parentElement as HTMLElement;
    await waitFor(() => expect(frame.className).toContain("ring-1"));
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
    await new Promise((resolve) => setTimeout(resolve, 8100));
    await waitFor(() => expect(frame.className).not.toContain("ring-1"));
  }, 15000);

  it("follows a hash change to another comment", async () => {
    const second = comment({ id: "923e4567-e89b-12d3-a456-426614174009", comment_html: "<p>Second words</p>" });
    stubTransport(() => ({ status: 200, body: [comment(), second] }));
    renderThread();
    await screen.findByText("Second words", undefined, { timeout: 5000 });
    act(() => {
      window.history.replaceState({}, "", `/acme/projects/p/issues/i#comment-${second.id}`);
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    });
    const frame = screen.getByText("Second words").closest("article")?.parentElement as HTMLElement;
    await waitFor(() => expect(frame.className).toContain("ring-1"));
  });

  it("deletes from the card menu without asking", async () => {
    const calls = stubTransport((call) =>
      call.method === "DELETE" ? { status: 204, body: "" } : { status: 200, body: [comment()] }
    );
    renderThread();
    await screen.findByText("Thread words", undefined, { timeout: 5000 });
    await screen.findByRole("button", { name: "Comment actions" }, { timeout: 5000 });
    fireEvent.click(screen.getByRole("button", { name: "Comment actions" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete" }, { timeout: 5000 }));
    await waitFor(() => expect(calls.some((call) => call.method === "DELETE")).toBe(true));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(COMMENT_REMOVED_TITLE));
  });

  it("saves an edited body through the comment endpoint", async () => {
    let revised = false;
    const calls = stubTransport((call) => {
      if (call.method === "PATCH") {
        revised = true;
        return { status: 200, body: comment({ comment_html: "<p>Revised</p>", edited_at: "2026-10-18T10:00:00Z" }) };
      }
      return {
        status: 200,
        body: [revised ? comment({ comment_html: "<p>Revised</p>", edited_at: "2026-10-18T10:00:00Z" }) : comment()],
      };
    });
    const user = userEvent.setup();
    renderThread();
    await screen.findByText("Thread words", undefined, { timeout: 5000 });
    await screen.findByRole("button", { name: "Comment actions" }, { timeout: 5000 });
    fireEvent.click(screen.getByRole("button", { name: "Comment actions" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit" }, { timeout: 5000 }));
    const editable = (await screen.findByLabelText("Edit comment", undefined, {
      timeout: 5000,
    })) as HTMLTextAreaElement;
    editable.focus();
    editable.setSelectionRange(editable.value.length, editable.value.length);
    await user.keyboard(" more");
    await waitFor(() => expect(editable.value).toMatch(/more/), { timeout: 5000 });
    await user.click(screen.getByRole("button", { name: "Save comment" }));
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/comments/${COMMENT}/`);
    expect(JSON.parse(patch?.body ?? "{}").comment_html).toContain("more");
    expect(await screen.findByText("Revised", undefined, { timeout: 5000 })).toBeInTheDocument();
  });

  it("flips the access specifier from the menu", async () => {
    const calls = stubTransport((call) =>
      call.method === "PATCH"
        ? { status: 200, body: comment({ access: "EXTERNAL" }) }
        : { status: 200, body: [comment()] }
    );
    renderThread({ published: true });
    await screen.findByText("Thread words", undefined, { timeout: 5000 });
    await screen.findByRole("button", { name: "Comment actions" }, { timeout: 5000 });
    fireEvent.click(screen.getByRole("button", { name: "Comment actions" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Switch to public comment" }, { timeout: 5000 }));
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(JSON.parse(calls.find((call) => call.method === "PATCH")?.body ?? "{}")).toEqual({ access: "EXTERNAL" });
  });

  it("folds a comment through its labels", async () => {
    const calls = stubTransport((call) =>
      call.method === "PATCH"
        ? { status: 200, body: comment({ labels: ["fold"] }) }
        : { status: 200, body: [comment()] }
    );
    renderThread();
    await screen.findByText("Thread words", undefined, { timeout: 5000 });
    await screen.findByRole("button", { name: "Comment actions" }, { timeout: 5000 });
    fireEvent.click(screen.getByRole("button", { name: "Comment actions" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Fold comment" }, { timeout: 5000 }));
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(JSON.parse(calls.find((call) => call.method === "PATCH")?.body ?? "{}")).toEqual({ labels: ["fold"] });
  });
}, 20_000);
